//! M1 slices M1.2–M1.4 — storage (SQLite + FTS5) integration tests (RED first).
//!
//! Scenarios: docs/TEST_STRATEGY.md#storage-sqlite--fts5 and docs/plans/M1-config-storage.md.
//! API anchor: docs/project_plan.md §3.2.2 (API) + §4.1 (schema) + §4.3 (types).
//! All DB state isolated via `tempfile`; assertions check real column values and ordering.

use std::path::{Path, PathBuf};

use codecache::storage::{Storage, StorageError};
use codecache::types::{Chunk, FileMeta, Language, SymbolType};

// ───────────────────────── M8.3 / D19 — `symbols_for_path` skeleton helpers ─────────────────────────

/// Build a `Chunk` with a controlled file/symbol/type/parent and an explicit 1-based inclusive
/// line range, so the D19 `symbols_for_path` ordering + projection can be asserted exactly. The
/// `chunk_text` is irrelevant to the skeleton (D19 returns no body) but must be non-empty so the
/// row inserts cleanly.
#[allow(clippy::too_many_arguments)]
fn outline_chunk(
    file: &str,
    name: &str,
    symbol_type: SymbolType,
    parent: Option<&str>,
    start_line: usize,
    end_line: usize,
) -> Chunk {
    Chunk {
        symbol_name: name.to_string(),
        symbol_type,
        file_path: PathBuf::from(file),
        start_byte: 0,
        end_byte: 1,
        start_line,
        end_line,
        chunk_text: format!("def {name}(): pass"),
        language: Language::Python,
        parent_symbol: parent.map(str::to_string),
        file_docstring: None,
        imports: Vec::new(),
        cross_references: Vec::new(),
        is_heuristic: false,
    }
}

/// Build a `Storage` over a fresh temp DB with the schema initialized.
/// Returns (dir, storage); keep `dir` alive for the test's duration.
fn fresh_storage() -> (tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("index.db");
    let storage = Storage::new(&db_path).expect("open/create db");
    storage.init_schema().expect("init schema");
    (dir, storage)
}

/// Construct a minimal-but-complete `Chunk` for a given file/symbol/body.
fn chunk(file: &str, name: &str, body: &str) -> Chunk {
    Chunk {
        symbol_name: name.to_string(),
        symbol_type: SymbolType::Function,
        file_path: PathBuf::from(file),
        start_byte: 0,
        end_byte: body.len(),
        start_line: 1,
        end_line: 10,
        chunk_text: body.to_string(),
        language: Language::Python,
        parent_symbol: None,
        file_docstring: None,
        imports: Vec::new(),
        cross_references: Vec::new(),
        is_heuristic: false,
    }
}

/// Construct a `Chunk` whose only distinguishing text lives in `file_docstring`, so a query that
/// matches must be matching the (indexed) docstring column. `symbol_name`/`chunk_text` are kept
/// free of the docstring term on purpose.
fn chunk_with_docstring(file: &str, name: &str, body: &str, docstring: &str) -> Chunk {
    let mut c = chunk(file, name, body);
    c.file_docstring = Some(docstring.to_string());
    c
}

// ───────────────────────── M1.2 — schema, idempotency, migration ─────────────────────────

#[test]
fn new_db_creates_all_tables_expects_symbols_files_index_state() {
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("index.db");
    let storage = Storage::new(&db_path).expect("open db");

    // init_schema must succeed and create the three logical stores; a search on the freshly
    // created (empty) symbols table proves the FTS5 virtual table exists (D9).
    storage.init_schema().expect("init schema creates tables");
    let results = storage
        .search("anything", 10)
        .expect("search empty symbols table");
    assert!(results.is_empty(), "fresh db search returns empty");

    // files_metadata exists and is empty.
    let hash = storage
        .get_file_hash(Path::new("nope.py"))
        .expect("query files_metadata");
    assert_eq!(hash, None, "no metadata for unknown file");
}

#[test]
fn init_schema_twice_expects_no_error_idempotent() {
    let (_dir, storage) = fresh_storage();
    // Second init must be a no-op, not an error (CREATE ... IF NOT EXISTS).
    storage
        .init_schema()
        .expect("second init_schema is idempotent");
}

#[test]
fn older_version_db_expects_migration_to_current() {
    // Open a db, init schema, then simulate an older version by setting index_state.version
    // backwards. A subsequent init_schema must migrate it forward to the current version.
    let (_dir, storage) = fresh_storage();
    storage
        .set_index_state("version", "0.0.1")
        .expect("seed older version");

    storage.init_schema().expect("re-init triggers migration");

    let version = storage
        .get_index_state("version")
        .expect("read version")
        .expect("version present");
    assert_eq!(version, "0.1.0", "migration bumps version to current");
}

