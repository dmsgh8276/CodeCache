# BRIEF — R2.3a / crate chunk-ingestion seam

- **Milestone:** R2.3 (research track — chunker ablation) · slice **R2.3a** · **Module(s):** `cli`, `app` (+ reuse `storage`)
- **Owner (manager):** principal-engineering-manager  ·  **Created:** 2026-06-14
- **Status:** RED ▣  GREEN ▢  REVIEW ▢  DONE ▢  (RED captured 2026-06-14 — see RED section)
- **Links:** docs/ROADMAP.md#R2 (D23, **new D25**) · docs/TODO.md (R2.3 row) · docs/TEST_STRATEGY.md#indexer / #cli
- **Decision:** **D25 — Adopted (user-ratified via spike→decision)**. NOT proposed.

## Goal
Add a **CLI-reachable chunk-ingestion path** that reads a JSON file of caller-supplied chunk records and
inserts them straight into storage (bypassing discover→parse→chunk), so the R2 research harness can do
`init` (fresh DB) → **ingest `<chunks.json>`** → `query` per chunker arm. Any external chunker's output
(an in-harness stub now at R2.3b; astchunk/cAST at the gated R2.6) then flows through CodeCache's **same
storage + FTS5-BM25 + retriever**, isolating the chunker as the only variable in the ablation.

## Why (research framing — D23/D25)
The chunker is an index-time **hardcoded free fn** (`chunker::chunk(tree, source, lang)`), not a trait,
not swappable; the research harness is **process-boundary-only** (shells to the `codecache` binary; no
FFI/PyO3). To ablate the chunker we need a seam reachable from the CLI that accepts *pre-chunked* input.
`Storage::insert_chunks(&[Chunk])` already exists (single-transaction batch) — the ingestion path reuses
it verbatim. This is the **one** R2.3 crate touch; R2.3b (the stub chunker + A/B plumbing) is a pure
`research/` follow-on owned by research-harness-engineer.

## Scope (in / out)
- **In:**
  - A new **`ingest <CHUNKS_JSON>`** CLI subcommand (8th command), **`hide = true`** (research/undocumented;
    reachable but not advertised in `--help`). `--db-path` like the others.
  - A **format-local input DTO** (`IngestChunk` / `IngestFile` envelope) in the ingesting module that
    deserializes the JSON and maps to `types::Chunk` (serde stays OFF `types::Chunk` — D4/D5).
  - An `app::ingest_chunks(project_root, chunks_path) -> Result<IngestStats, AppError>` facade
    (mirrors `app::index`): open storage at resolved db_path → parse+validate JSON → `insert_chunks` in
    **JSON-array order** → write one `files_metadata` row per distinct `file_path` → restamp
    `total_files`/`total_chunks`. Returns a small stats struct (files, chunks).
  - **No-panic validation**: malformed JSON / missing required field / unknown enum string / empty input /
    wrong JSON types → typed error → clean **nonzero exit** (the R2.2a pattern).
- **Out (defer):**
  - The harness-side stub chunker + A/B sweep plumbing → **R2.3b** (pure `research/`, research-harness-engineer).
  - astchunk/cAST baseline → **R2.6** (gated).
  - **Incremental / idempotency / re-ingest semantics** — the harness `init`s a **fresh DB per arm**, so
    ingestion targets an empty DB. Out of scope; noted as a non-goal. (If a `file_path` repeats *within one
    JSON*, rows are inserted in array order — no dedup; that mirrors `insert_chunks`.)
  - Persisting `is_heuristic` (still the deferred M5/M7 seam — schema has no column; round-trip reconstructs `false`).
  - Any change to `index`/`query`/`update`/`status`/`config`/`serve` behavior (additive only).

## Design decisions (manager — resolved test-first; forks flagged for the user)

### CLI surface — **`ingest <CHUNKS_JSON>` (new hidden subcommand)**, NOT `index --from-chunks`
`index` semantically means *discover → parse → chunk → store* and is covered by a mature test suite;
overloading it with a `--from-chunks` mode that skips all three muddies a clean command and risks
regressing its contract. A separate **`ingest`** subcommand keeps the production surface honest and the
new operation independently testable. Marked **`hide = true`** so `codecache --help` does not advertise a
research-only path to end users (the user accepted "likely hidden/research"). **FORK for the user:**
confirm hidden (`hide = true`) vs a normally-documented command. Default taken: **hidden**.

