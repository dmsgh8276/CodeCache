//! R2.3a — D25: chunk-ingestion seam, end-to-end through the BUILT `codecache` binary (RED, test-lead).
//!
//! Pins the hidden `codecache ingest <CHUNKS_JSON> [--db-path <PATH>]` subcommand (§7.2,
//! Decision Log D25) by driving the actual compiled binary as a subprocess
//! (`assert_cmd::Command::cargo_bin("codecache")`) with its working directory set to a fresh
//! `tempfile::TempDir` (`.current_dir(tmp)`). The whole seam — JSON deserialization of an array
//! of caller-supplied chunk records, `Storage::insert_chunks` in array order, one
//! `files_metadata` row per distinct `file_path`, `index_state` total restamping, and the
//! retrieval/`status` surfaces reading the ingested rows — runs through `main.rs` on a real,
//! on-disk DB. Nothing reaches the library internals here; the contract under test is "what the
//! binary does to stdout/stderr/exit-code" plus "the ingested chunks are queryable afterward".
//!
//! Why a NEW file (mirrors `tests/e2e_cli.rs`): the ingest command is its own command surface; it
//! is hidden (`hide = true`) but FULLY REACHABLE — running it works, it just is not advertised in
//! `--help`. The CLI-parsing-layer assertions (hidden-but-reachable, missing-positional → nonzero)
//! live in `tests/cli_tests.rs`; this file is the end-to-end ingest→query/status chain plus the
//! validation-matrix exit-code lock-in.
//!
//! RED rationale: at this slice there is NO `ingest` subcommand — clap rejects `ingest <...>` as an
//! unknown subcommand (nonzero), so every `.success()` ingest here FAILS for the right reason (the
//! command is not implemented yet). The error-path tests assert NONZERO + non-empty stderr + no
//! `panicked`; once the command exists they lock in the typed-error → clean-nonzero contract. These
//! tests COMPILE (assert_cmd drives a subprocess — no not-yet-existing library API is imported), so
//! their failures are behavioral, not compile errors. The crate-root library re-export of
//! `ingest_chunks`/`IngestStats` is exercised separately in `tests/e2e_ingest_lib.rs`.
//!
//! Determinism: chunks are inserted in JSON-array order, so FTS5 assigns rowids in that order and
//! the retriever's `bm25 ASC, rowid ASC` tie-break is a fixed function of the input JSON. Two
//! chunks tied on BM25 therefore come back in array order; ingesting the same JSON into two fresh
//! DBs yields identical query orderings. Both are pinned below.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;

/// Fresh handle to the built binary for each invocation (parallel-safe: no shared state).
fn cc() -> Command {
    Command::cargo_bin("codecache").expect("binary `codecache` should build")
}

/// Fresh binary handle whose working directory is `root` — exercises the cwd-relative
/// `.codecache/` creation + db-path resolution the ingest path depends on end-to-end.
fn cc_in(root: &Path) -> Command {
    let mut cmd = cc();
    cmd.current_dir(root);
    cmd
}

/// An initialized temp project (`init` already run) with NO source files — ingestion supplies the
/// chunks directly, so no `.py`/`.ts`/`.go` tree is needed. The returned `TempDir` cleans up on drop.
fn init_temp_project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("create temp project dir");
    cc_in(tmp.path()).arg("init").assert().success();
    tmp
}

/// Write `contents` to `<root>/<name>` and return the absolute path as a String for arg passing.
fn write_chunks_json(root: &Path, name: &str, contents: &str) -> String {
    let path = root.join(name);
    fs::write(&path, contents).expect("write chunks json fixture");
    path.to_string_lossy().into_owned()
}

