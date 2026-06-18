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
use std::path::{Path, PathBuf};

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

// ═══════════════ Slice M10 / D20 — batch inserts (one outer transaction) ═══════════════
//
// D20: the indexer batches every per-file write across a run into ONE outer transaction to
// amortize commit/fsync overhead, with a SAVEPOINT per file so one file's DB error rolls back only
// that file (preserving D2). The tests below pin the OBSERVABLE behavior of the batched path so the
// eng lead is free in HOW it batches (the savepoint primitive itself is pinned in
// `storage_tests.rs::write_in_transaction_*`). They must hold against the new wiring without
// changing any public `Indexer` signature.

#[test]
fn unreadable_file_mid_batch_does_not_discard_committed_siblings() {
    // D2 UNDER BATCHING — a DIFFERENT failure stage than `malformed_file_in_repo_does_not_abort_index`
    // (that one fails at PARSE; the chunker degrades and usually returns Ok BEFORE any DB write). Here
    // a file fails at the READ stage (invalid UTF-8 ⇒ `read_to_string` errors ⇒ `IndexError::File`),
    // which surfaces a per-file Err that the pipeline isolates INSIDE the batched outer transaction.
    // Several valid siblings are indexed in the SAME run: `index_all` must return Ok, every valid
    // file's symbol must be searchable (none discarded by the bad file's failure), and totals must
    // reflect exactly the valid files. This is the indexer-surface proof that one file's mid-batch
    // failure does not roll back / abort the whole batch.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "v1.py", "def v1_fn():\n    return 1\n");
    write_file(root, "v2.py", "def v2_fn():\n    return 2\n");
    write_file(root, "v3.py", "def v3_fn():\n    return 3\n");
    // A real .py file (discovered) whose bytes are NOT valid UTF-8 ⇒ read_to_string fails ⇒ the
    // per-file pipeline errors mid-batch. Deterministic + reaches the per-file write path's caller.
    fs::write(
        root.join("unreadable.py"),
        [0x66, 0x6e, 0xff, 0xfe, 0x00, 0x80],
    )
    .expect("write invalid-UTF-8 .py file");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());

    let result = indexer.index_all();
    assert!(
        result.is_ok(),
        "a file failing mid-batch (read error) must NOT abort the batched index (D2): {result:?}"
    );

    // All THREE valid siblings survived the batch — the bad file did not roll them back.
    for sym in ["v1_fn", "v2_fn", "v3_fn"] {
        let hits = searchable_symbols(&storage, sym);
        assert!(
            hits.iter().any(|n| n == sym),
            "valid sibling symbol {sym:?} must be committed even though a batch-mate failed, got {hits:?}"
        );
    }
    // The unreadable file contributed nothing and was not counted.
    let stats = result.expect("ok");
    assert_eq!(
        stats.files_processed, 3,
        "exactly the three valid files are processed; the unreadable one is skipped (D2)"
    );
    // index_state totals reflect only the committed valid files (the bad file has no metadata row).
    assert_eq!(
        index_state_count(&storage, "total_files"),
        3,
        "total_files counts only the files that actually committed"
    );
    assert_eq!(
        index_state_count(&storage, "total_chunks"),
        3,
        "total_chunks counts only the chunks that actually committed"
    );
}

// ═════════════ Slice M5.3 — incremental + idempotency + delete ═════════════
//
// API under test (pinned for the engineering lead):
// ```ignore
// impl Indexer {
//     // Re-index exactly the files in `files` whose content hash changed (skip unchanged within the
//     // list per `compute_file_hash` vs `get_file_hash`). `files_processed` counts the files actually
//     // re-indexed. Each test below changes EVERY file it passes, so `files_processed == files.len()`
//     // regardless of whether the impl hash-filters or force-reindexes.
//     pub fn update_files(&mut self, files: &[PathBuf]) -> Result<IndexStats, IndexError>;
// }
// ```
// `index_all` on an already-populated DB runs in INCREMENTAL / RECONCILE mode (§5.2): skip files
// whose hash is unchanged, re-index changed files, index newly-appeared files, and reconcile
// deletions — files present in `files_metadata` but no longer on disk have their chunks deleted and
// their metadata row cleared. Tests #1/#4/#5 depend on these `index_all` reconcile semantics.