### Where the code lives — `cli/ingest.rs` (DTO + handler) + `app::ingest_chunks` (facade)
Mirrors the existing split: `cli/index.rs` (thin handler) → `app::index` (facade). The **DTO is
format-local in `cli/ingest.rs`**, exactly as `formatter::json::JsonChunk` is format-local in
`src/formatter/json.rs` — `serde` derives live on the DTO, never on `types::Chunk` (D4/D5). The handler
reads the file + deserializes; the facade does storage. (Eng-lead may instead place the DTO beside the
facade in `app` if that reads cleaner — the **constraint** is only: format-local DTO, not on `types::Chunk`.)

### Input DTO + JSON schema (fuller than the query-output JSON — deliberately)
The query-output `JsonChunk` (`formatter/json.rs`) is **LOSSY** (omits `start_line`/`end_line`,
`parent_symbol`, `file_docstring`, `imports`, `cross_references`, `is_heuristic`). Ingestion must let the
harness control **every** field that affects retrieval — including enrichment, because R2.3b holds
enrichment **constant** to isolate chunking. So the schema is a **fuller input DTO**, not the query shape.

Top-level JSON = an **array of chunk records** (array order = insertion/rowid order). Each record:

| field | JSON type | required? | maps to `Chunk` field | notes |
|---|---|---|---|---|
| `symbol_name` | string | **required** | `symbol_name` | |
| `symbol_type` | string | **required** | `symbol_type` | one of `function`/`class`/`method`/`struct` via `SymbolType::from_str_lenient`; unknown → error |
| `file_path` | string | **required** | `file_path` | path as stored; one `files_metadata` row written per distinct value |
| `start_byte` | integer ≥0 | **required** | `start_byte` | |
| `end_byte` | integer ≥0 | **required** | `end_byte` | |
| `start_line` | integer ≥0 | **required** | `start_line` | 1-based inclusive (D7) |
| `end_line` | integer ≥0 | **required** | `end_line` | 1-based inclusive (D7) |
| `chunk_text` | string | **required** | `chunk_text` | the body BM25 indexes |
| `language` | string | **required** | `language` | one of `python`/`typescript`/`go` via `Language::from_str_lenient`; unknown → error |
| `parent_symbol` | string \| null | optional (default `null`) | `parent_symbol` | |
| `file_docstring` | string \| null | optional (default `null`) | `file_docstring` | |
| `imports` | string[] | optional (default `[]`) | `imports` | |
| `cross_references` | string[] | optional (default `[]`) | `cross_references` | |
| `is_heuristic` | bool | optional (default `false`) | `is_heuristic` | passed in-memory; storage drops it (no column — known seam) |

Enum strings use the **existing** `SymbolType::from_str_lenient` / `Language::from_str_lenient` (total,
no-panic, `None` on unknown). Optional fields use `#[serde(default)]`. Unknown/extra JSON keys: eng-lead's
call — **lenient (ignore) is acceptable** and simplest for a research seam (note it either way).

### `files_metadata` + `index_state` totals
Ingestion bypasses file hashing, but `status` and `codecache_outline` read `files_metadata`, so write a
row per **distinct** `file_path` so those surfaces work on an ingested DB. Per-file `FileMeta`:
- `content_hash`: a fresh DB per arm makes the hash semantically irrelevant — but the column is
  `NOT NULL`. Use a deterministic sentinel (e.g. `"ingested"` or `""`) — eng-lead picks; pin it in a test
  if a test observes it. (Re-ingest/incremental is out of scope, so the hash need not be a real content hash.)