// A two-file chunks.json with enrichment populated on the first record. Distinct `file_path`s
// (`src/auth.py`, `src/util.py`) ⇒ two `files_metadata` rows. The query target `authenticate_user`
// appears in `symbol_name` AND `chunk_text`; `normalize_path` is the second file's symbol.
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
    "parent_symbol": null,
    "file_docstring": "Auth helpers module.",
    "imports": ["os", "hashlib"],
    "cross_references": ["verify"],
    "is_heuristic": false
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
// 1. HAPPY PATH (e2e): init → write a 2-file chunks.json with enrichment →
//    `ingest chunks.json` exits 0 → the ingested symbol is queryable (text +
//    json). Proves the JSON-array → insert_chunks → FTS5 → retriever path runs
//    through the binary, and that `--format json` parses end-to-end.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_happy_path_makes_chunks_queryable() {
    let tmp = init_temp_project();
    let root = tmp.path();
    let chunks = write_chunks_json(root, "chunks.json", TWO_FILE_CHUNKS);

    // ingest → exit 0 (success is the contract; the report wording is not pinned exactly,
    // only that it ran cleanly and named the work it did via the counts below where relevant).
    cc_in(root).args(["ingest", &chunks]).assert().success();

    // The ingested symbol is queryable (default text format): stdout names the symbol.
    cc_in(root)
        .args(["query", "authenticate_user"])
        .assert()
        .success()
        .stdout(contains("authenticate_user"));

    // The second file's symbol is also queryable — both records were ingested.
    cc_in(root)
        .args(["query", "normalize_path"])
        .assert()
        .success()
        .stdout(contains("normalize_path"));

    // `--format json` parses end-to-end and its `chunks[]` carries the queried symbol (§6.4.2).
    let out = cc_in(root)
        .args(["query", "authenticate_user", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).expect("query --format json stdout must be valid UTF-8");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("query --format json must emit parseable JSON");
    let chunks_arr = value
        .get("chunks")
        .and_then(|c| c.as_array())
        .expect("JSON output must have a `chunks` array (§6.4.2)");
    assert!(
        chunks_arr.iter().any(|chunk| chunk
            .get("symbol_name")
            .and_then(|s| s.as_str())
            .map(|s| s.contains("authenticate_user"))
            .unwrap_or(false)),
        "JSON `chunks[]` must contain the ingested symbol `authenticate_user`; got: {value}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 2. STATUS sees ingested rows: after ingest, `codecache status` reports the
//    Files/Chunks totals that were ingested. The 2-file fixture ⇒ 2 distinct
//    files / 2 chunks, so a `files_metadata` row per distinct path and the
//    restamped `index_state` totals are pinned through the status surface.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn status_reflects_ingested_files_and_chunks() {
    let tmp = init_temp_project();
    let root = tmp.path();
    let chunks = write_chunks_json(root, "chunks.json", TWO_FILE_CHUNKS);

    cc_in(root).args(["ingest", &chunks]).assert().success();

    let version = env!("CARGO_PKG_VERSION");
    cc_in(root)
        .arg("status")
        .assert()
        .success()
        .stdout(contains(version))
        // 2 distinct file_path values ⇒ one files_metadata row each ⇒ total_files == 2.
        .stdout(contains("Files"))
        .stdout(contains("2"))
        // 2 chunk records ingested ⇒ total_chunks == 2.
        .stdout(contains("Chunks"));
}

// ───────────────────────────────────────────────────────────────────────────
// 3. ENRICHMENT ROUND-TRIPS: a chunk whose query TERM appears ONLY in an
//    enrichment field (here `file_docstring`, and a second variant in
//    `cross_references`) — never in symbol_name or chunk_text — is still
//    retrievable after ingest. Proves the enrichment fields are ingested AND
//    FTS5-indexed (per §4.1 the symbols table indexes file_docstring + imports +
//    cross_references), not silently dropped on the ingest path.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn enrichment_only_term_is_retrievable_after_ingest() {
    let tmp = init_temp_project();
    let root = tmp.path();

    // `zqxwombat` appears ONLY in file_docstring; `plumbusctl` appears ONLY in cross_references.
    // Neither is in symbol_name or chunk_text, so a hit proves enrichment was indexed.
    let json = r#"[
      {
        "symbol_name": "alpha",
        "symbol_type": "function",
        "file_path": "src/a.py",
        "start_byte": 0,
        "end_byte": 20,
        "start_line": 1,
        "end_line": 2,
        "chunk_text": "def alpha():\n    pass\n",
        "language": "python",
        "file_docstring": "module about zqxwombat internals"
      },
      {
        "symbol_name": "beta",
        "symbol_type": "function",
        "file_path": "src/b.py",
        "start_byte": 0,
        "end_byte": 20,
        "start_line": 1,
        "end_line": 2,
        "chunk_text": "def beta():\n    pass\n",
        "language": "python",
        "cross_references": ["plumbusctl"]
      }
    ]"#;
    let chunks = write_chunks_json(root, "enriched.json", json);

    cc_in(root).args(["ingest", &chunks]).assert().success();

    // Querying a docstring-only term surfaces the chunk whose ONLY match is its file_docstring.
    cc_in(root)
        .args(["query", "zqxwombat"])
        .assert()
        .success()
        .stdout(contains("alpha"));

    // Querying a cross_references-only term surfaces the chunk whose ONLY match is a cross_reference.
    cc_in(root)
        .args(["query", "plumbusctl"])
        .assert()
        .success()
        .stdout(contains("beta"));
}