/// Searchable symbol names for `query`, sorted, deduped — a stable observable view of the chunk set
/// for before/after comparison.
fn searchable_symbols(storage: &Storage, query: &str) -> Vec<String> {
    let mut names: Vec<String> = storage
        .search(query, 100)
        .expect("search must succeed")
        .into_iter()
        .map(|h| h.chunk.symbol_name)
        .collect();
    names.sort();
    names.dedup();
    names
}

#[test]
fn reindex_unchanged_repo_performs_no_writes() {
    // Idempotency: index_all once, capture the observable state (each file's stored hash, the
    // index_state totals, each file's FileMeta content_hash/mtime, and the searchable chunk set).
    // Re-run index_all with NO file changes; every observable must be byte-identical. A re-index of
    // an unchanged file would re-stamp its FileMeta (content_hash/mtime) and delete+re-insert its
    // chunks (changing rowids/ordering) — so stable FileMeta + a stable chunk set is the integration
    // -level proxy for "no writes were issued" (a SQLite write-spy is not reachable from here; this
    // limitation is documented in the brief).
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "alpha.py", "def alpha_fn():\n    return 1\n");
    write_file(root, "beta.py", "def beta_fn():\n    return 2\n");
    let alpha = root.join("alpha.py");
    let beta = root.join("beta.py");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());
    indexer.index_all().expect("first index_all must succeed");

    // Capture-before.
    let alpha_hash_before = storage
        .get_file_hash(&alpha)
        .expect("read alpha hash")
        .expect("alpha must have a stored hash after the first index");
    let beta_hash_before = storage
        .get_file_hash(&beta)
        .expect("read beta hash")
        .expect("beta must have a stored hash after the first index");
    let total_files_before = index_state_count(&storage, "total_files");
    let total_chunks_before = index_state_count(&storage, "total_chunks");
    let alpha_meta_before = storage
        .get_file_meta(&alpha)
        .expect("read alpha meta")
        .expect("alpha meta present");
    let symbols_before = searchable_symbols(&storage, "alpha_fn OR beta_fn");

    // Re-index with no changes.
    indexer.index_all().expect("second index_all must succeed");

    // Capture-after — every observable identical.
    let alpha_hash_after = storage
        .get_file_hash(&alpha)
        .expect("read alpha hash")
        .expect("alpha hash present");
    let beta_hash_after = storage
        .get_file_hash(&beta)
        .expect("read beta hash")
        .expect("beta hash present");
    let alpha_meta_after = storage
        .get_file_meta(&alpha)
        .expect("read alpha meta")
        .expect("alpha meta present");
    let symbols_after = searchable_symbols(&storage, "alpha_fn OR beta_fn");

    assert_eq!(
        alpha_hash_before, alpha_hash_after,
        "re-index of an unchanged file must not change its stored content hash"
    );
    assert_eq!(
        beta_hash_before, beta_hash_after,
        "re-index of an unchanged file must not change its stored content hash"
    );
    assert_eq!(
        index_state_count(&storage, "total_files"),
        total_files_before,
        "total_files must be unchanged after a no-op re-index"
    );
    assert_eq!(
        index_state_count(&storage, "total_chunks"),
        total_chunks_before,
        "total_chunks must be unchanged after a no-op re-index"
    );
    // FileMeta re-stamp proxy: an unchanged file must keep its exact content_hash and mtime.
    assert_eq!(
        alpha_meta_before.content_hash, alpha_meta_after.content_hash,
        "FileMeta.content_hash must not be re-stamped for an unchanged file"
    );
    assert_eq!(
        alpha_meta_before.mtime, alpha_meta_after.mtime,
        "FileMeta.mtime must not be re-stamped for an unchanged file"
    );
    assert_eq!(
        alpha_meta_before.chunk_count, alpha_meta_after.chunk_count,
        "FileMeta.chunk_count must be unchanged for an unchanged file"
    );
    assert_eq!(
        symbols_before, symbols_after,
        "the searchable chunk set must be identical after a no-op re-index"
    );
    assert_eq!(
        symbols_after,
        vec!["alpha_fn".to_string(), "beta_fn".to_string()],
        "both functions remain searchable exactly once after the no-op re-index"
    );
}