- `mtime` `0`, `file_size` `0` (or `chunk_text` byte sum — eng-lead's call; not load-bearing),
  `language` = that file's chunk language, `chunk_count` = number of ingested chunks for that path.
Then restamp `total_files`/`total_chunks`. **Reuse**: `Indexer::restamp_index_state` is private to
`indexer`; rather than widen the indexer, the facade can either (a) compute totals from `files_metadata`
via the existing `Storage::all_indexed_files` + `Storage::get_file_meta` (same logic, no new storage API),
or (b) sum locally during ingestion. **(b) is simplest and deterministic** — prefer it unless a test wants
the recompute path. `Storage::update_file_hash` (D6 upsert) writes the rows; `Storage::set_index_state`
writes the totals. **No new `Storage` method should be required** — flag to manager if you find one is.

### Determinism
Insert chunks in **JSON-array order**; FTS5 assigns rowids in insert order, and the retriever's
`bm25 ASC, rowid ASC` tie-break then depends on that order — so a fixed input JSON yields a fixed ranking.
A RED test pins this.

## Scenarios to cover (from TEST_STRATEGY — #indexer / #cli; new e2e file)
RED should live as **e2e through the built binary** (mirrors `tests/e2e_cli.rs`, assert_cmd + TempDir) for
the command surface, plus a focused unit/integration test for the DTO→Chunk mapping and validation matrix.

- [ ] **happy path (e2e):** `init` a temp project → write a `chunks.json` (≥2 files, enrichment populated) →
      `codecache ingest chunks.json` exits 0 and reports files/chunks → `codecache query "<term in a chunk>"`
      returns the expected symbol; `--format json` parses end-to-end.
- [ ] **status/outline see ingested rows:** after ingest, `codecache status` shows the right Files/Chunks;
      (if cheap) an `mcp_server` outline or a `Storage::symbols_for_path` assertion sees the ingested symbols.
- [ ] **enrichment round-trips:** a chunk whose query term appears **only** in `file_docstring` (or
      `cross_references`) is retrievable — proves enrichment fields are ingested + indexed (not dropped).
- [ ] **determinism:** ingesting the same JSON twice into two fresh DBs yields identical query ordering;
      and array order drives the rowid tie-break (two chunks tied on BM25 return in array order).
- [ ] **edge — empty input:** `[]` ingests cleanly (0 files, 0 chunks), exits 0 (empty is valid, not an error)
      — OR, if the team prefers, a typed "empty input" nonzero; **manager's call: treat `[]` as success/no-op**
      (it is a valid degenerate corpus). Pin whichever is chosen.
- [ ] **error — malformed JSON:** not-JSON / truncated → typed error → **nonzero exit**, no panic, stderr message.
- [ ] **error — missing required field:** a record missing `symbol_name` (or any required) → nonzero, no panic.
- [ ] **error — unknown enum:** `symbol_type: "trait"` or `language: "rust"` → nonzero (from_str_lenient None), no panic.
- [ ] **error — wrong types:** `start_byte: "x"` (string where integer) → serde error → nonzero, no panic.
- [ ] **error — missing file:** `ingest does-not-exist.json` → nonzero, no panic.

## Definition of Done
- [ ] Tests written first, now green · `cargo clippy --all-targets -- -D warnings` clean · `cargo fmt --all -- --check` clean
- [ ] **`Cargo.toml` UNTOUCHED** (serde + serde_json already deps; no new dep)
- [ ] API matches the amended `project_plan.md` §7 (ingest command) + §3.2.4 (`app::ingest_chunks`) /
      §3.2.2 note; **D25 recorded Adopted** in ROADMAP — *spec amended BEFORE code*
- [ ] No reachable `unwrap()/expect()/panic!`; typed errors + `?`; additive only (index/query/etc. + all
      existing tests stay green); deterministic insertion order preserved
- [ ] reviewer APPROVED
- [ ] `docs/TODO.md` (R2.3 row) + touched module `CLAUDE.md` (cli + app/indexer; storage only if it gains a
      method — it should not) updated in the same change
- [ ] **`.claude/settings.json` left untouched and OUT of staging**; gates run EXPLICITLY (hooks are OFF)

---
## RED — test lead
**Status: RED captured 2026-06-14.** Tests written first; production code does not exist. Three test
files (one new library file, one new binary-e2e file, plus an append to `cli_tests.rs`). RED is split
between a **compile error** (the new-symbol library contract) and **behavioral failures** (the binary
e2e + cli-parsing layer). `Cargo.toml` UNTOUCHED. No existing test weakened/deleted; full pre-existing
suite re-run green (see below).

### Files added / touched (tests only)
1. **`C:\Users\ehlee\workspace\projects\CodeCache\tests\e2e_ingest.rs`** (NEW) — binary e2e through
   `assert_cmd::Command::cargo_bin("codecache")` + `TempDir` + `predicates`, mirrors `tests/e2e_cli.rs`.
   The **bulk of coverage**; independent of the library re-export path. 10 tests.
2. **`C:\Users\ehlee\workspace\projects\CodeCache\tests\e2e_ingest_lib.rs`** (NEW) — library surface;
   imports `codecache::{init, ingest_chunks, IngestStats}`. 3 tests. **This file is the compile-RED.**
3. **`C:\Users\ehlee\workspace\projects\CodeCache\tests\cli_tests.rs`** (APPENDED) — 3 CLI-parsing-layer
   tests for the hidden-but-reachable `ingest` subcommand (kept in the M7 parsing file by convention).

### Test → scenario map (brief "Scenarios to cover")
**`e2e_ingest.rs` (binary e2e):**
| test | scenario | RED now? |
|---|---|---|
| `ingest_happy_path_makes_chunks_queryable` | #1 happy path (init → 2-file chunks.json w/ enrichment → ingest exit 0 → query text + `--format json` parses, `chunks[]` carries symbol) | **FAILS** (no `ingest` cmd) |
| `status_reflects_ingested_files_and_chunks` | #2 status sees ingested rows (Files=2 / Chunks=2 from 2 distinct file_path / 2 records) | **FAILS** |
| `enrichment_only_term_is_retrievable_after_ingest` | #3 enrichment round-trips (term ONLY in `file_docstring` and a variant ONLY in `cross_references` → retrievable) | **FAILS** |
| `ingest_ordering_is_deterministic_and_follows_array_order` | #4 determinism (two BM25-tied chunks return in JSON-array order; same JSON → two fresh DBs → identical ordering) | **FAILS** |
| `ingest_empty_array_is_a_clean_no_op` | #5 edge `[]` → exit 0, 0 files/0 chunks, query well-formed | **FAILS** |
| `ingest_malformed_json_exits_nonzero_without_panic` | #6 malformed/truncated/not-JSON → nonzero, stderr, no panic | passes\* |
| `ingest_missing_required_field_exits_nonzero_without_panic` | #7 omit `symbol_name` → nonzero, no panic | passes\* |
| `ingest_unknown_enum_exits_nonzero_without_panic` | #8 `symbol_type:"trait"` / `language:"rust"` → nonzero, no panic | passes\* |
| `ingest_wrong_json_type_exits_nonzero_without_panic` | #9 `start_byte:"x"` → nonzero, no panic | passes\* |
| `ingest_missing_input_file_exits_nonzero_without_panic` | #10 `ingest no-such.json` → nonzero, no panic | passes\* |

\* **GREEN-on-arrival and that is correct**: today clap rejects the absent `ingest` subcommand with a
clean nonzero + stderr + no panic, which already satisfies the error-path contract's shape. These are
the **lock-in** that the eng-lead's typed-error path must keep satisfying once `ingest` EXISTS (at which
point the error must come from JSON validation, not from "unknown subcommand"). They are written to
fail if a future handler ever exits 0 on bad input.

