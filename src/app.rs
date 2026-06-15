//! Application facade: the thin public `init` → `index` library surface (slice **M5.4**).
//!
//! API anchor: `.claude/briefs/BRIEF-M5-indexer.md` (M5.4). Owner: `principal-engineering-lead`.
//! Scenarios: `tests/e2e_index.rs` + `docs/TEST_STRATEGY.md#indexer`.
//!
//! This module is **pure glue** over the already-implemented leaf/orchestration modules
//! (`config`, `storage`, `indexer`). It exposes two free functions re-exported at the crate root:
//!
//! - [`init`] — create `<root>/.codecache/`, write a default `config.toml` **if absent**
//!   (non-clobbering), and create + `init_schema()` the SQLite DB at the resolved `db_path`.
//! - [`index`] — load the project's `config.toml`, open `Storage`, build an [`Indexer`], run
//!   `index_all`, and return its [`IndexStats`].
//!
//! All failures surface through the typed [`AppError`] (no reachable `unwrap`/`expect`/`panic!`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::config::{Config, ConfigError};
use crate::indexer::{IndexError, IndexStats, Indexer};
use crate::storage::{Storage, StorageError};
use crate::types::{Chunk, FileMeta, Language, SymbolType};

/// Directory name CodeCache stores its config + database under, inside a project root.
const CODECACHE_DIR: &str = ".codecache";
/// Config file name written inside [`CODECACHE_DIR`].
const CONFIG_FILE: &str = "config.toml";

/// Initialize a project for indexing.
///
/// Creates `<project_root>/.codecache/`, writes a default `config.toml` from [`Config::default`]
/// **only if one does not already exist** (so a second call never clobbers a user's config), and
/// creates + `init_schema()`s the SQLite database at the config's `db_path` resolved under
/// `project_root` (default `<project_root>/.codecache/index.db`).
///
/// Idempotent: calling `init` again on an already-initialized project does not error and does not
/// rewrite an existing config — `init_schema` is itself idempotent (`CREATE ... IF NOT EXISTS`).
///
/// # Errors
/// Returns [`AppError::Config`] if the existing config cannot be read/parsed, and
/// [`AppError::Storage`] if the database cannot be created or its schema initialized. Filesystem
/// failures while writing the config or creating directories surface as [`AppError::Io`].
pub fn init(project_root: &Path) -> Result<(), AppError> {
    let cc_dir = project_root.join(CODECACHE_DIR);
    create_dir(&cc_dir)?;

    // Write the default config only if absent — re-init must not clobber an existing config.
    let config_path = cc_dir.join(CONFIG_FILE);
    if !config_path.exists() {
        let toml = toml::to_string(&Config::default()).map_err(AppError::serialize_config)?;
        std::fs::write(&config_path, toml)
            .map_err(|source| AppError::write_config(&config_path, source))?;
    }

    // Resolve the DB path from the config that now exists on disk and create + init its schema.
    let config = Config::load(&config_path).map_err(AppError::Config)?;
    let db_path = resolve_db_path(project_root, &config);
    open_storage(&db_path)?
        .init_schema()
        .map_err(AppError::Storage)?;

    Ok(())
}

/// Index a previously-initialized project.
///
/// Loads `<project_root>/.codecache/config.toml`, opens `Storage` at the resolved `db_path`, builds
/// an [`Indexer`] rooted at `project_root`, runs `index_all` (full on a fresh DB, incremental +
/// reconcile on a populated one), and returns its [`IndexStats`].
///
/// # Errors
/// Returns [`AppError::Config`] if the config cannot be loaded, [`AppError::Storage`] if the DB
/// cannot be opened, and [`AppError::Index`] if the indexing run fails.
pub fn index(project_root: &Path) -> Result<IndexStats, AppError> {
    let config_path = project_root.join(CODECACHE_DIR).join(CONFIG_FILE);
    let config = Config::load(&config_path).map_err(AppError::Config)?;

    let db_path = resolve_db_path(project_root, &config);
    let storage = open_storage(&db_path)?;

    let mut indexer =
        Indexer::new(config, storage, project_root.to_path_buf()).map_err(AppError::Index)?;
    let stats = indexer.index_all().map_err(AppError::Index)?;
    Ok(stats)
}

/// Sentinel `content_hash` for an ingested `files_metadata` row. Ingestion bypasses file hashing
/// (there is no source file on disk to hash), and the research workflow `init`s a fresh DB per arm
/// and never re-hashes — so a format-valid 32-hex sentinel stands in for the absent content hash.
/// (Do not run `index` over an ingested DB: reconciliation would delete the ingested rows, since no
/// matching files exist on disk. Ingestion targets a fresh DB — R2.3a brief, non-goal.)
const INGESTED_FILE_HASH: &str = "00000000000000000000000000000000";

/// Stats from an [`ingest_chunks`] run: distinct `file_path`s and total chunk records inserted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IngestStats {
    /// Number of distinct `file_path` values ingested (one `files_metadata` row each).
    pub files_ingested: usize,
    /// Total chunk records inserted.
    pub chunks_ingested: usize,
}