#[test]
fn modify_one_file_reindexes_only_that_file() {
    // Modify ONE file of two; the incremental entry point re-indexes only that file: the modified
    // file's new symbol becomes searchable AND its old symbol is gone, while the untouched file's
    // chunk + stored hash are unchanged.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "changed.py", "def original_fn():\n    return 1\n");
    write_file(root, "stable.py", "def stable_fn():\n    return 2\n");
    let changed = root.join("changed.py");
    let stable = root.join("stable.py");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());
    indexer.index_all().expect("initial index_all must succeed");

    let stable_hash_before = storage
        .get_file_hash(&stable)
        .expect("read stable hash")
        .expect("stable hash present after initial index");

    // Mutate exactly one file's content: replace the function with a new symbol.
    write_file(root, "changed.py", "def renamed_fn():\n    return 99\n");

    // Incremental entry point: re-index only the changed file by passing it explicitly.
    let stats = indexer
        .update_files(&[changed.clone()])
        .expect("update_files must succeed");
    assert_eq!(
        stats.files_processed, 1,
        "exactly one file was changed and passed, so exactly one is re-indexed"
    );

    // The new symbol is searchable; the old symbol is gone (chunks for the file were replaced).
    let renamed = searchable_symbols(&storage, "renamed_fn");
    assert_eq!(
        renamed,
        vec!["renamed_fn".to_string()],
        "the modified file's new symbol must be searchable after the incremental update"
    );
    let original = searchable_symbols(&storage, "original_fn");
    assert!(
        !original.iter().any(|n| n == "original_fn"),
        "the modified file's old symbol must no longer be searchable, got {original:?}"
    );

    // The untouched file is untouched: same hash, same searchable symbol.
    let stable_hash_after = storage
        .get_file_hash(&stable)
        .expect("read stable hash")
        .expect("stable hash present");
    assert_eq!(
        stable_hash_before, stable_hash_after,
        "the untouched file's stored hash must not change during an incremental update"
    );
    let stable_syms = searchable_symbols(&storage, "stable_fn");
    assert_eq!(
        stable_syms,
        vec!["stable_fn".to_string()],
        "the untouched file's symbol remains searchable exactly once"
    );
}

#[test]
fn update_files_with_n_changed_reindexes_exactly_n() {
    // update_files(&[..N..]) re-indexes exactly the N changed files in the list. Three files are
    // indexed, then ALL THREE are modified and passed; files_processed must equal 3 and every new
    // symbol must be searchable. (Each passed file is genuinely changed, so this assertion holds
    // whether the impl force-reindexes the list or hash-filters it — see the brief's pinned
    // `update_files` semantics.)
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "f1.py", "def f1_old():\n    return 1\n");
    write_file(root, "f2.py", "def f2_old():\n    return 2\n");
    write_file(root, "f3.py", "def f3_old():\n    return 3\n");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());
    indexer.index_all().expect("initial index_all must succeed");

    // Modify all three.
    write_file(root, "f1.py", "def f1_new():\n    return 11\n");
    write_file(root, "f2.py", "def f2_new():\n    return 22\n");
    write_file(root, "f3.py", "def f3_new():\n    return 33\n");

    let mut files: Vec<PathBuf> = vec![root.join("f1.py"), root.join("f2.py"), root.join("f3.py")];
    files.sort();

    let stats = indexer
        .update_files(&files)
        .expect("update_files must succeed");
    assert_eq!(
        stats.files_processed, 3,
        "all three passed files were changed, so exactly three are re-indexed"
    );

    for sym in ["f1_new", "f2_new", "f3_new"] {
        let hits = searchable_symbols(&storage, sym);
        assert!(
            hits.iter().any(|n| n == sym),
            "the new symbol {sym:?} must be searchable after update_files, got {hits:?}"
        );
    }
    // The old symbols are replaced, not duplicated.
    for sym in ["f1_old", "f2_old", "f3_old"] {
        let hits = searchable_symbols(&storage, sym);
        assert!(
            !hits.iter().any(|n| n == sym),
            "the old symbol {sym:?} must be gone after its file was re-indexed, got {hits:?}"
        );
    }
}