#[test]
fn corrupt_db_file_expects_typed_error_not_panic() {
    // A file that is not a valid SQLite database must surface a typed error, never a panic.
    let dir = tempfile::tempdir().expect("temp dir");
    let db_path = dir.path().join("corrupt.db");
    std::fs::write(&db_path, b"this is definitely not sqlite").expect("write garbage");

    let result = Storage::new(&db_path).and_then(|s| s.init_schema().map(|_| s));
    assert!(
        result.is_err(),
        "corrupt db file must yield a typed error, not a panic"
    );
}

// ───────────────────────── M1.3 — CRUD round-trip + delete-by-file ─────────────────────────

#[test]
fn insert_then_search_returns_inserted_chunk_with_fields() {
    let (_dir, storage) = fresh_storage();
    let mut c = chunk(
        "src/auth/handlers.py",
        "authenticate_user",
        "def authenticate_user(username, password):\n    return verify(username, password)",
    );
    c.symbol_type = SymbolType::Function;
    c.start_byte = 12;
    c.end_byte = 90;
    c.start_line = 45;
    c.end_line = 67;
    c.language = Language::Python;

    storage.insert_chunks(&[c.clone()]).expect("insert chunk");

    let results = storage.search("authenticate_user", 10).expect("search");
    assert_eq!(results.len(), 1, "exactly one match");
    let got = &results[0].chunk;
    assert_eq!(got.symbol_name, "authenticate_user");
    assert_eq!(got.symbol_type, SymbolType::Function);
    assert_eq!(got.file_path, PathBuf::from("src/auth/handlers.py"));
    assert_eq!(got.start_byte, 12);
    assert_eq!(got.end_byte, 90);
    assert_eq!(got.start_line, 45, "D7: start_line round-trips");
    assert_eq!(got.end_line, 67, "D7: end_line round-trips");
    assert_eq!(got.language, Language::Python);
    assert_eq!(got.chunk_text, c.chunk_text);
}

#[test]
fn insert_then_search_round_trips_some_file_docstring() {
    // D3: file_docstring is a persisted+indexed enrichment column. A Some(..) value must survive
    // insert → search byte-for-byte. The docstring term is unique so the chunk is findable, but
    // we assert on the *reconstructed field*, not just the match.
    let (_dir, storage) = fresh_storage();
    let c = chunk_with_docstring(
        "src/payments/gateway.py",
        "charge",
        "def charge(amount):\n    pass",
        "Module docstring: orchestrates the stripe billing workflow.",
    );

    storage.insert_chunks(&[c.clone()]).expect("insert chunk");

    let results = storage.search("charge", 10).expect("search");
    assert_eq!(results.len(), 1, "exactly one match");
    assert_eq!(
        results[0].chunk.file_docstring,
        Some("Module docstring: orchestrates the stripe billing workflow.".to_string()),
        "Some(file_docstring) round-trips through insert→search"
    );
}

#[test]
fn insert_then_search_round_trips_none_file_docstring() {
    // The absence of a docstring must round-trip as None (not Some("")). `chunk()` sets None.
    let (_dir, storage) = fresh_storage();
    let c = chunk("src/util.py", "helper", "def helper():\n    pass");
    assert_eq!(
        c.file_docstring, None,
        "helper builds a None-docstring chunk"
    );

    storage.insert_chunks(&[c]).expect("insert chunk");

    let results = storage.search("helper", 10).expect("search");
    assert_eq!(results.len(), 1, "exactly one match");
    assert_eq!(
        results[0].chunk.file_docstring, None,
        "absent docstring round-trips as None, not Some(\"\")"
    );
}

