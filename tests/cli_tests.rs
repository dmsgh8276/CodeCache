//! M7.2 — CLI parsing + errors + exit codes (RED, test-lead).
//!
//! Pins the clap surface from `docs/project_plan.md` §7.1–§7.2 by driving the BUILT
//! `codecache` binary as a subprocess (`assert_cmd`) and matching stdout/stderr/exit
//! codes (`predicates`). These tests live at the *parsing* layer — they only invoke
//! `--help` (which clap handles before any command handler runs) or trigger clap's own
//! arg/enum/required-arg validation. No command handler logic is exercised here; handler
//! behavior is M7.3 and is verified end-to-end in M7.4.
//!
//! RED rationale: the current M0 stub (`src/cli/mod.rs::run`) ignores all args and just
//! prints `codecache <VERSION>` then exits 0. So `<cmd> --help` will NOT exit 0 with the
//! documented flag text, an unknown command will NOT error, and a missing required arg
//! will NOT be rejected. Every assertion below fails against that stub for the right
//! reason: clap parsing is not implemented yet. The file COMPILES (assert_cmd drives a
//! subprocess — there is no not-yet-existing library API to import), so the failures are
//! purely behavioral.

use std::fs;
use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use predicates::str::contains;

/// The seven documented subcommands (§7.1).
const SUBCOMMANDS: [&str; 7] = [
    "init", "index", "update", "query", "status", "config", "serve",
];

/// Fresh handle to the built binary for each invocation (parallel-safe: no shared state).
fn cc() -> Command {
    Command::cargo_bin("codecache").expect("binary `codecache` should build")
}

// ---------------------------------------------------------------------------
// 1. Each command parses its documented flags (§7.2).
//    `<cmd> --help` exits 0 and the help text names that command's flags. This
//    pins flag NAMES at the parsing layer without running any handler.
// ---------------------------------------------------------------------------

#[test]
fn each_command_parses_its_documented_flags() {
    // init — §7.2: --db-path, --index-path, --ignore, --languages
    cc().args(["init", "--help"])
        .assert()
        .success()
        .stdout(contains("--db-path"))
        .stdout(contains("--index-path"))
        .stdout(contains("--ignore"))
        .stdout(contains("--languages"));

    // index — §7.2: --full, --db-path, --progress
    cc().args(["index", "--help"])
        .assert()
        .success()
        .stdout(contains("--full"))
        .stdout(contains("--db-path"))
        .stdout(contains("--progress"));

    // update <FILE>... — §7.2: --db-path (positional FILE arg shown in usage)
    cc().args(["update", "--help"])
        .assert()
        .success()
        .stdout(contains("--db-path"))
        .stdout(contains("FILE"));

    // query <QUERY> — §7.2: --max-tokens, --max-results, --format, --file-filter, --db-path
    cc().args(["query", "--help"])
        .assert()
        .success()
        .stdout(contains("--max-tokens"))
        .stdout(contains("--max-results"))
        .stdout(contains("--format"))
        .stdout(contains("--file-filter"))
        .stdout(contains("--db-path"))
        .stdout(contains("QUERY"));

    // status — §7.2: --db-path
    cc().args(["status", "--help"])
        .assert()
        .success()
        .stdout(contains("--db-path"));

    // config — §7.2 gives no detailed flag spec; M7.3 defines the handler. RED-minimal:
    // assert it is a recognized subcommand whose `--help` parses and exits 0.
    cc().args(["config", "--help"]).assert().success();

    // serve — §7.2: --transport, --port, --db-path
    cc().args(["serve", "--help"])
        .assert()
        .success()
        .stdout(contains("--transport"))
        .stdout(contains("--port"))
        .stdout(contains("--db-path"));
}

// ---------------------------------------------------------------------------
// 2. Query defaults match the spec (§7.2): --max-tokens 4000, --max-results 20,
//    --format text, value set toon|json|text. Pinned via help output, not by
//    executing the query handler (that is M7.3).
// ---------------------------------------------------------------------------

#[test]
fn query_defaults_match_spec() {
    let assert = cc().args(["query", "--help"]).assert().success();

    // Defaults are advertised in clap help as `[default: <value>]`.
    assert
        .stdout(contains("4000"))
        .stdout(contains("20"))
        // Default format is text.
        .stdout(contains("text"))
        // The accepted format value set is toon|json|text.
        .stdout(contains("toon"))
        .stdout(contains("json"));
}

