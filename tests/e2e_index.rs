//! End-to-end tests for the public `init → index` library surface — slice **M5.4**.
//!
//! TDD RED: written before the `init`/`index` facade exists. Scenarios from
//! `docs/plans/M5-indexer.md` (slice M5.4) + `docs/TEST_STRATEGY.md#indexer` +
//! `.claude/briefs/BRIEF-M5-indexer.md`.
//!
//! These tests drive CodeCache through the **public library entry points only** — no reaching
//! into private internals, and no CLI (that is deferred to M7). The pinned surface under test:
//!
//! ```ignore
//! // src/app.rs, re-exported at the crate root as `codecache::{init, index, AppError}`:
//!
//! /// Initialize a project: create `<root>/.codecache/`, write a default `config.toml`
//! /// (from `Config::default()`), then create + `init_schema()` the SQLite DB at the config's
//! /// `db_path` resolved under `root` (default `<root>/.codecache/index.db`).
//! ///
//! /// Idempotent: a second call must NOT error and must NOT clobber an existing config or DB —
//! /// an existing `config.toml` is left untouched, and `init_schema()` is itself idempotent.
//! pub fn init(project_root: &Path) -> Result<(), AppError>;
//!
//! /// Load `<root>/.codecache/config.toml`, open `Storage` at the resolved `db_path`, build
//! /// `Indexer::new(config, storage, root)`, run `index_all`, return its `IndexStats`.
//! pub fn index(project_root: &Path) -> Result<IndexStats, AppError>;
//!
//! /// Top-level error wrapping the config/storage/index failures behind the facade.
//! pub enum AppError { Config(ConfigError), Storage(StorageError), Index(IndexError) }
//! ```
//!
//! **DB-path resolution (pinned):** `index`/`init` load the config and resolve
//! `config.storage.db_path` against `project_root` — `<root>/.codecache/index.db` for the default
//! config. The e2e re-opens that same DB via `Storage::new` (a public API) to assert symbols are
//! searchable, so the tests must agree with the impl on where the DB lives.
//!
//! Repos are built at runtime under a `tempfile::TempDir` (no committed fixture tree — preferred,
//! consistent with M5.1–M5.3). Every search-set assertion sorts before comparing (determinism).

use std::fs;
use std::path::{Path, PathBuf};

use codecache::storage::Storage;
use codecache::{index, init, AppError, IndexStats};
use tempfile::TempDir;

// ───────────────────────────── fixture helpers ─────────────────────────────

/// Create a temp project root for one test. Removed when the returned `TempDir` is dropped.
fn temp_repo() -> TempDir {
    tempfile::tempdir().expect("create temp repo dir")
}

/// Write `contents` to `root/rel`, creating parent directories as needed.
fn write_file(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create parent dirs");
    }
    fs::write(&path, contents).expect("write fixture file");
}

/// The default DB location `init`/`index` resolve `config.storage.db_path` to under `root`.
fn default_db_path(root: &Path) -> PathBuf {
    root.join(".codecache").join("index.db")
}

/// Re-open the resulting on-disk DB read-only via the public `Storage` API and collect the sorted,
/// deduped symbol names matching `query` — a stable observable for "is this symbol queryable?".
fn searchable_symbols(db_path: &Path, query: &str) -> Vec<String> {
    let storage = Storage::new(db_path).expect("re-open indexed db");
    let mut names: Vec<String> = storage
        .search(query, 100)
        .expect("search must succeed on a populated db")
        .into_iter()
        .map(|h| h.chunk.symbol_name)
        .collect();
    names.sort();
    names.dedup();
    names
}

/// Build a small, fully-controlled Python repo: three files, each with exactly one top-level
/// definition (one function file, one class file, one second function file). The M4 chunker proves
/// a single top-level function yields exactly one chunk; the class file (`Service` + 2 methods)
/// yields three chunks (class + `__init__` + `process`) — so the expected chunk total is pinned.
fn build_python_repo(root: &Path) {
    write_file(
        root,
        "auth.py",
        "def authenticate_user():\n    return True\n",
    );
    write_file(
        root,
        "service.py",
        "class Service:\n    def __init__(self):\n        self.ready = True\n\n    def process(self):\n        return 1\n",
    );
    write_file(root, "util.py", "def normalize_path():\n    return 0\n");
}

// ═══════════════════════ Slice M5.4 — e2e init → index ═══════════════════════