/// Format-local input DTO for one ingested chunk record (R2.3a / **D25**). Mirrors `types::Chunk`
/// but lives here so `serde` stays OFF `types::Chunk` (transport separation, D4/D5). The enum fields
/// are `String` and validated via `from_str_lenient` when mapped to a [`Chunk`]; the optional
/// enrichment/degradation fields default. Unknown JSON keys are ignored (lenient — a research seam).
#[derive(Debug, Deserialize)]
struct IngestChunk {
    symbol_name: String,
    symbol_type: String,
    file_path: String,
    start_byte: usize,
    end_byte: usize,
    start_line: usize,
    end_line: usize,
    chunk_text: String,
    language: String,
    #[serde(default)]
    parent_symbol: Option<String>,
    #[serde(default)]
    file_docstring: Option<String>,
    #[serde(default)]
    imports: Vec<String>,
    #[serde(default)]
    cross_references: Vec<String>,
    #[serde(default)]
    is_heuristic: bool,
}

impl IngestChunk {
    /// Map the DTO into a [`Chunk`], validating the enum strings (unknown ⇒ typed [`IngestError`]).
    fn into_chunk(self) -> Result<Chunk, IngestError> {
        let IngestChunk {
            symbol_name,
            symbol_type,
            file_path,
            start_byte,
            end_byte,
            start_line,
            end_line,
            chunk_text,
            language,
            parent_symbol,
            file_docstring,
            imports,
            cross_references,
            is_heuristic,
        } = self;
        let symbol_type = match SymbolType::from_str_lenient(&symbol_type) {
            Some(st) => st,
            None => return Err(IngestError::UnknownSymbolType(symbol_type)),
        };
        let language = match Language::from_str_lenient(&language) {
            Some(lang) => lang,
            None => return Err(IngestError::UnknownLanguage(language)),
        };
        Ok(Chunk {
            symbol_name,
            symbol_type,
            file_path: PathBuf::from(file_path),
            start_byte,
            end_byte,
            start_line,
            end_line,
            chunk_text,
            language,
            parent_symbol,
            file_docstring,
            imports,
            cross_references,
            is_heuristic,
        })
    }
}

/// Ingest caller-supplied chunks from a JSON file into a previously-initialized project (R2.3a /
/// **D25** — the research chunker-ablation seam, driven by the hidden `codecache ingest` command).
///
/// Reads `chunks_path` (a JSON array of chunk records — `project_plan.md` §3.2.4 / §7.2),
/// deserializes + validates it, and inserts the chunks **straight into storage in array order**
/// (bypassing discover→parse→chunk) so any external chunker's output flows through CodeCache's same
/// storage + FTS5-BM25 + retriever. Writes one `files_metadata` row per distinct `file_path` and
/// restamps the `index_state` totals, so `status` / `codecache_outline` read the ingested rows. An
/// empty array `[]` is a clean no-op (`Ok` with zero stats), not an error.
///
/// # Errors
/// [`AppError::Ingest`] for an unreadable/malformed/invalid chunks file (missing required field,
/// unknown `symbol_type`/`language`, wrong JSON type); [`AppError::Config`] if the project config
/// cannot be loaded; [`AppError::Storage`] if a database write fails. No reachable panic.
pub fn ingest_chunks(project_root: &Path, chunks_path: &Path) -> Result<IngestStats, AppError> {
    let raw = std::fs::read_to_string(chunks_path).map_err(|source| {
        AppError::Ingest(IngestError::Read {
            path: chunks_path.to_path_buf(),
            source,
        })
    })?;
    let dtos: Vec<IngestChunk> =
        serde_json::from_str(&raw).map_err(|e| AppError::Ingest(IngestError::Parse(e)))?;

    // Map DTO → Chunk preserving array order (array order = insertion = rowid order).
    let mut chunks: Vec<Chunk> = Vec::with_capacity(dtos.len());
    for dto in dtos {
        chunks.push(dto.into_chunk().map_err(AppError::Ingest)?);
    }

    let config_path = project_root.join(CODECACHE_DIR).join(CONFIG_FILE);
    let config = Config::load(&config_path).map_err(AppError::Config)?;
    let db_path = resolve_db_path(project_root, &config);
    let storage = open_storage(&db_path)?;

    storage.insert_chunks(&chunks).map_err(AppError::Storage)?;

    // One `files_metadata` row per distinct `file_path` (first-seen order), carrying that file's
    // language + ingested chunk count. There is no real file to hash → sentinel `content_hash`.
    let mut order: Vec<PathBuf> = Vec::new();
    let mut per_file: HashMap<PathBuf, (Language, usize)> = HashMap::new();
    for c in &chunks {
        match per_file.get_mut(&c.file_path) {
            Some((_, count)) => *count += 1,
            None => {
                order.push(c.file_path.clone());
                per_file.insert(c.file_path.clone(), (c.language, 1));
            }
        }
    }
    for path in &order {
        if let Some(&(language, chunk_count)) = per_file.get(path) {
            let meta = FileMeta {
                content_hash: INGESTED_FILE_HASH.to_string(),
                mtime: 0,
                file_size: 0,
                language,
                chunk_count,
            };
            storage
                .update_file_hash(path, &meta)
                .map_err(AppError::Storage)?;
        }
    }

    let files_ingested = order.len();
    let chunks_ingested = chunks.len();
    storage
        .set_index_state("total_files", &files_ingested.to_string())
        .map_err(AppError::Storage)?;
    storage
        .set_index_state("total_chunks", &chunks_ingested.to_string())
        .map_err(AppError::Storage)?;

    Ok(IngestStats {
        files_ingested,
        chunks_ingested,
    })
}