**`e2e_ingest_lib.rs` (library surface — COMPILE-RED):**
| test | scenario |
|---|---|
| `ingest_chunks_populates_queryable_db_and_reports_stats` | §3.2.4 signature: `IngestStats{files_ingested:2, chunks_ingested:2}` + symbols searchable via `Storage::search` |
| `ingest_chunks_empty_array_returns_zero_stats` | `[]` → `Ok(IngestStats{0,0})` (library-level no-op contract) |
| `ingest_chunks_invalid_input_is_typed_err` | unknown `language` → `Err(AppError)`, not panic, not Ok |

**`cli_tests.rs` append (parsing layer):**
| test | scenario | RED now? |
|---|---|---|
| `ingest_help_is_reachable` | hidden ≠ unreachable: `ingest --help` exits 0, names `<CHUNKS_JSON>` + `--db-path` | **FAILS** (unknown subcommand) |
| `ingest_subcommand_is_hidden_from_top_level_help` | `hide=true`: `--help` lists the 7 documented cmds, NOT `ingest` | passes\*\* |
| `ingest_requires_chunks_json_positional` | bare `ingest` → clap required-arg nonzero, no panic | passes\*\* |

\*\* GREEN-on-arrival (ingest absent ⇒ not in help, and bare `ingest` is already nonzero). These become
the **hidden-but-reachable lock-in** once the command exists: `ingest --help` must succeed AND the
top-level `--help` must still omit `ingest`.

