//! Integration tests for the `indexer` module — slice M5.1 (discovery + language detection).
//!
//! TDD RED: written before `src/indexer/discovery.rs` exists. Scenarios from
//! `docs/plans/M5-indexer.md` (slice M5.1) + `docs/TEST_STRATEGY.md#indexer` +
//! `.claude/briefs/BRIEF-M5-indexer.md`.
//!
//! The public API under test (free functions in the `indexer` module — the "discovery.rs" split
//! per the plan; promoted to `pub` so integration tests can reach the seam):
//! ```ignore
//! pub fn detect_language(path: &Path) -> Option<Language>;
//! pub fn discover_files(config: &Config, root: &Path) -> Result<Vec<PathBuf>, IndexError>;
//! ```
//! `detect_language` maps a path's extension to a [`Language`] (`.py`→Python, `.ts`→TypeScript,
//! `.go`→Go), returning `None` for non-source extensions. `discover_files` walks `config
//! .index_paths` resolved against `root` (defaulting to `root` itself when `index_paths` is
//! empty), honors `.gitignore`, applies `config.ignore_patterns`, and restricts results to
//! `config.languages`.
//!
//! Discovery order is filesystem-dependent, so every assertion sorts results first (determinism).
//! Repos are built at runtime under a `tempfile::TempDir` (no committed fixture tree needed —
//! `.gitignore` is created in-test), keeping the repo clean and tests parallel-safe.

use std::fs;
use std::path::Path;

use codecache::config::Config;
use codecache::indexer::{detect_language, discover_files, IndexStats, Indexer};
use codecache::storage::Storage;
use codecache::types::Language;
use tempfile::TempDir;

// ───────────────────────────── fixture helpers ─────────────────────────────

/// Create a temp repo root for one test. The directory (and everything under it) is removed when
/// the returned `TempDir` is dropped.
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

/// A `Config` whose `index_paths` is empty (⇒ discovery defaults to walking `root` itself) and
/// whose `ignore_patterns` is empty, with the given language set.
fn config_with_languages(languages: Vec<Language>) -> Config {
    Config {
        languages,
        ..Config::default()
    }
}

/// The set of file names (last path component) discovered, sorted for deterministic comparison.
fn discovered_file_names(config: &Config, root: &Path) -> Vec<String> {
    let mut names: Vec<String> = discover_files(config, root)
        .expect("discover_files must succeed on a readable repo")
        .into_iter()
        .map(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    names.sort();
    names
}

/// Sorted, root-relative path strings (forward-slash normalized) for path-sensitive assertions.
fn discovered_rel_paths(config: &Config, root: &Path) -> Vec<String> {
    let mut rels: Vec<String> = discover_files(config, root)
        .expect("discover_files must succeed on a readable repo")
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(root).unwrap_or(&p);
            rel.to_string_lossy().replace('\\', "/")
        })
        .collect();
    rels.sort();
    rels
}

// ═══════════════ Slice M5.1 — discovery + language detection ═══════════════

#[test]
fn language_detected_from_extension() {
    // `.py`→Python, `.ts`→TypeScript, `.go`→Go; a non-source extension (and no extension) → None.
    assert_eq!(
        detect_language(Path::new("foo/bar.py")),
        Some(Language::Python),
        ".py must detect as Python"
    );
    assert_eq!(
        detect_language(Path::new("foo/bar.ts")),
        Some(Language::TypeScript),
        ".ts must detect as TypeScript"
    );
    assert_eq!(
        detect_language(Path::new("foo/bar.go")),
        Some(Language::Go),
        ".go must detect as Go"
    );
    assert_eq!(
        detect_language(Path::new("README.md")),
        None,
        "a non-source extension must detect as None"
    );
    assert_eq!(
        detect_language(Path::new("Makefile")),
        None,
        "an extension-less file must detect as None"
    );
}

#[test]
fn discovery_only_returns_configured_languages() {
    // languages = [Python]: a repo with a.py, b.ts, c.go returns only a.py.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "a.py", "def a():\n    return 1\n");
    write_file(root, "b.ts", "export const b = () => 1;\n");
    write_file(root, "c.go", "package main\nfunc c() int { return 1 }\n");

    let config = config_with_languages(vec![Language::Python]);

    assert_eq!(
        discovered_file_names(&config, root),
        vec!["a.py".to_string()],
        "with languages=[Python], only the .py file is discovered (.ts/.go skipped)"
    );
}