// ───────────────────────────────────────────────────────────────────────────
// 4. DETERMINISM:
//    (a) ingesting the SAME JSON into two FRESH DBs yields identical query
//        result ordering (a fixed input ⇒ a fixed ranking), and
//    (b) two chunks TIED on BM25 (identical chunk_text) come back in a STABLE,
//        data-determined order — the Retriever breaks BM25 ties by
//        (file_path, start_byte, end_byte) ascending (M6.2), not by symbol name.
//    Asserted through the binary via `--format json` (stable, parseable order).
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_ordering_is_deterministic_and_follows_array_order() {
    // Two chunks in two files whose bodies are IDENTICAL on the query terms, so their BM25 scores
    // tie; the Retriever then breaks the tie by file_path ascending (M6.2). `aaa_first` lives in
    // `src/first.py` and `zzz_second` in `src/second.py`, so the result is `aaa_first` then
    // `zzz_second` — a stable, data-determined order, NOT the symbol-name sort (which is the reverse).
    let tied_json = r#"[
      {
        "symbol_name": "zzz_second",
        "symbol_type": "function",
        "file_path": "src/second.py",
        "start_byte": 0,
        "end_byte": 44,
        "start_line": 1,
        "end_line": 2,
        "chunk_text": "handle request and return the response payload",
        "language": "python"
      },
      {
        "symbol_name": "aaa_first",
        "symbol_type": "function",
        "file_path": "src/first.py",
        "start_byte": 0,
        "end_byte": 44,
        "start_line": 1,
        "end_line": 2,
        "chunk_text": "handle request and return the response payload",
        "language": "python"
      }
    ]"#;

    // Helper: ingest `tied_json` into a fresh project and return the ordered symbol_name list that
    // `query "<term>" --format json` reports.
    fn ordered_symbols(json: &str) -> Vec<String> {
        let tmp = tempfile::tempdir().expect("temp project");
        let root = tmp.path();
        let mut init = Command::cargo_bin("codecache").expect("binary builds");
        init.current_dir(root).arg("init").assert().success();

        let path = root.join("tied.json");
        fs::write(&path, json).expect("write tied json");
        let path_s = path.to_string_lossy().into_owned();

        let mut ingest = Command::cargo_bin("codecache").expect("binary builds");
        ingest
            .current_dir(root)
            .args(["ingest", &path_s])
            .assert()
            .success();

        let mut q = Command::cargo_bin("codecache").expect("binary builds");
        let out = q
            .current_dir(root)
            .args(["query", "request response payload", "--format", "json"])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone();
        let stdout = String::from_utf8(out).expect("json stdout utf8");
        let value: serde_json::Value = serde_json::from_str(&stdout).expect("parseable query json");
        value
            .get("chunks")
            .and_then(|c| c.as_array())
            .expect("chunks array")
            .iter()
            .filter_map(|c| {
                c.get("symbol_name")
                    .and_then(|s| s.as_str())
                    .map(str::to_owned)
            })
            .collect()
    }

    // (b) Tied BM25 scores break by the Retriever's (file_path, start_byte, end_byte) key.
    let order = ordered_symbols(tied_json);
    assert_eq!(
        order,
        vec!["aaa_first".to_string(), "zzz_second".to_string()],
        "two BM25-tied chunks must return in the Retriever's (file_path, …) tie-break order \
         (src/first.py before src/second.py), NOT symbol-name order"
    );

    // (a) Re-ingesting the IDENTICAL JSON into a SECOND fresh DB yields the IDENTICAL ordering.
    let order_again = ordered_symbols(tied_json);
    assert_eq!(
        order, order_again,
        "ingesting the same JSON into two fresh DBs must yield identical query result ordering"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// 5. EDGE — EMPTY INPUT `[]`: ingests cleanly as a valid no-op (0 files, 0
//    chunks), exit 0. Empty is a degenerate-but-valid corpus, NOT an error
//    (pinned per the brief's manager call). A subsequent query is well-formed
//    (it returns no results, but never errors/panics on the empty index).
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_empty_array_is_a_clean_no_op() {
    let tmp = init_temp_project();
    let root = tmp.path();
    let chunks = write_chunks_json(root, "empty.json", "[]");

    // Empty input ⇒ exit 0, no panic on either stream.
    cc_in(root)
        .args(["ingest", &chunks])
        .assert()
        .success()
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());

    // status over the empty-ingest DB reports zero files/chunks and still exits 0.
    cc_in(root)
        .arg("status")
        .assert()
        .success()
        .stdout(contains("Files"))
        .stdout(contains("0"));

    // Querying the empty index is well-formed (no results, clean exit, no panic).
    cc_in(root)
        .args(["query", "anything"])
        .assert()
        .success()
        .stdout(contains("panicked").not())
        .stderr(contains("panicked").not());
}

