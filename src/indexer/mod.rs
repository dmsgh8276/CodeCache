//! Indexer: orchestrate file discovery → parse → chunk → hash → store; incremental updates.
//!
//! API anchor: `project_plan.md` §3.2.4 / §5.1 / §5.2. Owner: `principal-engineering-lead`.
//! Scenarios: `docs/TEST_STRATEGY.md#indexer`. M0: empty stub; implemented at M5.
//!
//! Slice **M5.1** ships file discovery + language detection (see [`discovery`]); slice **M5.2**
//! adds the [`Indexer`] facade and its full-index entry point [`Indexer::index_all`] (see
//! [`pipeline`]). Incremental updates (`update_files`) land in M5.3+.

mod discovery;
mod pipeline;

use std::path::PathBuf;

pub use discovery::{detect_language, discover_files};

use crate::config::Config;
use crate::parser::Parser;
use crate::storage::Storage;

/// Aggregate result of an indexing run (`project_plan.md` §3.2.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct IndexStats {
    /// Number of source files successfully processed in this run.
    pub files_processed: usize,
    /// Total chunks inserted into storage across all processed files.
    pub chunks_indexed: usize,
    /// Wall-clock duration of the run, in milliseconds.
    pub duration_ms: u64,
}

/// Orchestrates the indexing pipeline: discover → hash → parse → chunk → store (`project_plan.md`
/// §3.2.4 / §5.1). Holds a reusable [`Parser`] and a cheaply-clonable [`Storage`] handle; `root`
/// is the directory discovery walks (`config.index_paths` resolved against it, defaulting to
/// `root` itself when empty).
pub struct Indexer {
    config: Config,
    storage: Storage,
    root: PathBuf,
    parser: Parser,
}

impl Indexer {
    /// Construct an indexer over `config` and `storage`, rooted at `root`.
    ///
    /// # Errors
    /// Returns [`IndexError::Parser`] if the Tree-sitter [`Parser`] cannot be constructed (e.g. an
    /// embedded query fails to compile).
    pub fn new(config: Config, storage: Storage, root: PathBuf) -> Result<Indexer, IndexError> {
        let parser = Parser::new().map_err(IndexError::Parser)?;
        Ok(Indexer {
            config,
            storage,
            root,
            parser,
        })
    }

    /// Index `root` incrementally (`project_plan.md` §5.1 / §5.2). On a fresh database this is a
    /// full index; on a populated one it runs as **incremental + reconcile**:
    /// 1. discover source files (honoring `.gitignore` + config patterns + language filter);
    /// 2. skip files whose on-disk content hash equals the stored hash — **no writes** for them
    ///    (the idempotency guarantee: an unchanged file is neither deleted, re-inserted, nor
    ///    re-stamped);
    /// 3. re-index changed files (delete their old chunks first, then re-insert) and index new
    ///    files;
    /// 4. reconcile deletions: every path in `files_metadata` no longer present on disk has its
    ///    chunks deleted and its metadata row removed;
    /// 5. re-stamp `index_state` `total_files`/`total_chunks` to the reconciled, DB-wide totals.
    ///
    /// `IndexStats.files_processed`/`chunks_indexed` count only the files actually (re-)indexed in
    /// this run — an unchanged repo yields `files_processed == 0`.
    ///
    /// **D2 isolation:** a single file that fails (unreadable, parse/store error) is counted as
    /// skipped and the batch continues — `index_all` returns `Ok` rather than aborting.
    ///
    /// # Errors
    /// Returns [`IndexError`] only for failures that are not isolatable to a single file: discovery
    /// (`.gitignore`/glob/walk) errors, reconcile/storage reads, or the `index_state` totals write.
    pub fn index_all(&mut self) -> Result<IndexStats, IndexError> {
        let start = std::time::Instant::now();

        let discovered = discover_files(&self.config, &self.root)?;

        // (2)+(3): re-index only the changed/new files; unchanged files take the no-write skip path.
        let changed = pipeline::detect_changed_files(&self.storage, &discovered)?;
        let mut stats = self.reindex_each(&changed)?;

        // (4): reconcile deletions — drop chunks + metadata for files no longer on disk.
        let on_disk: std::collections::HashSet<PathBuf> = discovered.into_iter().collect();
        for stored in self
            .storage
            .all_indexed_files()
            .map_err(IndexError::Storage)?
        {
            if !on_disk.contains(&stored) {
                self.storage
                    .delete_chunks_for_file(&stored)
                    .map_err(IndexError::Storage)?;
                self.storage
                    .delete_file_meta(&stored)
                    .map_err(IndexError::Storage)?;
            }
        }

        // (5): re-stamp totals to the post-reconcile, DB-wide counts (not just this run's delta).
        self.restamp_index_state()?;

        stats.duration_ms = start.elapsed().as_millis() as u64;
        Ok(stats)
    }