#[test]
fn new_file_added_gets_indexed() {
    // index_all in reconcile mode discovers a file added after the initial index: its symbol becomes
    // searchable and a FileMeta row exists, without re-stamping the pre-existing file.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "existing.py", "def existing_fn():\n    return 1\n");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());
    indexer.index_all().expect("initial index_all must succeed");

    // Add a brand-new source file after the first index.
    write_file(root, "added.py", "def added_fn():\n    return 2\n");
    let added = root.join("added.py");

    indexer
        .index_all()
        .expect("reconcile index_all must succeed");

    // The new file's symbol is searchable and it has a FileMeta row.
    let added_syms = searchable_symbols(&storage, "added_fn");
    assert_eq!(
        added_syms,
        vec!["added_fn".to_string()],
        "a file added after the initial index must be discovered and indexed"
    );
    let added_meta = storage
        .get_file_meta(&added)
        .expect("read added meta")
        .expect("the newly-added file must have a files_metadata row after reconcile index_all");
    assert_eq!(
        added_meta.language,
        Language::Python,
        "the new file's metadata records its language"
    );
    assert_eq!(
        added_meta.chunk_count, 1,
        "the new single-function file contributes exactly one chunk"
    );

    // The pre-existing file is still searchable (not dropped during reconcile).
    let existing_syms = searchable_symbols(&storage, "existing_fn");
    assert_eq!(
        existing_syms,
        vec!["existing_fn".to_string()],
        "the pre-existing file must remain indexed after a reconcile that adds a new file"
    );

    // index_state totals reflect both files.
    assert_eq!(
        index_state_count(&storage, "total_files"),
        2,
        "total_files must reflect the newly-added file"
    );
}

#[test]
fn deleted_file_has_chunks_removed_and_metadata_cleared() {
    // After a full index, a file deleted from disk is reconciled by index_all: its chunks vanish
    // from search, its FileMeta row becomes None, and index_state totals decrease accordingly.
    let repo = temp_repo();
    let root = repo.path();
    write_file(root, "keep.py", "def keep_fn():\n    return 1\n");
    write_file(root, "doomed.py", "def doomed_fn():\n    return 2\n");
    let doomed = root.join("doomed.py");

    let storage = fresh_storage(root);
    let mut indexer = python_indexer(root, storage.clone());
    indexer.index_all().expect("initial index_all must succeed");

    // Sanity: both indexed before deletion.
    assert_eq!(index_state_count(&storage, "total_files"), 2);
    assert_eq!(index_state_count(&storage, "total_chunks"), 2);
    assert!(
        storage
            .get_file_meta(&doomed)
            .expect("read doomed meta")
            .is_some(),
        "doomed.py must have a metadata row before deletion"
    );

    // Delete the file from disk, then reconcile.
    fs::remove_file(&doomed).expect("delete doomed.py from disk");
    indexer
        .index_all()
        .expect("reconcile index_all must succeed");

    // The deleted file's chunks are gone from search.
    let doomed_syms = searchable_symbols(&storage, "doomed_fn");
    assert!(
        !doomed_syms.iter().any(|n| n == "doomed_fn"),
        "the deleted file's symbol must be removed from search, got {doomed_syms:?}"
    );
    // Its metadata row is cleared.
    assert!(
        storage
            .get_file_meta(&doomed)
            .expect("read doomed meta")
            .is_none(),
        "the deleted file's files_metadata row must be cleared after reconcile"
    );
    // The surviving file is intact.
    let keep_syms = searchable_symbols(&storage, "keep_fn");
    assert_eq!(
        keep_syms,
        vec!["keep_fn".to_string()],
        "the surviving file must remain indexed after a delete reconcile"
    );
    // Totals decreased to reflect the single remaining file/chunk.
    assert_eq!(
        index_state_count(&storage, "total_files"),
        1,
        "total_files must decrease after a file is deleted and reconciled"
    );
    assert_eq!(
        index_state_count(&storage, "total_chunks"),
        1,
        "total_chunks must decrease after a file's chunks are removed"
    );
}