#[test]
fn bulk_insert_many_chunks_expects_all_present() {
    let (_dir, storage) = fresh_storage();
    let chunks: Vec<Chunk> = (0..50)
        .map(|i| {
            chunk(
                "src/mod.py",
                &format!("widget_{i}"),
                &format!("def widget_{i}():\n    return shared_marker_term"),
            )
        })
        .collect();

    storage.insert_chunks(&chunks).expect("bulk insert");

    let results = storage
        .search("shared_marker_term", 100)
        .expect("search all");
    assert_eq!(results.len(), 50, "all 50 chunks present");
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// M10 / D20 — batch indexer inserts: savepoint-per-item isolation inside ONE outer transaction.
//
// The D20 fix amortizes the indexer's per-file commit/fsync overhead by batching every per-file
// write across an index run into a SINGLE outer transaction — while PRESERVING D2 per-file error
// isolation via a SAVEPOINT per item: one item's DB error must roll back ONLY that item, not the
// good items already written in the same outer transaction.
//
// These tests fail to COMPILE today — `Storage::write_in_transaction` and the `BatchWriter` it
// lends do not exist yet. That compile error IS the RED state; the GREEN target is the eng lead's.
//
// PINNED CONTRACT (the tests are the spec; the eng lead must honor this exact shape — though the
// internal SQL/SAVEPOINT plumbing is theirs):
//   - Signature (on `Storage`):
//       pub fn write_in_transaction<T, F>(&self, items: &[T], each: F) -> Result<Vec<Result<()>>>
//       where F: FnMut(&BatchWriter<'_>, &T) -> Result<()>;
//   - ONE outer transaction wraps the whole call (so the batch pays one commit/fsync). For each
//     `item` in `items`, in order: open a SAVEPOINT, run `each(writer, item)` — which performs the
//     item's writes through `writer` so they participate in that savepoint:
//       * `each` returns Ok(())  ⇒ RELEASE the savepoint (the item's writes persist within the tx);
//       * `each` returns Err(e)  ⇒ ROLLBACK TO the savepoint (discard the item's partial writes),
//                                   record Err(e), and CONTINUE to the next item (no abort, no panic).
//   - After all items, COMMIT the outer transaction once — committing every survivor atomically.
//   - Returns `Ok(per_item)` where `per_item[i]` is the result of `each` for `items[i]` (same order,
//     same length). The OUTER `Result` is `Err` only for a non-isolatable failure (e.g. the outer
//     BEGIN/COMMIT or a poisoned lock) — never for a single item's error.
//   - `BatchWriter<'_>` lends the per-item write ops the indexer needs, each executing against the
//     CURRENT savepoint (NOT re-locking the connection — it borrows the open transaction):
//       pub fn insert_chunks(&self, chunks: &[Chunk]) -> Result<()>;
//       pub fn delete_chunks_for_file(&self, file_path: &Path) -> Result<()>;
//       pub fn update_file_hash(&self, file_path: &Path, meta: &FileMeta) -> Result<()>;
//     (Same semantics as the autocommit `Storage` methods of the same names, but transactional.)
// ═══════════════════════════════════════════════════════════════════════════════════════════════

#[test]
fn write_in_transaction_commits_all_survivors_in_one_batch() {
    // Happy path: three per-file items, each inserting one chunk through the BatchWriter, all Ok.
    // Every item's chunk must be searchable after the single outer commit, and every per-item
    // result must be Ok — i.e. batching does not drop or reorder successful writes.
    let (_dir, storage) = fresh_storage();

    let files = vec!["a.py", "b.py", "c.py"];
    let per_item = storage
        .write_in_transaction(&files, |writer, file| {
            let name = file.trim_end_matches(".py");
            writer.insert_chunks(&[chunk(
                file,
                name,
                &format!("def {name}():\n    return batch_marker_term"),
            )])
        })
        .expect("outer transaction must commit (no non-isolatable failure)");

    assert_eq!(
        per_item.len(),
        3,
        "one per-item result per input item, in order"
    );
    assert!(
        per_item.iter().all(std::result::Result::is_ok),
        "all three items succeeded, so every per-item result is Ok, got {per_item:?}"
    );

    let hits = storage
        .search("batch_marker_term", 100)
        .expect("search batched chunks");
    let mut names: Vec<String> = hits.into_iter().map(|h| h.chunk.symbol_name).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
        "all three batched chunks committed atomically in one outer transaction"
    );
}

#[test]
fn write_in_transaction_isolates_one_items_db_error_via_savepoint() {
    // THE LOAD-BEARING D20 GUARANTEE. A three-item batch where the MIDDLE item fails AFTER it has
    // already written a chunk inside its savepoint. The savepoint must roll that partial write back
    // so it never reaches search, while the FIRST and THIRD items (good) still commit in the SAME
    // outer transaction. This is exactly what a naive single outer transaction (rollback-all on any
    // error) would get WRONG — and what the savepoint-per-item design must get right.
    let (_dir, storage) = fresh_storage();

    // Marker shared by every chunk so one search surfaces whichever items survived the batch.
    let body = "def stub():\n    return savepoint_marker_term";
    let items = vec!["good_first.py", "doomed_middle.py", "good_last.py"];

    let per_item = storage
        .write_in_transaction(&items, |writer, file| {
            let name = file.trim_end_matches(".py");
            // Every item writes a chunk FIRST (so the doomed item has an in-savepoint partial write).
            writer.insert_chunks(&[chunk(file, name, body)])?;
            // The middle item then fails — its just-written chunk must be rolled back to the
            // savepoint, not committed, while the other items' writes survive.
            if *file == "doomed_middle.py" {
                return Err(StorageError::CorruptRow(
                    "synthetic mid-savepoint per-item failure (D20 isolation probe)".to_string(),
                ));
            }
            Ok(())
        })
        .expect(
            "the outer transaction must still commit despite one item's error (D2 under batching)",
        );

    // Per-item results: good, err, good — in order. The batch did NOT abort on the middle error.
    assert_eq!(per_item.len(), 3, "one per-item result per item");
    assert!(
        per_item[0].is_ok(),
        "first (good) item committed: {per_item:?}"
    );
    assert!(
        per_item[1].is_err(),
        "the doomed middle item's per-item result is Err, surfaced not swallowed: {per_item:?}"
    );
    assert!(
        per_item[2].is_ok(),
        "third (good) item committed: {per_item:?}"
    );

    // Observable DB state: ONLY the two good items' chunks are searchable; the doomed item's
    // partial write was rolled back to its savepoint.
    let hits = storage
        .search("savepoint_marker_term", 100)
        .expect("search after isolated-failure batch");
    let mut names: Vec<String> = hits.into_iter().map(|h| h.chunk.symbol_name).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["good_first".to_string(), "good_last".to_string()],
        "the failed item's in-savepoint write is rolled back; the good siblings stay committed"
    );
}

