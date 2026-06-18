//! Per-file indexing pipeline (slice M5.2; batched per Decision Log **D20**).
//!
//! API anchor: `project_plan.md` §5.1 (step 3a–3e) + §3.2.2 (`write_in_transaction`). Owner:
//! `principal-engineering-lead`. Scenarios: `docs/TEST_STRATEGY.md#indexer` (full-index rows + D2).
//!
//! [`reindex_file_batched`] performs the per-file work of an index run for one discovered source
//! file: delete-first → compute hash → read content → detect language → parse → chunk →
//! `insert_chunks` → write the `files_metadata` row, all through a savepoint-scoped
//! [`BatchWriter`] so the whole run commits once (D20) while a single file's failure rolls back only
//! that file (D2). It returns the number of chunks inserted so the caller can accumulate
//! [`IndexStats`](super::IndexStats). Each fallible step surfaces a typed [`IndexError`] via `?`;
//! the caller (`index_all`) maps that per-file error to a savepoint rollback so the batch continues
//! (D2 degrade-and-continue) — there is no reachable `unwrap`/`expect`/`panic`.

use std::path::{Path, PathBuf};

use crate::chunker;
use crate::hasher;
use crate::parser::Parser;
use crate::storage::{BatchWriter, Storage};
use crate::types::{Chunk, FileMeta, Language};

use super::{detect_language, IndexError};

/// Per-file index work routed through a savepoint-scoped [`BatchWriter`] inside the run's single
/// outer transaction ([`Storage::write_in_transaction`], Decision Log **D20**): delete-first → hash
/// → read → parse → chunk → `insert_chunks` → `update_file_hash`, all participating in this file's
/// SAVEPOINT. A per-file failure (read/parse/chunk/store) returns `Err` so the caller's savepoint
/// rolls back ONLY this file (D2 isolation) while the committed siblings survive the single outer
/// commit; the read-stage `IndexError::File` is exactly the path the `unreadable_file_mid_batch_…`
/// guard exercises. Returns the count of chunks inserted so the caller can accumulate
/// [`IndexStats`](super::IndexStats) for the files whose savepoint committed.
///
/// # Errors
/// [`IndexError::Hash`] if hashing fails, [`IndexError::File`] if the content/metadata cannot be
/// read, [`IndexError::Parser`]/[`IndexError::Chunker`] on parse/chunk failure, and
/// [`IndexError::Storage`] if the delete/insert/upsert through `writer` fails.
pub fn reindex_file_batched(
    parser: &mut Parser,
    writer: &BatchWriter<'_>,
    path: &Path,
) -> Result<usize, IndexError> {
    // Delete-first (within the savepoint) avoids duplicate/stale chunks across re-indexes.
    writer
        .delete_chunks_for_file(path)
        .map_err(IndexError::Storage)?;

    let (chunks, meta) = extract_file(parser, path)?;

    // §5.1 step 3d–3e: store chunks + upsert the metadata row, both in this file's savepoint.
    writer.insert_chunks(&chunks).map_err(IndexError::Storage)?;
    writer
        .update_file_hash(path, &meta)
        .map_err(IndexError::Storage)?;

    Ok(chunks.len())
}