/// A failure reading, parsing, or validating an ingest chunks file (R2.3a / **D25**). Wrapped by
/// [`AppError::Ingest`]; implements [`std::error::Error`] with a `source()` chain, never panics.
#[derive(Debug)]
pub enum IngestError {
    /// The chunks JSON file could not be read.
    Read {
        /// The path that could not be read.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
    /// The file was not valid JSON for an array of chunk records (malformed, a missing required
    /// field, or a field of the wrong JSON type).
    Parse(serde_json::Error),
    /// A record's `symbol_type` was not one of `function`/`class`/`method`/`struct`.
    UnknownSymbolType(String),
    /// A record's `language` was not one of `python`/`typescript`/`go`.
    UnknownLanguage(String),
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IngestError::Read { path, source } => {
                write!(
                    f,
                    "could not read chunks file '{}': {source}",
                    path.display()
                )
            }
            IngestError::Parse(e) => write!(f, "invalid chunks JSON: {e}"),
            IngestError::UnknownSymbolType(s) => {
                write!(
                    f,
                    "unknown symbol_type '{s}' (expected function|class|method|struct)"
                )
            }
            IngestError::UnknownLanguage(s) => {
                write!(f, "unknown language '{s}' (expected python|typescript|go)")
            }
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            IngestError::Read { source, .. } => Some(source),
            IngestError::Parse(e) => Some(e),
            IngestError::UnknownSymbolType(_) | IngestError::UnknownLanguage(_) => None,
        }
    }
}

/// Resolve the SQLite database path: `config.storage.db_path` joined under `project_root`. For the
/// default config this is `<project_root>/.codecache/index.db`; an absolute `db_path` is returned
/// unchanged by `Path::join`.
fn resolve_db_path(project_root: &Path, config: &Config) -> PathBuf {
    project_root.join(&config.storage.db_path)
}

/// Open `Storage` at `db_path`, defensively ensuring its parent directory exists first
/// (`Connection::open` does not create missing parent directories).
fn open_storage(db_path: &Path) -> Result<Storage, AppError> {
    if let Some(parent) = db_path.parent() {
        create_dir(parent)?;
    }
    Storage::new(db_path).map_err(AppError::Storage)
}

/// `create_dir_all`, mapping a filesystem failure into [`AppError::Io`] (the IO-flavored variant
/// on the facade; the path context is preserved in the error message).
fn create_dir(path: &Path) -> Result<(), AppError> {
    std::fs::create_dir_all(path).map_err(|source| AppError::create_dir(path, source))
}

/// Top-level error for the public `init`/`index` facade. Wraps the config, storage, and indexer
/// failures behind one type so callers (CLI in M7, the MCP server in M8, the e2e tests) match a
/// single error surface. Implements [`std::error::Error`] with a `source()` chain; never panics.
#[derive(Debug)]
pub enum AppError {
    /// Loading, serializing, or writing the project config failed.
    Config(ConfigError),
    /// Opening or initializing the SQLite database failed.
    Storage(StorageError),
    /// The indexing run failed.
    Index(IndexError),
    /// Reading, parsing, or validating an ingest chunks file failed (R2.3a / D25).
    Ingest(IngestError),
    /// A filesystem operation (create dir, write config) failed before a typed sub-error applied.
    Io {
        /// The path the operation targeted.
        path: PathBuf,
        /// The underlying I/O error.
        source: std::io::Error,
    },
}

impl AppError {
    fn create_dir(path: &Path, source: std::io::Error) -> AppError {
        AppError::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    fn write_config(path: &Path, source: std::io::Error) -> AppError {
        AppError::Io {
            path: path.to_path_buf(),
            source,
        }
    }

    fn serialize_config(source: toml::ser::Error) -> AppError {
        AppError::Io {
            path: PathBuf::from(CONFIG_FILE),
            source: std::io::Error::other(source),
        }
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppError::Config(e) => write!(f, "config error: {e}"),
            AppError::Storage(e) => write!(f, "storage error: {e}"),
            AppError::Index(e) => write!(f, "indexing error: {e}"),
            AppError::Ingest(e) => write!(f, "ingest error: {e}"),
            AppError::Io { path, source } => {
                write!(f, "filesystem error at '{}': {source}", path.display())
            }
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::Config(e) => Some(e),
            AppError::Storage(e) => Some(e),
            AppError::Index(e) => Some(e),
            AppError::Ingest(e) => Some(e),
            AppError::Io { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {}