#[test]
fn write_in_transaction_failed_item_does_not_discard_committed_siblings() {
    // A sharper restatement of the savepoint guarantee against the naive-rollback failure mode: even
    // when the FIRST item fails, the LATER good items must still commit (a naive `?`-on-error inside
    // one transaction would either abort before they ran or roll the whole tx back). After the batch,
    // exactly the surviving items are present — the failing item contributes nothing.
    let (_dir, storage) = fresh_storage();

    let body = "def stub():\n    return order_marker_term";
    let items = vec!["doomed_first.py", "good_second.py", "good_third.py"];

    let per_item = storage
        .write_in_transaction(&items, |writer, file| {
            let name = file.trim_end_matches(".py");
            writer.insert_chunks(&[chunk(file, name, body)])?;
            if *file == "doomed_first.py" {
                return Err(StorageError::CorruptRow(
                    "synthetic first-item failure".to_string(),
                ));
            }
            Ok(())
        })
        .expect("outer transaction commits the survivors even when the first item fails");

    assert!(per_item[0].is_err(), "first item failed: {per_item:?}");
    assert!(
        per_item[1].is_ok() && per_item[2].is_ok(),
        "later items committed: {per_item:?}"
    );

    let hits = storage
        .search("order_marker_term", 100)
        .expect("search after first-item-failure batch");
    let mut names: Vec<String> = hits.into_iter().map(|h| h.chunk.symbol_name).collect();
    names.sort();
    assert_eq!(
        names,
        vec!["good_second".to_string(), "good_third".to_string()],
        "a failing first item must NOT discard the good items committed later in the same batch"
    );
}

#[test]
fn delete_chunks_for_file_removes_only_that_files_chunks() {
    let (_dir, storage) = fresh_storage();
    storage
        .insert_chunks(&[
            chunk("a.py", "alpha", "def alpha():\n    common_term"),
            chunk("b.py", "beta", "def beta():\n    common_term"),
        ])
        .expect("insert two files");

    storage
        .delete_chunks_for_file(Path::new("a.py"))
        .expect("delete a.py chunks");

    let results = storage
        .search("common_term", 10)
        .expect("search after delete");
    assert_eq!(results.len(), 1, "only b.py remains");
    assert_eq!(results[0].chunk.file_path, PathBuf::from("b.py"));
}

#[test]
fn update_then_get_file_hash_round_trips_filemeta() {
    let (_dir, storage) = fresh_storage();
    let path = Path::new("src/auth/handlers.py");
    let meta = FileMeta {
        content_hash: "0123456789abcdef0123456789abcdef".to_string(),
        mtime: 1_700_000_000,
        file_size: 2048,
        language: Language::Python,
        chunk_count: 7,
    };

    storage
        .update_file_hash(path, &meta)
        .expect("write file meta");

    let hash = storage.get_file_hash(path).expect("read hash");
    assert_eq!(
        hash,
        Some("0123456789abcdef0123456789abcdef".to_string()),
        "content_hash round-trips"
    );

    // The full FileMeta must be persisted (D6) — verify via the read-back accessor.
    let stored = storage
        .get_file_meta(path)
        .expect("read meta")
        .expect("meta present");
    assert_eq!(stored.content_hash, meta.content_hash);
    assert_eq!(stored.mtime, meta.mtime);
    assert_eq!(stored.file_size, meta.file_size);
    assert_eq!(stored.language, meta.language);
    assert_eq!(stored.chunk_count, meta.chunk_count);
}

#[test]
fn empty_db_search_expects_empty_vec() {
    let (_dir, storage) = fresh_storage();
    let results = storage.search("authenticate", 20).expect("search empty db");
    assert!(results.is_empty(), "no rows ⇒ empty vec, not an error");
}

// ───────────────────────── M1.4 — FTS5 MATCH + bm25 ordering ─────────────────────────

#[test]
fn match_query_returns_rows_containing_term() {
    let (_dir, storage) = fresh_storage();
    storage
        .insert_chunks(&[
            chunk(
                "auth.py",
                "login",
                "def login():\n    authenticate the request",
            ),
            chunk("math.py", "add", "def add(a, b):\n    return a + b"),
        ])
        .expect("insert");

    let results = storage.search("authenticate", 10).expect("match search");
    assert_eq!(results.len(), 1, "only the matching row");
    assert_eq!(results[0].chunk.symbol_name, "login");
}

