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

use std::path::{Path, PathBuf};

use crate::config::{Config, ConfigError};
use crate::indexer::{IndexError, IndexStats, Indexer};
use crate::storage::{Storage, StorageError};

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
            AppError::Io { source, .. } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {}
