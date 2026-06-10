//! Per-file indexing pipeline (slice M5.2).
//!
//! API anchor: `project_plan.md` §5.1 (step 3a–3e). Owner: `principal-engineering-lead`.
//! Scenarios: `docs/TEST_STRATEGY.md#indexer` (full-index rows + D2).
//!
//! [`index_file`] performs the per-file work of a full index for one discovered source file:
//! compute hash → read content → detect language → parse → chunk → `insert_chunks` → write the
//! `files_metadata` row. It returns the number of chunks inserted so the caller can accumulate
//! [`IndexStats`](super::IndexStats). Each fallible step surfaces a typed [`IndexError`] via `?`;
//! the caller (`index_all`) wraps the whole call so a single file's error is counted/skipped and
//! the batch continues (D2 degrade-and-continue) — there is no reachable `unwrap`/`expect`/`panic`.

use std::path::Path;

use crate::chunker;
use crate::hasher;
use crate::parser::Parser;
use crate::storage::Storage;
use crate::types::{FileMeta, Language};

use super::{detect_language, IndexError};

/// Index one source file: hash, read, parse, chunk, store its chunks, and upsert its
/// `files_metadata` row (`project_plan.md` §5.1 step 3a–3e). Returns the count of chunks inserted.
///
/// The chunker handles a malformed tree gracefully (heuristic fallback or empty), so a syntactically
/// broken file does not error here; it is the unreadable/unsupported-language/storage failures that
/// surface an [`IndexError`] for the caller to isolate (D2).
///
/// # Errors
/// Returns [`IndexError::Hash`] if hashing fails, [`IndexError::File`] if the content/metadata
/// cannot be read, [`IndexError::Parser`]/[`IndexError::Chunker`] on parse/chunk failure, and
/// [`IndexError::Storage`] if the chunk insert or metadata upsert fails.
pub fn index_file(
    parser: &mut Parser,
    storage: &Storage,
    path: &Path,
) -> Result<usize, IndexError> {
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

    // §5.1 step 3d: store chunks (single transaction inside insert_chunks).
    storage
        .insert_chunks(&chunks)
        .map_err(IndexError::Storage)?;

    // §5.1 step 3e: upsert the file's metadata row (D6 bundle).
    let meta = FileMeta {
        content_hash,
        mtime,
        file_size,
        language,
        chunk_count,
    };
    storage
        .update_file_hash(path, &meta)
        .map_err(IndexError::Storage)?;

    Ok(chunk_count)
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