#[test]
fn bm25_orders_more_relevant_chunk_first() {
    let (_dir, storage) = fresh_storage();
    // One chunk repeats the term many times → higher relevance; one mentions it once.
    let dense = chunk("dense.py", "dense", "token token token token token here");
    let sparse = chunk("sparse.py", "sparse", "token appears once here");
    storage
        .insert_chunks(&[sparse, dense])
        .expect("insert both");

    let results = storage.search("token", 10).expect("ranked search");
    assert_eq!(results.len(), 2, "both match");
    assert_eq!(
        results[0].chunk.symbol_name, "dense",
        "more relevant chunk ranks first"
    );
    // FTS5 bm25() returns more-negative for better matches; lower (more negative) sorts first.
    assert!(
        results[0].bm25_score <= results[1].bm25_score,
        "scores are ordered best-first ({} <= {})",
        results[0].bm25_score,
        results[1].bm25_score
    );
}

#[test]
fn unindexed_columns_not_searchable() {
    let (_dir, storage) = fresh_storage();
    // The unique term lives only in the file_path (UNINDEXED) — must NOT match.
    storage
        .insert_chunks(&[chunk(
            "src/uniquepathtoken/m.py",
            "handler",
            "def handler():\n    pass",
        )])
        .expect("insert");

    let results = storage
        .search("uniquepathtoken", 10)
        .expect("search unindexed term");
    assert!(
        results.is_empty(),
        "terms only in UNINDEXED columns are not searchable"
    );
}

#[test]
fn term_only_in_file_docstring_is_matchable() {
    // D3: file_docstring is an INDEXED column. A term that appears only in the docstring (not in
    // symbol_name or chunk_text) must be matchable — this distinguishes "indexed" from merely
    // "stored" and is the regression guard for the missing-column gate failure.
    let (_dir, storage) = fresh_storage();
    storage
        .insert_chunks(&[chunk_with_docstring(
            "src/telemetry/mod.py",
            "emit",
            "def emit(event):\n    pass",
            "This module handles distributedtracingspans for the collector.",
        )])
        .expect("insert");

    let results = storage
        .search("distributedtracingspans", 10)
        .expect("search docstring-only term");
    assert_eq!(
        results.len(),
        1,
        "a term living only in the indexed file_docstring is searchable"
    );
    assert_eq!(results[0].chunk.symbol_name, "emit");
}

#[test]
fn column_weighting_respected() {
    let (_dir, storage) = fresh_storage();
    // "session" appears as the symbol_name of one chunk and only in the body of another.
    let name_match = chunk("a.py", "session", "def session():\n    pass");
    let body_match = chunk(
        "b.py",
        "helper",
        "def helper():\n    open the session and the session again",
    );
    storage
        .insert_chunks(&[body_match, name_match])
        .expect("insert");

    let results = storage.search("session", 10).expect("weighted search");
    assert_eq!(results.len(), 2);
    assert_eq!(
        results[0].chunk.symbol_name, "session",
        "symbol_name (weighted) match outranks body-only match"
    );
}

// ───────────────────────── Cross-cutting (TEST_STRATEGY) ─────────────────────────

#[test]
fn utf8_multibyte_identifier_round_trips_through_search() {
    let (_dir, storage) = fresh_storage();
    storage
        .insert_chunks(&[chunk(
            "héllo.py",
            "café_handler",
            "def café_handler():\n    return münchen",
        )])
        .expect("insert multibyte");

    let results = storage
        .search("café_handler", 10)
        .expect("search multibyte");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk.symbol_name, "café_handler");
    assert_eq!(results[0].chunk.file_path, PathBuf::from("héllo.py"));
}