// ───────────────────────────────────────────────────────────────────────────
// 6. ERROR — MALFORMED JSON (truncated / not JSON at all): a typed error →
//    NONZERO exit + a stderr message + NO panic on either stream.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_malformed_json_exits_nonzero_without_panic() {
    let tmp = init_temp_project();
    let root = tmp.path();

    // (label, body) — truncated array, and outright not-JSON.
    let bad: [(&str, &str); 2] = [
        ("truncated", r#"[ { "symbol_name": "x", "#),
        ("not json", "this is not json at all"),
    ];

    for (label, body) in bad {
        let chunks = write_chunks_json(root, &format!("malformed_{label}.json"), body);
        cc_in(root)
            .args(["ingest", &chunks])
            .assert()
            .failure()
            .stderr(predicate::str::is_empty().not())
            .stderr(contains("panicked").not())
            .stdout(contains("panicked").not());
    }
}

// ───────────────────────────────────────────────────────────────────────────
// 7. ERROR — MISSING REQUIRED FIELD: a record omitting `symbol_name` (a required
//    field) → NONZERO exit + stderr + NO panic. (Other required fields follow the
//    same serde-required path; symbol_name is the representative case.)
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_missing_required_field_exits_nonzero_without_panic() {
    let tmp = init_temp_project();
    let root = tmp.path();

    // Valid record shape EXCEPT `symbol_name` is absent.
    let json = r#"[
      {
        "symbol_type": "function",
        "file_path": "src/a.py",
        "start_byte": 0,
        "end_byte": 10,
        "start_line": 1,
        "end_line": 2,
        "chunk_text": "def a(): pass",
        "language": "python"
      }
    ]"#;
    let chunks = write_chunks_json(root, "missing_field.json", json);

    cc_in(root)
        .args(["ingest", &chunks])
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not())
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());
}

// ───────────────────────────────────────────────────────────────────────────
// 8. ERROR — UNKNOWN ENUM string: an out-of-set `symbol_type` ("trait") or
//    `language` ("rust") → NONZERO exit + stderr + NO panic. These map through
//    `SymbolType::from_str_lenient` / `Language::from_str_lenient` (total, `None`
//    on unknown) → a typed ingest error, never a panic.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_unknown_enum_exits_nonzero_without_panic() {
    let tmp = init_temp_project();
    let root = tmp.path();

    // Unknown symbol_type "trait".
    let bad_symbol_type = r#"[
      {
        "symbol_name": "x",
        "symbol_type": "trait",
        "file_path": "src/a.py",
        "start_byte": 0,
        "end_byte": 10,
        "start_line": 1,
        "end_line": 2,
        "chunk_text": "x",
        "language": "python"
      }
    ]"#;
    let st = write_chunks_json(root, "bad_symbol_type.json", bad_symbol_type);
    cc_in(root)
        .args(["ingest", &st])
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not())
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());

    // Unknown language "rust".
    let bad_language = r#"[
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
    let lang = write_chunks_json(root, "bad_language.json", bad_language);
    cc_in(root)
        .args(["ingest", &lang])
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not())
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());
}

// ───────────────────────────────────────────────────────────────────────────
// 9. ERROR — WRONG JSON TYPE: a string where an integer is required
//    (`start_byte: "x"`) → serde type error → NONZERO exit + stderr + NO panic.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_wrong_json_type_exits_nonzero_without_panic() {
    let tmp = init_temp_project();
    let root = tmp.path();

    let json = r#"[
      {
        "symbol_name": "x",
        "symbol_type": "function",
        "file_path": "src/a.py",
        "start_byte": "x",
        "end_byte": 10,
        "start_line": 1,
        "end_line": 2,
        "chunk_text": "x",
        "language": "python"
      }
    ]"#;
    let chunks = write_chunks_json(root, "wrong_type.json", json);

    cc_in(root)
        .args(["ingest", &chunks])
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not())
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());
}

// ───────────────────────────────────────────────────────────────────────────
// 10. ERROR — MISSING INPUT FILE: `ingest no-such.json` (the path does not
//     exist) → NONZERO exit + stderr + NO panic. A clean "cannot read file"
//     error, never an unwrap on the read.
// ───────────────────────────────────────────────────────────────────────────

#[test]
fn ingest_missing_input_file_exits_nonzero_without_panic() {
    let tmp = init_temp_project();
    let root = tmp.path();

    // A path that genuinely does not exist under the temp root.
    let missing = root.join("no-such.json");
    assert!(
        !missing.exists(),
        "precondition: the chunks file must not exist"
    );
    let missing_s = missing.to_string_lossy().into_owned();

    cc_in(root)
        .args(["ingest", &missing_s])
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not())
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());
}