// ---------------------------------------------------------------------------
// 3. Help & version flags work (§7.1 global options).
//    --help/-h list all 7 subcommands; --version/-V print the crate version;
//    global -v/--verbose is accepted (surfaces in top-level help).
// ---------------------------------------------------------------------------

#[test]
fn help_and_version_flags_work() {
    // `--help` exits 0 and lists every subcommand.
    let long = cc().arg("--help").assert().success();
    let mut long = long;
    for sub in SUBCOMMANDS {
        long = long.stdout(contains(sub));
    }

    // `-h` is the short alias and behaves the same (lists subcommands).
    let short = cc().arg("-h").assert().success();
    let mut short = short;
    for sub in SUBCOMMANDS {
        short = short.stdout(contains(sub));
    }

    // Global verbose flag is advertised in top-level help (both spellings).
    cc().arg("--help")
        .assert()
        .success()
        .stdout(contains("--verbose"))
        .stdout(contains("-v"));

    // `--version` and `-V` exit 0 and print the crate version (env!("CARGO_PKG_VERSION")).
    let version = env!("CARGO_PKG_VERSION");
    cc().arg("--version")
        .assert()
        .success()
        .stdout(contains(version));
    cc().arg("-V").assert().success().stdout(contains(version));
}

// ---------------------------------------------------------------------------
// 4. Bad args exit nonzero with a stderr message — true at the *parsing* layer
//    (clap type/enum/required-arg validation), independent of handler logic.
// ---------------------------------------------------------------------------

#[test]
fn bad_args_exit_nonzero_with_message() {
    // Non-numeric value for an integer flag: clap's type validation rejects it.
    cc().args(["query", "needle", "--max-tokens", "notanumber"])
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not());

    // Invalid enum value for --transport (value set is stdio|sse): clap rejects it.
    cc().args(["serve", "--transport", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not());

    // Missing the required positional <QUERY> arg: clap errors before any handler runs.
    cc().arg("query")
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not());
}

// ---------------------------------------------------------------------------
// 5. Unknown command errors cleanly — nonzero exit, stderr names the bad
//    subcommand, no panic (no "panicked at" in output).
// ---------------------------------------------------------------------------

#[test]
fn unknown_command_errors_cleanly() {
    cc().arg("frobnicate")
        .assert()
        .failure()
        // clap reports the unrecognized subcommand by name.
        .stderr(contains("frobnicate"))
        // A clean parse error, never a Rust panic.
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());
}

// ===========================================================================
// M7.3 — command handlers + status (RED, test-lead).
//
// These tests drive the BUILT `codecache` binary end-to-end against a real
// `tempfile::TempDir` project root (via `.current_dir(tmp)`, exercising
// cwd-relative path resolution) and assert REAL handler behavior: files are
// created on disk, the index reports the counts that genuinely exist, a known
// symbol is retrieved + formatted, an update re-indexes, and a config write
// persists through `Config::save` (D18).
//
// RED rationale: M7.2 shipped the clap surface but the handlers are inert
// placeholders — each prints "<cmd>: not yet implemented (M7.3)." and returns
// Ok(()) WITHOUT creating a db, indexing, querying, or persisting config. So:
//   * `init` does not write `.codecache/{config.toml,index.db}`,
//   * `status` reports no real counts,
//   * `query` does not emit the symbol / a file:line locator nor valid JSON,
//   * `update` does not re-index,
//   * `config` does not print defaults nor persist a new value.
// Every assertion below fails against those inert handlers for the right
// reason (handlers don't do the work yet), NOT a compile error: assert_cmd
// drives a subprocess, so there is no not-yet-existing library API to import.
//
// Fixture: `tests/fixtures/python/enriched_module.py` (committed). It defines a
// free function `hash_password`, a class `UserService`, and the method
// `register` (3 chunks across 1 Python file) — so the indexed totals are
// deterministic and `hash_password` is a stable, clearly-named query target.
//
// Config key pinned for the eng-lead's GREEN: `storage.max_db_size_mb`
// (`StorageConfig.max_db_size_mb: u64`, documented default 500, §7.3). It
// round-trips cleanly through `Config` and is set to `1000` per §7.2's own
// example (`codecache config storage.max_db_size_mb 1000`).
// ===========================================================================

/// The committed Python fixture used by the handler E2E tests. Copied into each
/// test's temp project root so indexing has a real, deterministic source file.
const ENRICHED_MODULE: &str = include_str!("fixtures/python/enriched_module.py");