### Captured RED output

**(A) Compile-RED — `e2e_ingest_lib.rs` (the new-symbol contract):**
```
error[E0432]: unresolved imports `codecache::ingest_chunks`, `codecache::IngestStats`
  --> tests\e2e_ingest_lib.rs:36:23
   |
36 | use codecache::{init, ingest_chunks, IngestStats};
   |                       ^^^^^^^^^^^^^  ^^^^^^^^^^^
   |                       |              |
   |                       |              no `IngestStats` in the root
   |                       |              help: a similar name exists in the module: `IndexStats`
   |                       no `ingest_chunks` in the root
error: could not compile `codecache` (test "e2e_ingest_lib") due to 1 previous error
```
This is the ONLY thing blocking a full `cargo test` build — exactly the brief-sanctioned RED for the
new symbols. (Run `cargo test --no-run --test e2e_ingest_lib` to reproduce in isolation.)

**(B) Behavioral-RED — `e2e_ingest` (binary, compiles clean, asserts at runtime):**
```
test enrichment_only_term_is_retrievable_after_ingest ... FAILED
test ingest_empty_array_is_a_clean_no_op ... FAILED
test ingest_happy_path_makes_chunks_queryable ... FAILED
test ingest_ordering_is_deterministic_and_follows_array_order ... FAILED
test ingest_malformed_json_exits_nonzero_without_panic ... ok
test ingest_missing_input_file_exits_nonzero_without_panic ... ok
test ingest_missing_required_field_exits_nonzero_without_panic ... ok
test ingest_unknown_enum_exits_nonzero_without_panic ... ok
test ingest_wrong_json_type_exits_nonzero_without_panic ... ok
test status_reflects_ingested_files_and_chunks ... FAILED
test result: FAILED. 5 passed; 5 failed; 0 ignored; 0 measured; 0 filtered out
```
Failure reason (right reason — command not implemented):
```
code=2
stderr=`error: unrecognized subcommand 'ingest'
  tip: some similar subcommands exist: 'index', 'init'
Usage: codecache.exe [OPTIONS] <COMMAND>`
```

**(C) Behavioral-RED — `cli_tests` (parsing layer):**
```
test ingest_help_is_reachable ... FAILED          (right reason: `ingest --help` → unrecognized subcommand, nonzero)
test ingest_requires_chunks_json_positional ... ok
test ingest_subcommand_is_hidden_from_top_level_help ... ok
test result: FAILED. 16 passed; 1 failed; 0 ignored   (cli_tests was 14 → now 17)
```

**No regressions:** `cargo --lib` + every pre-existing integration target (chunker/config/e2e_cli/
e2e_index/e2e_multilang/formatter/hasher/indexer/mcp/parser{,_ts,_go}/retrieval_quality/retriever/
smoke/storage) re-ran **all green** (lib 33; storage 25; mcp 19; indexer 15; retriever 15; …). The
only failing targets are the three new ingest concerns above.

### Contract questions the eng-lead MUST resolve (flagged)
1. **Crate-root re-export path of `ingest_chunks` + `IngestStats` (BLOCKING the lib test).** I wrote
   `tests/e2e_ingest_lib.rs` assuming a crate-root re-export `codecache::{ingest_chunks, IngestStats}`,
   mirroring the existing `pub use app::{index, init, AppError};` + `pub use indexer::IndexStats;` in
   `src/lib.rs`. Spec §3.2.4 places BOTH `ingest_chunks` and `IngestStats` on the **`app`** facade —
   so the natural addition is `pub use app::{ingest_chunks, IngestStats};` in `src/lib.rs`. **Action
   for eng-lead:** add that re-export (preferred — matches `init`/`index`), OR if you keep them under
   `codecache::app::{..}` only, change the single `use` line in `tests/e2e_ingest_lib.rs:36` to
   `use codecache::app::{ingest_chunks, IngestStats};` — but the test-lead recommendation is the
   crate-root re-export for symmetry with `index`/`init`. The binary e2e (`e2e_ingest.rs`) does NOT
   depend on this and carries the bulk of coverage regardless.
