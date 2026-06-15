//! R2.3a — D25: chunk-ingestion seam, LIBRARY surface (RED, test-lead).
//!
//! Pins the `app` facade entry point + its stats struct from `docs/project_plan.md` §3.2.4:
//!
//! ```ignore
//! // src/app.rs (facade), driven by the hidden `codecache ingest <CHUNKS_JSON>` command (§7.2):
//! pub fn ingest_chunks(project_root: &Path, chunks_json: &Path)
//!     -> Result<IngestStats, codecache::AppError>;
//! pub struct IngestStats { pub files_ingested: usize, pub chunks_ingested: usize }
//! ```
//!
//! These tests drive CodeCache through the PUBLIC library surface only (no CLI). The pinned
//! contract — written ASSUMING a crate-root re-export `codecache::{ingest_chunks, IngestStats}`
//! mirroring the existing `codecache::{init, index, AppError, IndexStats}` re-exports (see
//! `src/lib.rs`). If the eng-lead instead keeps these under `codecache::app::{..}`, this file's
//! `use` line is the single place to adjust; the binary e2e in `tests/e2e_ingest.rs` carries the
//! bulk of coverage and is independent of the re-export path.
//!
//! **CONTRACT QUESTION for the eng-lead (flagged in the brief's RED section):** confirm the
//! crate-root re-export path of `ingest_chunks` + `IngestStats`. This file currently imports
//! `codecache::{ingest_chunks, IngestStats}`.
//!
//! RED rationale: NEITHER `ingest_chunks` NOR `IngestStats` exists yet, so this file FAILS TO
//! COMPILE (`unresolved import codecache::ingest_chunks` / `codecache::IngestStats`). That compile
//! error IS the RED for the new-symbol contract — exactly per the brief. Once the facade exists the
//! assertions below (real `IngestStats` field values + a queryable DB) become the green target.
//!
//! DB-path resolution: `ingest_chunks` resolves `config.storage.db_path` under `project_root`
//! exactly as `init`/`index` do (default `<root>/.codecache/index.db`); the test re-opens that same
//! DB via the public `Storage::new` to assert the ingested symbols are searchable.

use std::fs;
use std::path::{Path, PathBuf};

use codecache::storage::Storage;
use codecache::{ingest_chunks, init, IngestStats};
use tempfile::TempDir;

/// Create a temp project root for one test. Removed when the returned `TempDir` is dropped.
fn temp_repo() -> TempDir {
    tempfile::tempdir().expect("create temp repo dir")
}

/// The default DB location `init`/`ingest_chunks` resolve `config.storage.db_path` to under `root`.
fn default_db_path(root: &Path) -> PathBuf {
    root.join(".codecache").join("index.db")
}

/// Write a chunks JSON file under `root` and return its path.
fn write_chunks(root: &Path, name: &str, contents: &str) -> PathBuf {
    let path = root.join(name);
    fs::write(&path, contents).expect("write chunks json");
    path
}

/// Re-open the resulting on-disk DB via the public `Storage` API and collect the sorted, deduped
/// symbol names matching `query` — a stable observable for "is this ingested symbol queryable?".
fn searchable_symbols(db_path: &Path, query: &str) -> Vec<String> {
    let storage = Storage::new(db_path).expect("re-open ingested db");
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

// A two-record, two-file chunks.json with enrichment populated on the first record.
const TWO_FILE_CHUNKS: &str = r#"[
  {
    "symbol_name": "authenticate_user",
    "symbol_type": "function",
    "file_path": "src/auth.py",
    "start_byte": 0,
    "end_byte": 52,
    "start_line": 1,
    "end_line": 3,
    "chunk_text": "def authenticate_user(name):\n    return verify(name)\n",
    "language": "python",
    "file_docstring": "Auth helpers module.",
    "imports": ["os"],
    "cross_references": ["verify"]
  },
  {
    "symbol_name": "normalize_path",
    "symbol_type": "function",
    "file_path": "src/util.py",
    "start_byte": 0,
    "end_byte": 40,
    "start_line": 1,
    "end_line": 2,
    "chunk_text": "def normalize_path(p):\n    return p\n",
    "language": "python"
  }
]"#;

// ───────────────────────────────────────────────────────────────────────────
// 1. The facade ingests a 2-file JSON into a fresh DB: returns the right
//    IngestStats (2 files / 2 chunks) and the ingested symbols are searchable
//    through the public Storage API. Pins the §3.2.4 signature + field names.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_chunks_populates_queryable_db_and_reports_stats() {
    let repo = temp_repo();
    let root = repo.path();

    init(root).expect("init must succeed on a fresh project");
    let chunks_path = write_chunks(root, "chunks.json", TWO_FILE_CHUNKS);

    let stats: IngestStats =
        ingest_chunks(root, &chunks_path).expect("ingest_chunks must succeed on valid input");

    // Two distinct file_path values ⇒ 2 files; two records ⇒ 2 chunks.
    assert_eq!(
        stats.files_ingested, 2,
        "two distinct file_path values must produce files_ingested == 2"
    );
    assert_eq!(
        stats.chunks_ingested, 2,
        "two chunk records must produce chunks_ingested == 2"
    );

    // The ingested symbols are searchable through the public Storage::search API.
    let db_path = default_db_path(root);
    assert_eq!(
        searchable_symbols(&db_path, "authenticate_user"),
        vec!["authenticate_user".to_string()],
        "the first ingested symbol must be searchable in the resulting db"
    );
    assert!(
        searchable_symbols(&db_path, "normalize_path").contains(&"normalize_path".to_string()),
        "the second ingested symbol must be searchable in the resulting db"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 2. Empty input `[]` is a clean no-op through the facade: Ok(stats) with
//    0 files / 0 chunks (NOT an error). Pins the degenerate-corpus contract.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_chunks_empty_array_returns_zero_stats() {
    let repo = temp_repo();
    let root = repo.path();

    init(root).expect("init must succeed");
    let chunks_path = write_chunks(root, "empty.json", "[]");

    let stats = ingest_chunks(root, &chunks_path).expect("empty input must be a clean no-op (Ok)");
    assert_eq!(stats.files_ingested, 0, "empty input ⇒ 0 files");
    assert_eq!(stats.chunks_ingested, 0, "empty input ⇒ 0 chunks");
}

// ───────────────────────────────────────────────────────────────────────────
// 3. Invalid input is a typed Err (never a panic) through the facade: an
//    unknown enum string returns Err(AppError), not Ok and not a panic.
//    (The full validation matrix is exercised end-to-end in tests/e2e_ingest.rs;
//    this pins that the LIBRARY surface returns a typed error.)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_chunks_invalid_input_is_typed_err() {
    let repo = temp_repo();
    let root = repo.path();

    init(root).expect("init must succeed");
    // Unknown language "rust" ⇒ from_str_lenient None ⇒ typed error.
    let json = r#"[
      {
        "symbol_name": "x",
        "symbol_type": "function",
        "file_path": "src/a.rs",
        "start_byte": 0,
        "end_byte": 10,
        "start_line": 1,
        "end_line": 2,
        "chunk_text": "fn x() {}",
        "language": "rust"
      }
    ]"#;
    let chunks_path = write_chunks(root, "bad.json", json);

    let result = ingest_chunks(root, &chunks_path);
    assert!(
        result.is_err(),
        "an unknown `language` string must yield a typed Err, not Ok; got: {result:?}"
    );
}