#[test]
fn discovery_respects_gitignore() {
    // A file matched by a `.gitignore` entry must not be returned.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "kept.py", "def kept():\n    return 1\n");
    write_file(root, "ignored.py", "def ignored():\n    return 1\n");
    write_file(root, ".gitignore", "ignored.py\n");

    let config = config_with_languages(vec![Language::Python]);

    assert_eq!(
        discovered_file_names(&config, root),
        vec!["kept.py".to_string()],
        "a .gitignore'd file must be excluded from discovery"
    );
}

#[test]
fn discovery_respects_extra_ignore_patterns_from_config() {
    // config.ignore_patterns excludes matching files in addition to .gitignore.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "keep.py", "def keep():\n    return 1\n");
    write_file(root, "schema_generated.py", "GENERATED = True\n");
    write_file(root, "vendor/dep.py", "def dep():\n    return 1\n");

    let config = Config {
        languages: vec![Language::Python],
        ignore_patterns: vec!["*_generated.py".to_string(), "vendor/**".to_string()],
        ..Config::default()
    };

    assert_eq!(
        discovered_rel_paths(&config, root),
        vec!["keep.py".to_string()],
        "config.ignore_patterns must exclude *_generated.py and everything under vendor/**"
    );
}

#[test]
fn non_source_files_skipped() {
    // .md, .txt, and an extension-less file are not source files and must not be returned.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "code.py", "def code():\n    return 1\n");
    write_file(root, "README.md", "# readme\n");
    write_file(root, "notes.txt", "just notes\n");
    write_file(root, "LICENSE", "MIT\n");

    let config = config_with_languages(vec![Language::Python]);

    assert_eq!(
        discovered_file_names(&config, root),
        vec!["code.py".to_string()],
        "non-source files (.md, .txt, extension-less) must be skipped"
    );
}

// ═══════════════════════ Slice M5.2 — full index (`index_all`) ═══════════════════════
//
// API under test (pinned for the engineering lead; root passed explicitly to `Indexer::new`):
// ```ignore
// pub struct IndexStats { pub files_processed: usize, pub chunks_indexed: usize, pub duration_ms: u64 }
// impl Indexer {
//     pub fn new(config: Config, storage: Storage, root: PathBuf) -> Result<Indexer, IndexError>;
//     pub fn index_all(&mut self) -> Result<IndexStats, IndexError>;
// }
// ```
// The `Indexer` takes an explicit `root: PathBuf` (third arg) — discovery walks `config.index_paths`
// resolved against `root`, defaulting to `root` itself when `index_paths` is empty (the M5.1
// default). Tests point `root` at a `tempfile::TempDir` so nothing touches the working tree.

/// Build a `Storage` backed by a fresh on-disk SQLite db inside `root` (schema initialized).
/// Returns the `Storage`; the db file lives under the test's `TempDir` and is cleaned up with it.
fn fresh_storage(root: &Path) -> Storage {
    let db_path = root.join("index.db");
    let storage = Storage::new(&db_path).expect("open fresh storage db");
    storage.init_schema().expect("init schema");
    storage
}

/// Construct an `Indexer` over a Python-only config rooted at `root`, sharing `storage`.
fn python_indexer(root: &Path, storage: Storage) -> Indexer {
    let config = config_with_languages(vec![Language::Python]);
    Indexer::new(config, storage, root.to_path_buf()).expect("construct Indexer")
}

/// Read an `index_state` integer counter (stored as text) as a `usize`, panicking with context on
/// a missing/garbled value so a wrong total is a clear failure rather than a silent zero.
fn index_state_count(storage: &Storage, key: &str) -> usize {
    let raw = storage
        .get_index_state(key)
        .expect("read index_state")
        .unwrap_or_else(|| panic!("index_state key {key:?} must be set after a full index"));
    raw.parse::<usize>()
        .unwrap_or_else(|e| panic!("index_state {key:?} = {raw:?} must parse as usize: {e}"))
}

#[test]
fn index_all_populates_storage_with_expected_chunk_count() {
    // Two fully-controlled Python files, each exactly one top-level function ⇒ exactly one chunk
    // each (M4: a single top-level definition yields exactly one chunk). Total = 2 chunks, and
    // each function's name is BM25-searchable after indexing.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "alpha.py", "def alpha_fn():\n    return 1\n");
    write_file(root, "beta.py", "def beta_fn():\n    return 2\n");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());
    let stats = indexer.index_all().expect("index_all must succeed");

    // Exact chunk count: two single-function files ⇒ exactly two chunks.
    assert_eq!(
        stats.chunks_indexed, 2,
        "two single-function files must index exactly two chunks"
    );

    // Each function's symbol is searchable by name.
    let alpha_hits = storage.search("alpha_fn", 10).expect("search alpha_fn");
    assert!(
        alpha_hits.iter().any(|h| h.chunk.symbol_name == "alpha_fn"),
        "alpha_fn must be searchable after index_all, got {:?}",
        alpha_hits
            .iter()
            .map(|h| &h.chunk.symbol_name)
            .collect::<Vec<_>>()
    );
    let beta_hits = storage.search("beta_fn", 10).expect("search beta_fn");
    assert!(
        beta_hits.iter().any(|h| h.chunk.symbol_name == "beta_fn"),
        "beta_fn must be searchable after index_all, got {:?}",
        beta_hits
            .iter()
            .map(|h| &h.chunk.symbol_name)
            .collect::<Vec<_>>()
    );
}