#[test]
fn e2e_init_then_index_populates_queryable_db() {
    // init(root) → assert `.codecache/`, config.toml, and the DB exist. Then index(root) → assert
    // the returned IndexStats counts are correct and a known symbol is searchable in the DB — all
    // through the public library surface.
    let repo = temp_repo();
    let root = repo.path();
    build_python_repo(root);

    // ── init ───────────────────────────────────────────────────────────────
    init(root).expect("init must succeed on a fresh project");

    let cc_dir = root.join(".codecache");
    let config_path = cc_dir.join("config.toml");
    let db_path = default_db_path(root);

    assert!(
        cc_dir.is_dir(),
        "init must create the .codecache/ directory"
    );
    assert!(
        config_path.is_file(),
        "init must write .codecache/config.toml"
    );
    assert!(
        db_path.is_file(),
        "init must create + init_schema the SQLite db at the resolved db_path"
    );

    // ── index ──────────────────────────────────────────────────────────────
    let stats: IndexStats = index(root).expect("index must succeed after init");

    // Three source files: one function, one class (3 chunks), one function ⇒ 3 files / 5 chunks.
    assert_eq!(
        stats.files_processed, 3,
        "all three Python source files must be processed"
    );
    assert_eq!(
        stats.chunks_indexed, 5,
        "1 (auth fn) + 3 (Service class + 2 methods) + 1 (util fn) = 5 chunks"
    );

    // The DB is queryable: known symbols are searchable through the public Storage::search API.
    assert_eq!(
        searchable_symbols(&db_path, "authenticate_user"),
        vec!["authenticate_user".to_string()],
        "the top-level function must be searchable in the indexed db"
    );
    assert!(
        searchable_symbols(&db_path, "process").contains(&"process".to_string()),
        "a class method must be searchable in the indexed db"
    );
}

#[test]
fn e2e_init_is_idempotent_or_safe() {
    // Calling init twice must not error and must not clobber an existing config or corrupt the DB.
    // Semantics pinned for the eng lead: a second init leaves an existing config.toml byte-for-byte
    // untouched and re-runs the idempotent init_schema (no error, no data loss).
    let repo = temp_repo();
    let root = repo.path();
    build_python_repo(root);

    init(root).expect("first init must succeed");

    let config_path = root.join(".codecache").join("config.toml");
    let config_after_first =
        fs::read_to_string(&config_path).expect("read config after first init");

    // Second init over an already-initialized project.
    init(root).expect("second init must NOT error (idempotent/safe re-init)");

    let config_after_second =
        fs::read_to_string(&config_path).expect("read config after second init");
    assert_eq!(
        config_after_first, config_after_second,
        "re-init must not clobber an existing config.toml"
    );

    // The project is still indexable after a double init (DB not corrupted).
    let stats = index(root).expect("index must still succeed after a double init");
    assert_eq!(
        stats.files_processed, 3,
        "double init leaves a healthy, indexable project"
    );
    assert_eq!(
        searchable_symbols(&default_db_path(root), "authenticate_user"),
        vec!["authenticate_user".to_string()],
        "symbols remain queryable after re-init + index"
    );
}

#[test]
fn e2e_reindex_after_modification_reflects_change() {
    // init → index → modify a file → index again → the change is reflected (ties M5.3 incremental
    // reconcile into the e2e through the public surface). The renamed symbol becomes searchable and
    // the old symbol disappears.
    let repo = temp_repo();
    let root = repo.path();
    build_python_repo(root);

    init(root).expect("init must succeed");
    index(root).expect("first index must succeed");

    let db_path = default_db_path(root);
    assert_eq!(
        searchable_symbols(&db_path, "normalize_path"),
        vec!["normalize_path".to_string()],
        "the original symbol is searchable after the first index"
    );

    // Modify one file: rename its single function.
    write_file(root, "util.py", "def canonicalize_path():\n    return 0\n");

    // Re-index through the public surface (index_all runs incremental + reconcile on a populated db).
    index(root).expect("second index must succeed");

    assert_eq!(
        searchable_symbols(&db_path, "canonicalize_path"),
        vec!["canonicalize_path".to_string()],
        "the modified file's new symbol must be searchable after re-index"
    );
    assert!(
        !searchable_symbols(&db_path, "normalize_path").contains(&"normalize_path".to_string()),
        "the modified file's old symbol must no longer be searchable after re-index"
    );
    // The untouched file's symbol survives the incremental re-index.
    assert_eq!(
        searchable_symbols(&db_path, "authenticate_user"),
        vec!["authenticate_user".to_string()],
        "an untouched file's symbol must remain searchable after an incremental re-index"
    );
}

/// `AppError` must surface as a typed, `Debug` error so callers (and these tests) can pattern-match
/// the facade's failure modes. A trivial type-level assertion that the error is reachable/public.
#[test]
fn app_error_is_public_and_debuggable() {
    fn assert_error<E: std::error::Error>() {}
    assert_error::<AppError>();
}