#[test]
fn same_inserts_expect_deterministic_ordering() {
    let build = || {
        let dir = tempfile::tempdir().expect("temp dir");
        let storage = Storage::new(&dir.path().join("index.db")).expect("open");
        storage.init_schema().expect("schema");
        storage
            .insert_chunks(&[
                chunk("a.py", "one", "term term other"),
                chunk("b.py", "two", "term something"),
                chunk("c.py", "three", "term term term"),
            ])
            .expect("insert");
        let names: Vec<String> = storage
            .search("term", 10)
            .expect("search")
            .into_iter()
            .map(|r| r.chunk.symbol_name)
            .collect();
        (dir, names)
    };
    let (_d1, first) = build();
    let (_d2, second) = build();
    assert_eq!(first, second, "identical inserts ⇒ identical ordering");
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// M8.3 — D19: `Storage::symbols_for_path(&Path) -> Result<Vec<SymbolOutline>>` (RED).
//
// The D19 contract (ROADMAP D19 + project_plan §3.2.2 / §8.2 Tool 3): a path-scoped, read-only
// column SELECT over the contentful `symbols` FTS5 table returning the SLIM skeleton projection
//   SymbolOutline { symbol_name, symbol_type, parent_symbol, file_path, start_line, end_line }
// (NOT a full Chunk — no chunk_text/imports/etc.), for an EXACT file path OR a directory prefix
// (`<dir>/%`), ordered deterministically by `(file_path, start_line, end_line)`. Zero source reads
// (D7): the line columns come straight off the stored UNINDEXED columns.
//
// These tests fail to COMPILE now — neither `Storage::symbols_for_path` nor `types::SymbolOutline`
// exists. That compile error is the RED state; it is the GREEN target for the eng-lead.
//
// PINNED DECISIONS (the eng-lead must honor — the tests are the contract):
//   - Signature: `pub fn symbols_for_path(&self, path: &Path) -> storage::Result<Vec<SymbolOutline>>`.
//   - `SymbolOutline` lives in `codecache::types` with the six fields above; `symbol_type` is the
//     typed `SymbolType` enum (not a string); `parent_symbol` is `Option<String>`; lines are
//     1-based inclusive `usize` (D7). Derives at least `Debug` + `Clone` + `PartialEq` + `Eq`.
//   - Path semantics: a query path that EXACTLY equals a stored `file_path` returns that file's
//     symbols; a query path that is a DIRECTORY returns every symbol whose `file_path` is under it
//     (prefix `<dir>/%`), but NOT a sibling file that merely shares the prefix string. Unknown
//     path ⇒ empty `Vec`, never an error.
//   - Ordering: `(file_path, start_line, end_line)` ascending — stable + deterministic.
// ═══════════════════════════════════════════════════════════════════════════════════════════════

/// Seed three files' worth of symbols and return the storage handle. Layout:
///   src/a.py        → b_func(10-12), a_class(1-8), a_method(3-7)   [unsorted on insert]
///   src/sub/b.py    → b_top(1-4)
///   other.py        → other_fn(1-2)
/// The deliberately out-of-order insert for `src/a.py` lets the ordering assertion prove
/// `symbols_for_path` sorts by `(file_path, start_line, end_line)` rather than echoing insert order.
fn seed_outline_storage() -> (tempfile::TempDir, Storage) {
    let (dir, storage) = fresh_storage();
    storage
        .insert_chunks(&[
            outline_chunk("src/a.py", "b_func", SymbolType::Function, None, 10, 12),
            outline_chunk("src/a.py", "a_class", SymbolType::Class, None, 1, 8),
            outline_chunk(
                "src/a.py",
                "a_method",
                SymbolType::Method,
                Some("a_class"),
                3,
                7,
            ),
            outline_chunk("src/sub/b.py", "b_top", SymbolType::Function, None, 1, 4),
            outline_chunk("other.py", "other_fn", SymbolType::Function, None, 1, 2),
        ])
        .expect("seed outline chunks");
    (dir, storage)
}

#[test]
fn symbols_for_path_exact_file_returns_its_symbols_ordered() {
    // An exact file path returns ONLY that file's symbols, ordered by (start_line, end_line).
    let (_dir, storage) = seed_outline_storage();

    let rows = storage
        .symbols_for_path(Path::new("src/a.py"))
        .expect("symbols_for_path on an exact file");

    // Only src/a.py's three symbols (not src/sub/b.py, not other.py).
    let names: Vec<&str> = rows.iter().map(|s| s.symbol_name.as_str()).collect();
    assert_eq!(
        names,
        vec!["a_class", "a_method", "b_func"],
        "exact-file outline returns that file's symbols ordered by start_line (1,3,10)"
    );

    // Every row is scoped to the queried file.
    for row in &rows {
        assert_eq!(
            row.file_path,
            PathBuf::from("src/a.py"),
            "exact-file outline rows all belong to the queried file"
        );
    }

    // The slim projection round-trips type, parent, and the D7 1-based inclusive line range.
    let a_class = &rows[0];
    assert_eq!(
        a_class.symbol_type,
        SymbolType::Class,
        "symbol_type is typed"
    );
    assert_eq!(a_class.parent_symbol, None, "top-level class has no parent");
    assert_eq!((a_class.start_line, a_class.end_line), (1, 8), "D7 lines");

    let a_method = &rows[1];
    assert_eq!(a_method.symbol_type, SymbolType::Method);
    assert_eq!(
        a_method.parent_symbol.as_deref(),
        Some("a_class"),
        "parent_symbol is projected for nested symbols"
    );
    assert_eq!((a_method.start_line, a_method.end_line), (3, 7));
}

#[test]
fn symbols_for_path_directory_prefix_returns_all_under_it() {
    // A directory path returns every symbol whose file is under it (src/a.py + src/sub/b.py),
    // ordered by (file_path, start_line, end_line); a sibling outside the dir (other.py) is excluded.
    let (_dir, storage) = seed_outline_storage();

    let rows = storage
        .symbols_for_path(Path::new("src"))
        .expect("symbols_for_path on a directory");

    // file_path set is exactly the two files under src/ — other.py is NOT included.
    let mut files: Vec<String> = rows
        .iter()
        .map(|s| s.file_path.to_string_lossy().into_owned())
        .collect();
    files.sort();
    files.dedup();
    assert_eq!(
        files,
        vec!["src/a.py".to_string(), "src/sub/b.py".to_string()],
        "directory outline spans every file under the prefix, excluding siblings (other.py)"
    );
    assert!(
        !rows
            .iter()
            .any(|s| s.file_path == PathBuf::from("other.py")),
        "a sibling file outside the queried directory must not appear"
    );

    // Ordered by (file_path, start_line): src/a.py(1,3,10) then src/sub/b.py(1).
    let ordered: Vec<(String, usize)> = rows
        .iter()
        .map(|s| (s.file_path.to_string_lossy().into_owned(), s.start_line))
        .collect();
    assert_eq!(
        ordered,
        vec![
            ("src/a.py".to_string(), 1),
            ("src/a.py".to_string(), 3),
            ("src/a.py".to_string(), 10),
            ("src/sub/b.py".to_string(), 1),
        ],
        "directory outline ordered by (file_path, start_line, end_line)"
    );
}

#[test]
fn symbols_for_path_unknown_path_returns_empty() {
    // A path that matches no stored file (neither exact nor prefix) yields an empty Vec, not an error.
    let (_dir, storage) = seed_outline_storage();

    let rows = storage
        .symbols_for_path(Path::new("does/not/exist"))
        .expect("unknown path is a well-formed empty result, not an error");
    assert!(
        rows.is_empty(),
        "unknown path ⇒ empty Vec (well-formed), got {rows:?}"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════════════════════
// R2.2a — D24: `Storage::search_with_weights(query, limit, Option<&[f64; 7]>)` (RED).
//
// The crate flag underlying `codecache query --bm25-weights "..."`: a per-invocation override of the
// 7 per-column FTS5 BM25 weights that `queries::SEARCH` currently bakes in. The 7 weights are in the
// indexed-column order of `schema::CREATE_SYMBOLS`:
//   [symbol_name, symbol_type, chunk_text, parent_symbol, imports, cross_references, file_docstring]
//
// These tests fail to COMPILE now — `Storage::search_with_weights` does not exist yet. That compile
// error IS the RED state; making it compile + pass is the eng-lead's GREEN target.
//
// PINNED CONTRACT (the tests are the spec; the eng-lead must honor these exact values):
//   - Signature: `pub fn search_with_weights(&self, query: &str, limit: usize,
//                 weights: Option<&[f64; 7]>) -> storage::Result<Vec<SearchResult>>`.
//   - DEFAULT_WEIGHTS = [10.0, 1.0, 1.0, 5.0, 2.0, 2.0, 2.0] — the value baked into `queries::SEARCH`.
//   - `None` ⇒ byte-identical to today's `search(query, limit)` AND to
//     `search_with_weights(query, limit, Some(&DEFAULT_WEIGHTS))`.
//   - Custom weights are formatted into the `bm25(symbols, …)` ranking expression (FTS5 bm25()
//     weights are auxiliary-fn args, NOT bindable `?` params); `MATCH ?1` / `LIMIT ?2` stay bound.
//   - Ordering invariant unchanged: `bm25 ASC, rowid ASC`.
//   - FTS5 accepts zero / negative weights ⇒ those must NOT error at the storage layer.
//
// REORDER SEED (verified empirically against real FTS5 — the flip is unambiguous + deterministic):
//   Seed two chunks sharing the term "session":
//     a.py / symbol_name="session", body "def session(): pass"      (term in the NAME column)
//     b.py / symbol_name="helper",  body "... session and the session again"  (term in BODY only)
//   With DEFAULT_WEIGHTS (symbol_name=10) the NAME match (a.py "session") ranks ABOVE the body-only
//   match (b.py "helper"). With REORDER_WEIGHTS = [0.0, 1.0, 5.0, 1.0, 1.0, 1.0, 1.0] (symbol_name
//   zeroed, chunk_text boosted to 5) the order FLIPS: the body-only "helper" ranks first. Distinct
//   bm25 scores under both weightings (no tie), so the flip is decided purely by score — stable.
// ═══════════════════════════════════════════════════════════════════════════════════════════════

/// The 7 per-column BM25 weights baked into `queries::SEARCH`, in `schema::CREATE_SYMBOLS` indexed-
/// column order. `None` and `Some(&DEFAULT_WEIGHTS)` must both reproduce today's `search`.
const DEFAULT_WEIGHTS: [f64; 7] = [10.0, 1.0, 1.0, 5.0, 2.0, 2.0, 2.0];

/// Custom weights that DETERMINISTICALLY flip the default name-vs-body ranking: symbol_name zeroed
/// (index 0), chunk_text boosted to 5.0 (index 2). Verified empirically against FTS5.
const REORDER_WEIGHTS: [f64; 7] = [0.0, 1.0, 5.0, 1.0, 1.0, 1.0, 1.0];

/// Seed the canonical name-vs-body reorder corpus and return the storage handle.
/// a.py: "session" is the symbol NAME (default-weighted high); b.py: "session" lives only in the
/// BODY (chunk_text). Both match the query "session"; distinct files + spans ⇒ no dedup collision.
fn seed_name_vs_body() -> (tempfile::TempDir, Storage) {
    let (dir, storage) = fresh_storage();
    let name_match = chunk("a.py", "session", "def session():\n    pass");
    let body_match = chunk(
        "b.py",
        "helper",
        "def helper():\n    open the session and the session again",
    );
    storage
        .insert_chunks(&[body_match, name_match])
        .expect("seed name-vs-body corpus");
    (dir, storage)
}

/// Project a result list down to the symbol-name order, for compact ordering assertions.
fn names_of(results: &[codecache::storage::SearchResult]) -> Vec<String> {
    results
        .iter()
        .map(|r| r.chunk.symbol_name.clone())
        .collect()
}

#[test]
fn search_with_weights_custom_reorders_ranking_vs_default() {
    // Proves the weights actually take effect: zeroing symbol_name + boosting chunk_text flips the
    // default name-match-first ordering so the body-only match leads. Asserts BOTH the default order
    // (baseline) and the flipped custom order on the same seed, so the change is unambiguous.
    let (_dir, storage) = seed_name_vs_body();

    let default = storage
        .search_with_weights("session", 10, Some(&DEFAULT_WEIGHTS))
        .expect("default-weighted search");
    assert_eq!(
        names_of(&default),
        vec!["session".to_string(), "helper".to_string()],
        "DEFAULT weights (symbol_name=10): the name match `session` outranks the body-only `helper`"
    );

    let reordered = storage
        .search_with_weights("session", 10, Some(&REORDER_WEIGHTS))
        .expect("custom-weighted search");
    assert_eq!(
        names_of(&reordered),
        vec!["helper".to_string(), "session".to_string()],
        "REORDER weights (symbol_name=0, chunk_text=5): the body-only `helper` now outranks `session`"
    );

    // The two weightings produce DIFFERENT orderings for this seed (the headline guarantee).
    assert_ne!(
        names_of(&default),
        names_of(&reordered),
        "custom weights must change the result ordering vs the default"
    );
}

#[test]
fn search_with_weights_none_is_identical_to_default() {
    // DEFAULT-IDENTICAL contract: for the same seed, `search`, `search_with_weights(.., None)`, and
    // `search_with_weights(.., Some(&DEFAULT_WEIGHTS))` return IDENTICAL Vec<SearchResult> — same
    // chunks, same order, same scores. `None` must reuse the existing default path, not a drifted copy.
    let (_dir, storage) = seed_name_vs_body();

    let via_search = storage.search("session", 10).expect("legacy search");
    let via_none = storage
        .search_with_weights("session", 10, None)
        .expect("search_with_weights None");
    let via_explicit_default = storage
        .search_with_weights("session", 10, Some(&DEFAULT_WEIGHTS))
        .expect("search_with_weights explicit default");

    assert_eq!(
        via_search, via_none,
        "search(q, lim) == search_with_weights(q, lim, None) (byte-identical default path)"
    );
    assert_eq!(
        via_search, via_explicit_default,
        "search(q, lim) == search_with_weights(q, lim, Some(&DEFAULT_WEIGHTS)) (default round-trips)"
    );
}

#[test]
fn search_with_weights_custom_is_deterministic_and_bm25_ordered() {
    // Determinism under custom weights: repeated calls with the same custom vector return identical
    // orderings, and the ordering stays bm25-ascending (best-first) — the documented invariant is
    // unchanged by the weight override.
    let (_dir, storage) = seed_name_vs_body();

    let first = storage
        .search_with_weights("session", 10, Some(&REORDER_WEIGHTS))
        .expect("custom search 1");
    for _ in 0..5 {
        let again = storage
            .search_with_weights("session", 10, Some(&REORDER_WEIGHTS))
            .expect("custom search n");
        assert_eq!(
            names_of(&first),
            names_of(&again),
            "repeated identical custom-weighted searches must yield identical ordering"
        );
    }

    // Scores are non-decreasing (bm25 ASC = best-first; FTS5 bm25() is lower-is-better).
    for w in first.windows(2) {
        assert!(
            w[0].bm25_score <= w[1].bm25_score,
            "custom-weighted results stay bm25-ascending: {} <= {}",
            w[0].bm25_score,
            w[1].bm25_score
        );
    }
}

#[test]
fn search_with_weights_zero_and_negative_do_not_error() {
    // Edge: FTS5's bm25() accepts zero and negative column weights; the storage layer must pass them
    // through and return Ok(_), never erroring. The sweep (R2.2b) may explore such vectors, so this
    // is a real contract, not a curiosity. We assert Ok + that the matching rows still come back.
    let (_dir, storage) = seed_name_vs_body();

    let weights: [f64; 7] = [0.0, 1.0, 1.0, 5.0, 2.0, 2.0, -1.0]; // a 0.0 and a -1.0 entry
    let result = storage.search_with_weights("session", 10, Some(&weights));
    assert!(
        result.is_ok(),
        "zero/negative weights must not error at the storage layer (FTS5 accepts them): {:?}",
        result.err()
    );
    let rows = result.expect("ok");
    assert_eq!(
        rows.len(),
        2,
        "both seeded chunks still match `session` under the edge weight vector"
    );
}