/// The read-only half of the per-file pipeline (§5.1 step 3a–3c): hash → read content+metadata →
/// detect language → parse → chunk → stamp `file_path`, returning the chunks plus the [`FileMeta`]
/// the caller persists. Shared so the write side stays the only difference between the autocommit
/// and batched (savepoint) paths. Does no DB writes, so it touches no transaction state.
fn extract_file(parser: &mut Parser, path: &Path) -> Result<(Vec<Chunk>, FileMeta), IndexError> {
    // §5.1 step 3a: content+mtime hash (the value stored in files_metadata.content_hash).
    let content_hash = hasher::compute_file_hash(path).map_err(IndexError::Hash)?;

    // §5.1 step 3b: read source + filesystem metadata (size, mtime) in one place.
    let content = std::fs::read_to_string(path).map_err(|source| IndexError::File {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata = std::fs::metadata(path).map_err(|source| IndexError::File {
        path: path.to_path_buf(),
        source,
    })?;
    let file_size = metadata.len();
    let mtime = file_mtime_secs(&metadata);

    // Language is known from discovery (only configured-language files are returned); recompute it
    // defensively so the pipeline is self-contained and the FileMeta language is correct.
    let language = detect_language(path).unwrap_or(Language::Python);

    // §5.1 step 3b–3c: parse → chunk. The chunker degrades a malformed tree internally (D2).
    let tree = parser
        .parse_file(path, &content, language)
        .map_err(IndexError::Parser)?;
    let mut chunks = chunker::chunk(&tree, &content, language).map_err(IndexError::Chunker)?;

    // The parser/chunker leave file_path empty (they are file-agnostic); stamp it so stored chunks
    // and the files_metadata row share the same key the tests query by (absolute-under-root path).
    for chunk in &mut chunks {
        chunk.file_path = path.to_path_buf();
    }

    let chunk_count = chunks.len();
    let meta = FileMeta {
        content_hash,
        mtime,
        file_size,
        language,
        chunk_count,
    };
    Ok((chunks, meta))
}

/// Of the candidate `files`, return those whose on-disk content hash differs from the stored hash
/// (`files_metadata.content_hash`) — i.e. files that are new (no stored hash) or changed (§5.2).
/// Unchanged files are skipped, which is what makes a re-index of an untouched repo a no-op.
///
/// A file whose hash cannot be computed (e.g. it vanished between discovery and here) is treated as
/// *changed* so the caller's per-file path can attempt it and isolate any failure (D2), rather than
/// being silently dropped from change detection.
///
/// # Errors
/// Propagates [`IndexError::Storage`] only if reading a stored hash fails (not isolatable per-file).
pub fn detect_changed_files(
    storage: &Storage,
    files: &[PathBuf],
) -> Result<Vec<PathBuf>, IndexError> {
    let mut changed = Vec::new();
    for path in files {
        let stored = storage.get_file_hash(path).map_err(IndexError::Storage)?;
        match hasher::compute_file_hash(path) {
            Ok(current) if stored.as_deref() == Some(current.as_str()) => {
                // Unchanged: stored hash equals the freshly-computed content+mtime hash → skip.
            }
            _ => changed.push(path.clone()),
        }
    }
    Ok(changed)
}

/// Modification time of `metadata` as Unix epoch seconds, or `0` when it is unavailable or predates
/// the epoch. The hash already encodes mtime authoritatively; the stored `FileMeta.mtime` is
/// bookkeeping, so a defensive `0` here is preferable to failing the whole file on a clock quirk.
fn file_mtime_secs(metadata: &std::fs::Metadata) -> u64 {
    metadata
        .modified()
        .ok()
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lock the load-bearing no-write guarantee at the unit level: after indexing a file, a second
    /// `detect_changed_files` over the same untouched file reports nothing changed — so the skip
    /// path (no delete/insert) is taken on a re-index of an unchanged repo (idempotency).
    #[test]
    fn detect_changed_files_empty_for_unchanged_repo() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        let file = root.join("solo.py");
        std::fs::write(&file, "def solo_fn():\n    return 1\n").expect("write fixture");

        let storage = Storage::new(&root.join("index.db")).expect("open storage");
        storage.init_schema().expect("init schema");
        let mut parser = Parser::new().expect("build parser");

        // Index once (through the batched D20 path): stores the content+mtime hash that
        // detect_changed_files will compare against.
        let files = [file.clone()];
        storage
            .write_in_transaction(&files, |writer, f| {
                reindex_file_batched(&mut parser, writer, f)
                    .map(|_| ())
                    .map_err(|e| match e {
                        IndexError::Storage(se) => se,
                        other => crate::storage::StorageError::BatchItem(other.to_string()),
                    })
            })
            .expect("seed via write_in_transaction");

        let changed =
            detect_changed_files(&storage, &[file.clone()]).expect("detect_changed_files");
        assert!(
            changed.is_empty(),
            "an unchanged file must not be reported as changed (no-write skip path), got {changed:?}"
        );
    }
}