#[test]
fn index_all_writes_files_metadata_for_each_file() {
    // After a full index every source file has a FileMeta row: non-empty content_hash,
    // file_size > 0, correct language, and chunk_count matching what was inserted for that file.
    // `solo.py` has exactly one top-level function ⇒ chunk_count == 1.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "solo.py", "def solo_fn():\n    return 42\n");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());
    indexer.index_all().expect("index_all must succeed");

    let path = root.join("solo.py");
    let meta = storage
        .get_file_meta(&path)
        .expect("read file meta")
        .expect("solo.py must have a files_metadata row after index_all");

    assert!(
        !meta.content_hash.is_empty(),
        "content_hash must be non-empty"
    );
    assert!(
        meta.file_size > 0,
        "file_size must be > 0 for a non-empty file"
    );
    assert_eq!(
        meta.language,
        Language::Python,
        "language must be recorded as Python"
    );
    assert_eq!(
        meta.chunk_count, 1,
        "chunk_count must match the chunks inserted for solo.py (one top-level function)"
    );
}

#[test]
fn index_all_updates_index_state_totals() {
    // §5.1 step 4: after a full index, index_state total_files / total_chunks reflect the run.
    // Two single-function files ⇒ total_files = 2, total_chunks = 2.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "one.py", "def one_fn():\n    return 1\n");
    write_file(root, "two.py", "def two_fn():\n    return 2\n");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());
    indexer.index_all().expect("index_all must succeed");

    assert_eq!(
        index_state_count(&storage, "total_files"),
        2,
        "index_state total_files must equal the number of indexed source files"
    );
    assert_eq!(
        index_state_count(&storage, "total_chunks"),
        2,
        "index_state total_chunks must equal the total chunks inserted"
    );
}

#[test]
fn index_all_returns_indexstats_with_counts_and_duration() {
    // index_all returns IndexStats { files_processed, chunks_indexed, duration_ms }. We assert the
    // counts exactly (two single-function files) and that duration_ms is a u64 field (>= 0 always
    // holds for u64 — we assert the field's presence/type, not a timing value, to stay deterministic).
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "first.py", "def first_fn():\n    return 1\n");
    write_file(root, "second.py", "def second_fn():\n    return 2\n");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage);
    let stats: IndexStats = indexer.index_all().expect("index_all must succeed");

    assert_eq!(
        stats.files_processed, 2,
        "files_processed must equal the number of source files indexed"
    );
    assert_eq!(
        stats.chunks_indexed, 2,
        "chunks_indexed must equal the total chunks inserted"
    );
    // duration_ms is a u64; this both type-checks the field and asserts the trivially-true bound.
    let _duration: u64 = stats.duration_ms;
    assert!(
        stats.duration_ms < u64::MAX,
        "duration_ms must be a real u64 measurement, not a sentinel"
    );
}

#[test]
fn malformed_file_in_repo_does_not_abort_index() {
    // D2: a repo with one valid and one syntactically-broken .py. index_all() must SUCCEED (Ok),
    // index the valid file's chunks, and never panic. The broken file may be chunked heuristically
    // or skipped — either is acceptable; the batch must not abort and the good file must be present.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "good.py", "def good_fn():\n    return 1\n");
    // Deeply broken: unbalanced delimiters / garbage tokens ⇒ high parser ERROR rate.
    write_file(
        root,
        "broken.py",
        "def broken(:\n    @@@ ))) ((( {{{ ]][[\n=== +++ ***\n",
    );

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());

    let result = indexer.index_all();
    assert!(
        result.is_ok(),
        "a malformed file must NOT abort the batch (D2): {result:?}"
    );

    // The valid file's symbol survived the batch and is searchable.
    let hits = storage.search("good_fn", 10).expect("search good_fn");
    assert!(
        hits.iter().any(|h| h.chunk.symbol_name == "good_fn"),
        "the valid file's symbol must be indexed even when a sibling file is malformed, got {:?}",
        hits.iter()
            .map(|h| &h.chunk.symbol_name)
            .collect::<Vec<_>>()
    );
}