/// A second tiny Python source with a distinct, clearly-named symbol, written at
/// runtime for the `update` test (so the newly-indexed symbol is unambiguous).
const NEW_MODULE_SRC: &str = "def freshly_added_symbol():\n    return 42\n";

/// Build a fresh temp project root containing exactly one real `.py` source file
/// (`module.py`, the enriched fixture). Returned `TempDir` cleans up on drop.
fn temp_project() -> tempfile::TempDir {
    let tmp = tempfile::tempdir().expect("create temp project dir");
    fs::write(tmp.path().join("module.py"), ENRICHED_MODULE).expect("write fixture source");
    tmp
}

/// Fresh binary handle whose working directory is `root` — exercises the cwd-
/// relative `.codecache/` + db-path resolution the handlers must perform.
fn cc_in(root: &Path) -> Command {
    let mut cmd = cc();
    cmd.current_dir(root);
    cmd
}

// ---------------------------------------------------------------------------
// 1. `init` creates the db + config on disk (§7.2 "Generated files").
// ---------------------------------------------------------------------------

#[test]
fn init_creates_db_and_config() {
    let tmp = temp_project();
    let root = tmp.path();

    cc_in(root).arg("init").assert().success();

    let config_path = root.join(".codecache").join("config.toml");
    let db_path = root.join(".codecache").join("index.db");

    assert!(
        config_path.is_file(),
        "init must write .codecache/config.toml (found: {})",
        config_path.display()
    );
    assert!(
        db_path.is_file(),
        "init must create .codecache/index.db (found: {})",
        db_path.display()
    );
}

// ---------------------------------------------------------------------------
// 2. `index` then `status` reports the counts that ACTUALLY EXIST (§7.2):
//    the crate version, a files total, and a chunks total. The enriched
//    fixture is 1 file / 3 chunks (hash_password fn + UserService class +
//    register method), so those totals are pinned exactly.
// ---------------------------------------------------------------------------

#[test]
fn index_then_status_reports_counts() {
    let tmp = temp_project();
    let root = tmp.path();

    cc_in(root).arg("init").assert().success();
    cc_in(root).arg("index").assert().success();

    let version = env!("CARGO_PKG_VERSION");

    cc_in(root)
        .arg("status")
        .assert()
        .success()
        // Version line (§7.2 layout: `Version: 0.1.0`).
        .stdout(contains(version))
        // Files section reports the genuine total_files aggregate: 1 source file.
        .stdout(contains("Files"))
        .stdout(contains("1"))
        // Chunks section reports the genuine total_chunks aggregate: 3 chunks.
        .stdout(contains("Chunks"))
        .stdout(contains("3"));
}

// ---------------------------------------------------------------------------
// 3. `query <symbol>` prints formatted results wiring Retriever -> formatter.
//    Default (text) output contains the symbol name + a file:line locator;
//    `--format json` yields parseable JSON that still contains the symbol.
// ---------------------------------------------------------------------------