    /// Incrementally re-index an explicit list of files (`project_plan.md` §5.2). For each path,
    /// compare its on-disk content hash against the stored hash; **skip** unchanged files (no
    /// delete/insert, not counted), and for changed/new files delete their old chunks then
    /// re-parse/re-chunk/insert and upsert the metadata row. `IndexStats.files_processed` counts the
    /// files actually re-indexed; `index_state` totals are re-stamped to the DB-wide counts so an
    /// incremental update does not leave the totals drifted.
    ///
    /// **D2 isolation:** a single failing file is counted-as-skipped; the batch continues.
    ///
    /// # Errors
    /// Returns [`IndexError`] only for non-isolatable failures: storage reads during change
    /// detection, or the `index_state` totals write.
    pub fn update_files(&mut self, files: &[PathBuf]) -> Result<IndexStats, IndexError> {
        let start = std::time::Instant::now();

        let changed = pipeline::detect_changed_files(&self.storage, files)?;
        let mut stats = self.reindex_each(&changed)?;

        self.restamp_index_state()?;

        stats.duration_ms = start.elapsed().as_millis() as u64;
        Ok(stats)
    }

    /// Re-index each file in `files` (delete-first to avoid duplicates) through ONE outer
    /// transaction with a SAVEPOINT per file (Decision Log **D20**), accumulating an [`IndexStats`]
    /// of files/chunks actually written. `files` holds only changed/new files (the caller runs
    /// `detect_changed_files` first), so an unchanged file never opens a savepoint or re-stamps its
    /// metadata — preserving the no-write idempotency guarantee inside the batch.
    ///
    /// **D2 isolation under batching:** each file's per-file work runs inside its own savepoint; a
    /// failing file (read/parse/chunk/store) rolls back ONLY its savepoint and is counted-as-skipped,
    /// while the committed siblings survive the single outer commit. `duration_ms` is left zero here
    /// — the public entry point stamps wall-clock time.
    ///
    /// # Errors
    /// Returns [`IndexError`] only for a non-isolatable storage failure (the outer begin/commit, a
    /// savepoint begin/release/rollback, or a poisoned lock); a single file's error is isolated.
    fn reindex_each(&mut self, files: &[PathBuf]) -> Result<IndexStats, IndexError> {
        // Per-file chunk counts captured out-of-band: `write_in_transaction` hands back one
        // `Result<()>` per file (which committed, which rolled back), and `chunk_counts[i]` is the
        // count `reindex_file_batched` inserted for file i — summed only for the committed files.
        // The closure runs once per file in order, so a running cursor keys each count by position.
        //
        // The closure's error type is the storage layer's `StorageError` (the primitive's contract),
        // but the per-file pipeline fails with `IndexError`. We map any per-file `IndexError` into a
        // `StorageError` purely as the savepoint rollback signal — its value is discarded (D2 just
        // counts the file as skipped), so no detail is lost that the indexer surfaces.
        let parser = &mut self.parser;
        let mut chunk_counts = vec![0usize; files.len()];
        let mut cursor = 0usize;
        let per_item = self
            .storage
            .write_in_transaction(files, |writer, file| {
                // Bind this call's slot up front (one `each` call per file, in order) so a per-file
                // `Err` does not misalign later files' counts; a failed file simply keeps its 0.
                let slot = cursor;
                cursor += 1;
                match pipeline::reindex_file_batched(parser, writer, file) {
                    Ok(count) => {
                        chunk_counts[slot] = count;
                        Ok(())
                    }
                    Err(e) => Err(index_error_as_storage_signal(e)),
                }
            })
            .map_err(IndexError::Storage)?;

        let mut stats = IndexStats::default();
        for (i, item) in per_item.iter().enumerate() {
            if item.is_ok() {
                stats.files_processed += 1;
                stats.chunks_indexed += chunk_counts[i];
            }
            // D2 degrade-and-continue: a failed file's savepoint was rolled back; it is not counted.
        }
        Ok(stats)
    }