2. **`IngestStats` field names are pinned by the lib test:** `files_ingested: usize`,
   `chunks_ingested: usize` (per §3.2.4). The binary tests do not observe the struct, only the
   `status` Files/Chunks output (=2/=2 for the 2-file fixture).
3. **`files_metadata` `content_hash` sentinel — NOT observed by any test.** No test asserts the
   sentinel hash value, so the eng-lead is free to pick (`"ingested"` / `""` / etc.) per the brief.
   The only `files_metadata`-derived observable I assert is the **`status` Files count** (one row per
   distinct `file_path` ⇒ count == number of distinct paths) and the per-distinct-path chunk searchability.
   If the eng-lead later wants the sentinel pinned, say so and I will add a focused storage assertion.
4. **Empty `[]` is pinned as SUCCESS (exit 0, 0/0), not an error** — both at the binary level
   (`ingest_empty_array_is_a_clean_no_op`) and library level (`ingest_chunks_empty_array_returns_zero_stats`),
   per the brief's manager call. Do not implement an "empty input" error.
5. **Determinism is pinned to JSON-array (insertion/rowid) order.** `ingest_ordering_*` deliberately
   makes the name sort (`aaa_first` < `zzz_second`) the REVERSE of array order (`zzz_second` first,
   `aaa_first` second) so the test can only pass if insertion order — not name — drives the tie-break.
   `insert_chunks` must be called in JSON-array order.
6. **Enrichment must reach FTS5.** `enrichment_only_term_*` queries a term present ONLY in
   `file_docstring` / `cross_references`; per §4.1 the symbols FTS5 table indexes those columns, so the
   ingest DTO→Chunk mapping must populate them (not drop them) for the chunk to be retrievable.

### Notes
- `cli_tests.rs` dev-dep comment scopes `assert_cmd` to `cli_tests.rs + e2e_cli.rs`; the new
  `tests/e2e_ingest.rs` also uses it. This needs **no `Cargo.toml` change** (dev-deps are crate-wide);
  the eng-lead/manager may optionally widen that comment's wording when updating module docs. Flagging
  for transparency only.
- `docs/TEST_STRATEGY.md` should gain an ingest row under `### indexer` (or a new `### app / ingest`
  sub-bullet) when this slice lands — I did not edit it pre-GREEN to avoid implying coverage that is
  still RED; recommend the manager add it at OUTCOME. Suggested bullet: *"R2.3a (D25) `ingest
  <CHUNKS_JSON>` (hidden): array-order insertion → queryable; status Files/Chunks reflect distinct
  file_path / record counts; enrichment-only terms retrievable; `[]` = clean 0/0 no-op; malformed/
  missing-field/unknown-enum/wrong-type/missing-file → nonzero, no panic; determinism via rowid
  tie-break = array order."*

## GREEN — engineering lead
**Status: GREEN 2026-06-14** (implemented in the main session after the orchestrating manager subagent
was cut off by a session limit mid-GREEN — the RED state was clean, no half-written `src/`). All RED tests
now pass; **224 tests total** (+16: 10 `e2e_ingest` + 3 `e2e_ingest_lib` + 3 `cli_tests`); `cargo clippy
--all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean. `Cargo.toml` UNTOUCHED.

### What was implemented
- **`src/app.rs`** — the `ingest_chunks(project_root, chunks_path) -> Result<IngestStats, AppError>`
  facade (full pipeline: read file → `serde_json` parse → DTO→`Chunk` map (enum validation) →
  `insert_chunks` in array order → one `files_metadata` row per distinct `file_path` (via
  `update_file_hash`, sentinel 32-hex `content_hash`) → restamp `total_files`/`total_chunks`). Plus the
  format-local `IngestChunk` DTO + `IngestStats` + `IngestError` enum + `AppError::Ingest` variant.
- **`src/cli/ingest.rs`** (NEW) — thin handler → `crate::ingest_chunks(&cwd, chunks_json)`, prints stats.
- **`src/cli/mod.rs`** — `mod ingest;`, `Command::Ingest { chunks_json, db_path }` with `#[command(hide =
  true)]`, dispatch arm. **`src/lib.rs`** — `pub use app::{… ingest_chunks, … IngestStats}`.

