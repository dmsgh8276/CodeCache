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

    /// Run a full index of `root` (`project_plan.md` §5.1): discover source files, then for each
    /// file hash/read/parse/chunk and store its chunks plus a `files_metadata` row, accumulating
    /// an [`IndexStats`]. After the batch, `index_state` `total_files`/`total_chunks` are stamped.
    ///
    /// **D2 isolation:** a single file that fails (unreadable, parse/store error) is counted as
    /// skipped and the batch continues — `index_all` returns `Ok` rather than aborting.
    ///
    /// # Errors
    /// Returns [`IndexError`] only for failures that are not isolatable to a single file: discovery
    /// (`.gitignore`/glob/walk) errors, or the storage write of the `index_state` totals.
    pub fn index_all(&mut self) -> Result<IndexStats, IndexError> {
        let start = std::time::Instant::now();

        let files = discover_files(&self.config, &self.root)?;

        let mut stats = IndexStats::default();
        for file in files {
            // D2: per-file work is isolated — a failure is logged-as-skipped, not propagated.
            match pipeline::index_file(&mut self.parser, &self.storage, &file) {
                Ok(chunk_count) => {
                    stats.files_processed += 1;
                    stats.chunks_indexed += chunk_count;
                }
                Err(_skipped) => {
                    // Degrade-and-continue: the malformed/unreadable file is dropped from the run.
                }
            }
        }

        self.storage
            .set_index_state("total_files", &stats.files_processed.to_string())
            .map_err(IndexError::Storage)?;
        self.storage
            .set_index_state("total_chunks", &stats.chunks_indexed.to_string())
            .map_err(IndexError::Storage)?;

        stats.duration_ms = start.elapsed().as_millis() as u64;
        Ok(stats)
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