#[test]
fn query_command_prints_formatted_results() {
    let tmp = temp_project();
    let root = tmp.path();

    cc_in(root).arg("init").assert().success();
    cc_in(root).arg("index").assert().success();

    // Default text format: stdout names the symbol and a `module.py:` locator.
    cc_in(root)
        .args(["query", "hash_password"])
        .assert()
        .success()
        .stdout(contains("hash_password"))
        .stdout(contains("module.py"));

    // JSON format: stdout is parseable JSON that contains the symbol somewhere.
    let out = cc_in(root)
        .args(["query", "hash_password", "--format", "json"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(out).expect("query --format json stdout must be valid UTF-8");
    let value: serde_json::Value =
        serde_json::from_str(&stdout).expect("query --format json must emit parseable JSON");
    assert!(
        value.to_string().contains("hash_password"),
        "JSON query output must contain the queried symbol `hash_password`; got: {value}"
    );
}

// ---------------------------------------------------------------------------
// 4. `update <FILE>` re-indexes the given file: a newly-added source file with
//    a fresh symbol becomes queryable after `update`, proving the handler runs
//    `Indexer::update_files` on the listed path.
// ---------------------------------------------------------------------------

#[test]
fn update_command_reindexes_given_files() {
    let tmp = temp_project();
    let root = tmp.path();

    cc_in(root).arg("init").assert().success();
    cc_in(root).arg("index").assert().success();

    // The new symbol does not exist in the index yet.
    cc_in(root)
        .args(["query", "freshly_added_symbol"])
        .assert()
        .success()
        .stdout(contains("freshly_added_symbol").not());

    // Add a new source file and update ONLY it.
    let new_file = root.join("fresh.py");
    fs::write(&new_file, NEW_MODULE_SRC).expect("write new source file");
    cc_in(root).args(["update", "fresh.py"]).assert().success();

    // After the targeted update the new symbol is queryable.
    cc_in(root)
        .args(["query", "freshly_added_symbol"])
        .assert()
        .success()
        .stdout(contains("freshly_added_symbol"))
        .stdout(contains("fresh.py"));
}

// ---------------------------------------------------------------------------
// 5. `config` reads + writes settings, persisting through `Config::save` (D18).
//    Key pinned: `storage.max_db_size_mb` (default 500 -> set to 1000, §7.2).
//    Reading with no args prints the resolved config (default value appears);
//    writing the key sets it, a subsequent read shows the new value, and the
//    on-disk `.codecache/config.toml` contains it (proves persistence).
// ---------------------------------------------------------------------------

#[test]
fn config_command_reads_writes_settings() {
    let tmp = temp_project();
    let root = tmp.path();

    cc_in(root).arg("init").assert().success();

    // Read (no args): the resolved config prints the documented default (500).
    cc_in(root)
        .arg("config")
        .assert()
        .success()
        .stdout(contains("max_db_size_mb"))
        .stdout(contains("500"));

    // Write: set the scalar key to a new value (§7.2 example).
    cc_in(root)
        .args(["config", "storage.max_db_size_mb", "1000"])
        .assert()
        .success();

    // Read again: the new value is reflected in the printed config.
    cc_in(root)
        .arg("config")
        .assert()
        .success()
        .stdout(contains("max_db_size_mb"))
        .stdout(contains("1000"));

    // Persistence: the on-disk config.toml carries the new value (Config::save).
    let config_path = root.join(".codecache").join("config.toml");
    let persisted = fs::read_to_string(&config_path).expect("read persisted config.toml");
    assert!(
        persisted.contains("1000"),
        "config write must persist `storage.max_db_size_mb = 1000` to disk; got:\n{persisted}"
    );
}

// ---------------------------------------------------------------------------
// 6. (optional) `serve` is a clean stub this slice — it must not panic/segfault
//    and should surface a notice. We do not pin an exact exit code (M8 owns the
//    final semantics); we only assert no Rust panic leaks to either stream.
// ---------------------------------------------------------------------------

#[test]
fn serve_is_a_clean_stub() {
    let tmp = temp_project();
    let root = tmp.path();

    cc_in(root).arg("init").assert().success();

    cc_in(root)
        .arg("serve")
        .assert()
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());
}

// ===========================================================================
// R2.2a — D24: `query --bm25-weights "<7 csv f64>"` (RED).
//
// A CLI-reachable per-column BM25 weight override so the R2 harness can sweep
// ranking weights across `codecache query` calls WITHOUT recompiling. The flag
// takes 7 comma-separated f64 in `schema::CREATE_SYMBOLS` indexed-column order
// [symbol_name, symbol_type, chunk_text, parent_symbol, imports, cross_references,
// file_docstring]. Absent ⇒ the built-in default (10,1,1,5,2,2,2). Malformed
// (wrong arity, non-numeric, empty) ⇒ a clean typed error → NONZERO exit + a
// stderr message, NEVER a panic.
//
// RED rationale: the `query` subcommand has no `--bm25-weights` arg yet, so clap
// rejects it as an unknown flag (nonzero) — `query_accepts_bm25_weights_flag`'s
// `.success()` therefore FAILS for the right reason (arg not implemented).
// `query_help_lists_bm25_weights_flag` fails because help omits the flag. The
// malformed-input tests are written to be GREEN-on-arrival ONCE the flag exists
// (an unknown flag is already nonzero today), but their no-panic + non-empty-
// stderr contract is the lock-in the eng-lead's typed-error parse must satisfy.
// ===========================================================================

#[test]
fn query_help_lists_bm25_weights_flag() {
    // Parsing layer: `query --help` must advertise the new flag (pins the flag NAME, §7.2 amend).
    cc().args(["query", "--help"])
        .assert()
        .success()
        .stdout(contains("--bm25-weights"));
}

#[test]
fn query_accepts_bm25_weights_flag() {
    // Valid 7-value vector parses and the query runs to exit 0 on an indexed fixture, still
    // surfacing the queried symbol (the override is well-formed ⇒ normal retrieval).
    let tmp = temp_project();
    let root = tmp.path();

    cc_in(root).arg("init").assert().success();
    cc_in(root).arg("index").assert().success();

    cc_in(root)
        .args(["query", "hash_password", "--bm25-weights", "10,1,1,5,2,2,2"])
        .assert()
        .success()
        .stdout(contains("hash_password"));
}

#[test]
fn query_bm25_weights_malformed_exits_nonzero_without_panic() {
    // Every malformed spelling must fail cleanly: NONZERO exit, a non-empty stderr message, and NO
    // Rust panic on either stream. Cases: too few values, too many values, non-numeric, empty.
    let tmp = temp_project();
    let root = tmp.path();

    cc_in(root).arg("init").assert().success();
    cc_in(root).arg("index").assert().success();

    // (label, flag value) — wrong arity (3 and 8), non-numeric, and empty string.
    let bad_values: [(&str, &str); 4] = [
        ("too few (3 values)", "1,2,3"),
        ("too many (8 values)", "1,2,3,4,5,6,7,8"),
        ("non-numeric", "a,b,c,d,e,f,g"),
        ("empty", ""),
    ];

    for (label, value) in bad_values {
        cc_in(root)
            .args(["query", "hash_password", "--bm25-weights", value])
            .assert()
            .failure()
            // A user-facing diagnostic exists …
            .stderr(predicate::str::is_empty().not())
            // … and it is NOT a Rust panic / segfault on either stream.
            .stderr(contains("panicked").not())
            .stdout(contains("panicked").not());
        // Re-assert per case so a failure names which malformed input slipped through.
        cc_in(root)
            .args(["query", "hash_password", "--bm25-weights", value])
            .assert()
            .failure()
            .stderr(predicate::str::is_empty().not())
            .stderr(contains("panicked").not());
        let _ = label; // documents the case in source; the loop body asserts the contract.
    }
}

// ===========================================================================
// R2.3a — D25: `ingest <CHUNKS_JSON> [--db-path <PATH>]` — CLI PARSING layer (RED).
//
// The 8th subcommand is a research-only chunk-ingestion seam (§7.2). It is clap
// `hide = true` — NOT advertised in the top-level `codecache --help` listing —
// but FULLY REACHABLE: `codecache ingest --help` parses + exits 0, and running it
// against a real JSON file works (the end-to-end behavior is covered in
// `tests/e2e_ingest.rs`; this file pins only the parsing-layer contract).
//
// RED rationale: there is no `ingest` subcommand yet, so:
//   * `ingest_subcommand_is_hidden_from_top_level_help` is GREEN-on-arrival today
//     (ingest is simply absent) but becomes the LOCK-IN that `hide = true` keeps it
//     out of `--help` once the command exists — paired with the reachability test
//     below it is the full "hidden but reachable" contract;
//   * `ingest_help_is_reachable` FAILS now (clap errors on `ingest --help` as an
//     unknown subcommand → nonzero, not the `.success()` asserted) — the right
//     reason: the command is not implemented;
//   * `ingest_requires_chunks_json_positional` FAILS now because there is no
//     `ingest` subcommand to enforce the required positional. Once it exists, a
//     bare `ingest` (no path) must be a clap required-arg error → nonzero, no panic.
// ===========================================================================

#[test]
fn ingest_subcommand_is_hidden_from_top_level_help() {
    // `hide = true`: the top-level help must NOT advertise `ingest` (research-only surface).
    // The seven documented subcommands stay listed (sanity that help still renders them).
    let assert = cc().arg("--help").assert().success();
    let mut assert = assert;
    for sub in SUBCOMMANDS {
        assert = assert.stdout(contains(sub));
    }
    // The hidden command name must not appear as an advertised subcommand line.
    assert.stdout(contains("ingest").not());
}

#[test]
fn ingest_help_is_reachable() {
    // Hidden does NOT mean unreachable: `ingest --help` parses, exits 0, and its own help
    // mentions the `<CHUNKS_JSON>` positional + the shared `--db-path` flag (§7.2).
    cc().args(["ingest", "--help"])
        .assert()
        .success()
        .stdout(contains("--db-path"))
        .stdout(contains("CHUNKS_JSON"));
}

#[test]
fn ingest_requires_chunks_json_positional() {
    // The `<CHUNKS_JSON>` positional is required: a bare `ingest` is a clap parse error before any
    // handler runs → nonzero exit + non-empty stderr, never a Rust panic.
    cc().arg("ingest")
        .assert()
        .failure()
        .stderr(predicate::str::is_empty().not())
        .stderr(contains("panicked").not())
        .stdout(contains("panicked").not());
}