    /// Re-stamp `index_state` `total_files`/`total_chunks` to the current DB-wide totals
    /// (`files_metadata` row count and summed `chunk_count`), so the counters stay consistent after
    /// incremental updates and deletion reconciliation rather than reflecting only one run's delta.
    fn restamp_index_state(&self) -> Result<(), IndexError> {
        let mut total_files = 0usize;
        let mut total_chunks = 0usize;
        for path in self
            .storage
            .all_indexed_files()
            .map_err(IndexError::Storage)?
        {
            if let Some(meta) = self
                .storage
                .get_file_meta(&path)
                .map_err(IndexError::Storage)?
            {
                total_files += 1;
                total_chunks += meta.chunk_count;
            }
        }
        self.storage
            .set_index_state("total_files", &total_files.to_string())
            .map_err(IndexError::Storage)?;
        self.storage
            .set_index_state("total_chunks", &total_chunks.to_string())
            .map_err(IndexError::Storage)?;
        Ok(())
    }
}

/// Map a per-file [`IndexError`] into a [`StorageError`](crate::storage::StorageError) to use as the
/// savepoint-rollback signal inside [`Storage::write_in_transaction`](crate::storage::Storage::write_in_transaction)
/// (Decision Log **D20**). The primitive's closure error type is the storage layer's, but the
/// per-file pipeline fails with `IndexError`; an `IndexError::Storage` carries the real storage
/// error through unchanged, and the other per-file (read/parse/chunk/hash) failures — which must
/// still roll the file's savepoint back — are wrapped so the file is counted-as-skipped (D2). The
/// returned value is never surfaced by the indexer (the old `reindex_each` discarded the error too).
fn index_error_as_storage_signal(err: IndexError) -> crate::storage::StorageError {
    match err {
        IndexError::Storage(e) => e,
        other => crate::storage::StorageError::BatchItem(other.to_string()),
    }
}

/// A typed indexer error. Wraps the failures that can occur while discovering and indexing files
/// (filesystem walk errors, invalid ignore-pattern globs, and the per-file parse/chunk/store
/// failures isolated by D2). Never panics; carries enough context to report what went wrong.
#[derive(Debug)]
pub enum IndexError {
    /// A filesystem walk entry under `path` could not be read (missing, unreadable, permissions, …).
    Io {
        /// The walk root whose traversal failed.
        path: std::path::PathBuf,
        /// The underlying walk error.
        source: ignore::Error,
    },
    /// A `config.ignore_patterns` entry is not a valid gitignore-style glob.
    Glob {
        /// The offending pattern (or the joined pattern set when the failure is build-wide).
        pattern: String,
        /// The underlying glob-compilation error.
        source: ignore::Error,
    },
    /// A file could not be read or its metadata obtained (per-file; isolated by D2).
    File {
        /// The file whose read/stat failed.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// Content hashing of a file failed (per-file; isolated by D2).
    Hash(crate::hasher::HasherError),
    /// The Tree-sitter parser could not be built or a file could not be parsed.
    Parser(crate::parser::ParserError),
    /// Chunk extraction from a parsed tree failed (per-file; isolated by D2).
    Chunker(crate::chunker::ChunkerError),
    /// A storage write/read failed (e.g. inserting chunks, the `index_state` totals).
    Storage(crate::storage::StorageError),
}

impl std::fmt::Display for IndexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IndexError::Io { path, source } => {
                write!(f, "failed to walk '{}': {source}", path.display())
            }
            IndexError::Glob { pattern, source } => {
                write!(f, "invalid ignore pattern '{pattern}': {source}")
            }
            IndexError::File { path, source } => {
                write!(f, "failed to read '{}': {source}", path.display())
            }
            IndexError::Hash(e) => write!(f, "failed to hash file: {e}"),
            IndexError::Parser(e) => write!(f, "failed to parse file: {e}"),
            IndexError::Chunker(e) => write!(f, "failed to chunk file: {e}"),
            IndexError::Storage(e) => write!(f, "storage error during indexing: {e}"),
        }
    }
}

impl std::error::Error for IndexError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IndexError::Io { source, .. } => Some(source),
            IndexError::Glob { source, .. } => Some(source),
            IndexError::File { source, .. } => Some(source),
            IndexError::Hash(e) => Some(e),
            IndexError::Parser(e) => Some(e),
            IndexError::Chunker(e) => Some(e),
            IndexError::Storage(e) => Some(e),
        }
    }
}

#[cfg(test)]
mod tests {}