### Plan deviation raised (brief-sanctioned)
- **DTO + facade BOTH live in `app.rs`, not `cli/ingest.rs`.** The library test (`e2e_ingest_lib.rs`)
  calls `app::ingest_chunks(path)` directly, so the facade must own read+parse+map+insert (the DTO must be
  reachable from `app`, which cannot depend on `cli`). The brief explicitly permitted "place the DTO beside
  the facade in `app` if that reads cleaner — the constraint is only: format-local DTO, not on
  `types::Chunk`." `cli/ingest.rs` is the thin handler. Contract Qs #1 (crate-root re-export of
  `ingest_chunks`/`IngestStats`), #2 (`IngestStats` field names), #4 (`[]`→Ok 0/0) resolved as the
  test-lead recommended.

### Observation for the reviewer (NOT a GREEN blocker — I did not touch the test)
- **`ingest_ordering_is_deterministic_and_follows_array_order` passes, but via the *retriever's* tie-break,
  not rowid.** The fixture array order is `[zzz_second (src/second.py), aaa_first (src/first.py)]`; the test
  asserts the query returns `[aaa_first, zzz_second]`. That is the order the **`Retriever`** produces — it
  re-sorts BM25 ties by `(file_path, start_byte, end_byte)` ascending (M6.2, `retriever/CLAUDE.md`), and
  `src/first.py` < `src/second.py`. So the final query order is **file_path-determined**, NOT the
  storage-level `rowid` (insertion) order the test's comment claims. The slice still inserts in array order
  (correct + storage-deterministic), and the test's core guarantee (same input → identical output) holds —
  but the comment/name overstate "insertion order drives the tie-break." Recommend the test-lead clarify
  the comment (or, to genuinely exercise rowid, two chunks would need identical `(file_path, start_byte,
  end_byte)`, which distinct chunks cannot have). Flagging per verify-don't-relay; reviewer to adjudicate.

## Specialist / Perf notes
<FTS5/storage depth only if engaged — likely none: insert_chunks + set_index_state are existing, tested APIs>

## REVIEW — code reviewer
**APPROVE 2026-06-14 — 0 blockers, 0 majors.** Independently re-ran all gates (`fmt --check` / `clippy
--all-targets -- -D warnings` / `test`) — clean; **224 passed / 0 failed**; `Cargo.toml`/`Cargo.lock`
untouched. Verified: no reachable `unwrap/expect/panic` in the ingest path (all failures typed + `?`
propagated → clean nonzero); the 13 ingest tests assert real behavior (not `is_ok` theater); `serde` stays
off `types::Chunk` (DTO in `app.rs` maps all 14 fields, `#[serde(default)]` on the 5 optionals); the reused
`insert_chunks` / `update_file_hash` (ON CONFLICT ⇒ no dup rows) / `set_index_state` are sound for this use;
impl matches §3.2.4/§7.2 + D25; `.claude/settings.json` correctly excluded (slice has no dependency on it).
- **minor (non-blocking, ADDRESSED):** the determinism test's comments + assert-message attributed the
  tie-break to "rowid/insertion order"; the deterministic order is actually the `Retriever`'s `(file_path,
  start_byte, end_byte)` key (`retriever/mod.rs:146-153`), which discards rowid. The eng-lead flagged this
  (verify-don't-relay) and **reworded the comments + assert message** (the assertion + fn name unchanged) per
  the reviewer's exact recommendation; `e2e_ingest` re-run 10/10 after the reword.

## OUTCOME — manager
**DONE 2026-06-14 (main session).** Aligned with the amended plan (§3.2.4 / §7.1 / §7.2) + ROADMAP **D25**;
RED → GREEN → reviewer APPROVE complete. GREEN was implemented in the **main session** because the
orchestrating manager subagent was cut off by a session limit mid-GREEN — the RED state it left was clean
(no half-written `src/`), and the main session independently verified the subagent-produced brief / RED tests
/ spec amendments were sound (the safety classifier was unavailable for that work) before building. Reused
existing storage APIs only — **no new `Storage` method, `Cargo.toml` untouched**. `docs/TODO.md` R2.3 row
updated (R2.3a done, R2.3b next) + `src/cli/CLAUDE.md` updated. Committed locally (no push).
**Follow-on:** R2.3b — the harness-side stub chunker + A/B plumbing over this `ingest` seam (pure
`research/`, reuses `corpus.py`) → research-harness-engineer.
