# BRIEF — M5 / indexer (M5.1–M5.4)

- **Milestone:** M5 — indexer (discovery → parse → chunk → hash → store; incremental)  ·  **Module(s):** `indexer` (+ thin `init`/`index` glue)
- **Owner (manager):** principal-engineering-manager  ·  **Created:** 2026-06-10
- **Status:** RED ✔  GREEN ✔  REVIEW ✔  DONE ✔  — all four slices M5.1–M5.4 complete (2026-06-10)
- **Links:** docs/plans/M5-indexer.md · docs/ROADMAP.md#m5--indexer · docs/TEST_STRATEGY.md#indexer · docs/project_plan.md §3.2.4 / §5.1 / §5.2 · docs/TODO.md Phase 5

## Goal
Wire the four leaf modules (storage M1, hasher M2, parser M3, chunker M4) plus `config` into a
working `Indexer` facade that: discovers source files (honoring `.gitignore` + config ignore
patterns + the configured language set), performs a correct full index of a fixture repo,
supports incremental updates that are **idempotent** (re-index of unchanged input issues no
writes), re-indexes exactly the files that changed, removes chunks for deleted files, and is
reachable end-to-end through `init → index` on a public library surface. This is the first
**integration** milestone — no new leaf algorithms, only orchestration.

## Scope (in / out)
- **In:**
  - `src/indexer/mod.rs` — `Indexer` facade per §3.2.4: `new`, `index_all`, `update_files`, plus
    private `discover_files`, `detect_changed_files`. Returns `IndexStats { files_processed,
    chunks_indexed, duration_ms }`.
  - `src/indexer/discovery.rs` — `discover_files()` via `ignore::WalkBuilder`; `detect_language(path)`
    by extension; honor config `ignore_patterns`; restrict to `config.languages` (§5.1).
  - `src/indexer/pipeline.rs` — per-file parse→chunk→hash→store orchestration + change detection;
    per-file error isolation (D2 degrade-and-continue); deletion reconciliation against
    `files_metadata`.
  - Thin `init` + `index` library entry points (create `.codecache/`, write config, `init_schema`)
    for the M5.4 e2e — **library-level only**.
  - `IndexStats` type (here unless §3.2.4 places it elsewhere — match the plan).
  - Indexing **bench skeleton** (perf engineer): a cold-index micro-bench wired but not gated; full
    validation deferred to M10.
  - Address the **M4 chunker cross-reference re-walk** perf follow-up while wiring M5.2 (see
    Follow-ups below) — single-pass bucketing of `call` nodes.
- **Out (defer):**
  - CLI command surface / `clap` wiring → **M7** (M5.4 uses library entry points, not the binary).
  - TypeScript + Go discovery/parsing correctness → **M9** (discovery may *detect* `.ts`/`.go`, but
    fixtures that get indexed are **Python-only**; language filter tests may use `.ts`/`.go` files
    only to assert they are *skipped/grouped*, never parsed).
  - BM25 retrieval/formatter → M6/M7.
  - Full perf-budget validation (cold 10K<5s / 100K<30s / incr 10 files<2s / index<100MB) → **M10**.

## Scenarios to cover (from docs/TEST_STRATEGY.md#indexer + plan §Ordered slices)

### Slice M5.1 — discovery + language detection  (`tests/indexer_tests.rs`, fixtures)
- [ ] happy: `language_detected_from_extension` (.py→Python, .ts→TypeScript, .go→Go)
- [ ] happy: `discovery_only_returns_configured_languages` (languages=[Python] ⇒ `.ts`/`.go` skipped)
- [ ] edge: `discovery_respects_gitignore` (a `.gitignore`d path is not returned)
- [ ] edge: `discovery_respects_extra_ignore_patterns_from_config`
- [ ] edge: `non_source_files_skipped` (e.g. `.md`, `.txt`, binaries)

### Slice M5.2 — full index (`index_all`)  (`tests/indexer_tests.rs`)
- [ ] happy: `index_all_populates_storage_with_expected_chunk_count`
- [ ] happy: `index_all_writes_files_metadata_for_each_file` (content_hash, mtime, file_size, language, chunk_count)
- [ ] happy: `index_all_updates_index_state_totals` (total_files / total_chunks — §5.1 step 4)
- [ ] happy: `index_all_returns_indexstats_with_counts_and_duration`
- [ ] error/D2: `malformed_file_in_repo_does_not_abort_index` (degrade, count/skip, batch continues)

### Slice M5.3 — incremental + idempotency + delete  (`tests/indexer_tests.rs`)
- [ ] happy(idempotent): `reindex_unchanged_repo_performs_no_writes` (hashes/rows unchanged; assert no delete/insert issued)
- [ ] happy: `modify_one_file_reindexes_only_that_file`
- [ ] happy: `update_files_with_n_changed_reindexes_exactly_n`
- [ ] happy: `new_file_added_gets_indexed`
- [ ] edge: `deleted_file_has_chunks_removed_and_metadata_cleared`

### Slice M5.4 — e2e init → index  (`tests/e2e_index.rs`, `tests/fixtures/repo/**`)
- [ ] e2e: `init` creates `.codecache/` (config + schema); `index` populates a queryable DB; `IndexStats` correct — all via public library entry points.

## Definition of Done
- [ ] M5.1–M5.4 green: idempotent re-index (no writes) + exact-N incremental + delete + e2e.
- [ ] Discovery honors `.gitignore` + config `ignore_patterns` + language filter.
- [ ] Malformed file does not abort a full index (D2); per-file errors counted/logged, batch continues.
- [ ] Indexing bench skeleton wired; perf budgets noted (full validation deferred to M10).
- [ ] M4 chunker cross-reference re-walk converted to single-pass bucketing; no M4/M5 budget regressed.
- [ ] `is_heuristic` persistence seam: decision recorded (see below) and honored in code.
- [ ] API matches project_plan §3.2.4 (`Indexer`, `IndexStats`) + §5.1/§5.2 algorithms.
- [ ] `cargo clippy --all-targets -- -D warnings` clean · `cargo fmt --all -- --check` clean · `cargo test --all` green.
- [ ] code-reviewer APPROVED.
- [ ] docs/TODO.md Phase 5 + `src/indexer/CLAUDE.md` updated in the same change.

---

## Execution sequence (for the runner / main session)

Drive one slice at a time, RED → GREEN → (perf) → REVIEW → manager-verify. Each agent **appends
to this brief** before handing off. Gate commands are identical to CI and the Stop hook.

**Per-slice gate commands (run in order; all must pass before the slice is "green"):**
```
cargo build
cargo clippy --all-targets -- -D warnings
cargo test --all
cargo fmt --all -- --check
```

### M5.1 — discovery + language detection
1. **principal-test-engineering-lead** — write the 5 RED tests + minimal fixtures
   (`tests/fixtures/repo/**`: a few `.py`, a `.ts`/`.go` to be skipped, a `.gitignore`, a `.md`).
   Append RED section (failing output). Tests must compile-fail/assert-fail, not error spuriously.
2. **principal-engineering-lead** — implement `src/indexer/discovery.rs` (`WalkBuilder` honoring
   `.gitignore`; apply config `ignore_patterns`; `detect_language` by extension; group/filter by
   `config.languages`). Route any `ignore`-crate gitignore-semantics questions to
   **rust-treesitter-specialist** only if needed (low risk here). Run gates → green. Append GREEN.
3. **code-reviewer** — APPROVE/BLOCK. Manager verifies, then proceed.

### M5.2 — full index (`index_all`)
1. **principal-test-engineering-lead** — 5 RED tests incl. D2 malformed-file. Append RED.
2. **principal-engineering-lead** — implement §5.1 in `pipeline.rs` + `index_all` in `mod.rs`:
   discover → group by language → per file {hash, read, parse, chunk, `insert_chunks`,
   `update_file_hash(&FileMeta)`} → accumulate `IndexStats` → `set_index_state` totals. Wrap each
   file's work so one failure is counted and skipped (D2), never aborting the batch. **While here**,
   apply the M4 cross-reference re-walk fix (single-pass bucket of `call` nodes) in `chunker`.
3. **performance-bench-engineer** — add the cold-index bench skeleton; record a baseline number vs
   the §5.4 budget (informational at M5). Append Perf notes.
4. **code-reviewer** → manager-verify → proceed.

### M5.3 — incremental + idempotency + delete
1. **principal-test-engineering-lead** — 5 RED tests; the idempotency test should assert **no
   writes** (e.g. via row/hash invariance, ideally a spy/counter on delete/insert). Append RED.
2. **principal-engineering-lead** — implement §5.2: `detect_changed_files` compares
   `compute_file_hash` vs `get_file_hash`, skip on equal; else `delete_chunks_for_file` → re-parse →
   re-chunk → `insert_chunks` → `update_file_hash`. `update_files` handles an explicit list;
   `index_all` (incremental/reconcile mode) deletes chunks+metadata for files in `files_metadata`
   no longer on disk. Append GREEN.
3. **code-reviewer** → manager-verify → proceed.

### M5.4 — e2e init → index
1. **principal-test-engineering-lead** — `tests/e2e_index.rs`: temp repo from fixtures → `init`
   (create `.codecache/`, config, schema) → `index` → assert DB queryable + stats. Public library
   surface only. Append RED.
2. **principal-engineering-lead** — thin `init`/`index` glue. Append GREEN.
3. **code-reviewer** → manager-verify.

### Closeout (manager)
- Verify full DoD; update `docs/TODO.md` Phase 5 (check boxes, record GREEN summary + gate
  versions) and `src/indexer/CLAUDE.md` (shipped API). Update `.gitignore` if M5 introduced new
  local artifacts (temp test repos go to `target/`/`tempdir`; only add patterns if anything lands
  in-tree). Engage **devops-release-engineer** only if CI gates need to mirror a new test target
  (new integration test files are auto-discovered, so usually no CI change).

### Commit-boundary recommendation
**One commit per slice (4 commits for M5).** Justification:
- M5 is four independently green, independently reviewable increments with clear seams
  (discovery / full / incremental / e2e); per-slice commits preserve the RED→GREEN→review history
  the DoD requires and keep each diff small for the reviewer and for `git bisect`.
- Each slice leaves the tree fully green (all four gates pass), so every commit is a safe landing
  point — consistent with how M1 landed as a coherent unit but M5 has more internal surface.
- The M4 cross-reference perf fix rides in the **M5.2** commit (it is wired alongside `index_all`),
  with its own line in the commit body referencing the M4 follow-up.
- Suggested messages: `M5.1: indexer discovery + language detection`, `M5.2: indexer full index
  (index_all) + chunker single-pass cross-refs`, `M5.3: indexer incremental + delete (idempotent)`,
  `M5.4: e2e init → index`. (If the runner prefers a single `M5: indexer` commit to match prior
  milestone granularity, that is acceptable — but per-slice is recommended.)

## Pre-logged follow-ups carried into M5

### (a) M4 perf follow-up — chunker cross-reference re-walk
`src/chunker/mod.rs::call_names_in_span` re-walks the whole tree **per chunk**, giving
O(chunks × tree_nodes) cross-reference enrichment — a deviation from M4's "single-pass, no
per-chunk re-query" budget (correctness unaffected; no M4 budget breached, so it was logged not
blocked). **Action:** address it in the **M5.2** slice while wiring the pipeline, because that is
where the chunker sits on the cold-index hot path and where the §5.4 budget first applies. Replace
the per-chunk re-walk with a **single walk that buckets all `call` nodes by containing chunk span**
(O(nodes + chunks·log)). `performance-bench-engineer` validates against the §5.4 cold-index budget
using the M5.2 bench skeleton. Keep the chunker's public `chunk()` signature and observable output
(deduped, first-seen `cross_references`) unchanged — this is an internal optimization, so existing
M4 chunker tests must stay green and gate the refactor.

### (b) `is_heuristic` storage-persistence seam — DECISION: **defer to M7, do not persist in M5**
**Context:** the M1 `symbols` schema has no `is_heuristic` column; `storage`'s row→`Chunk` path
reconstructs `is_heuristic: false` (see `src/chunker/CLAUDE.md` and TODO Phase 4). The flag is set
truthfully on the chunker output but is lost on round-trip through storage.
**Decision (manager):** **Defer persistence to M7; M5 does not add the column or migrate the
schema.** Rationale:
- M5's DoD and TEST_STRATEGY#indexer have **no scenario** that observes `is_heuristic` after a
  storage round-trip; nothing in the M5 pipeline branches on it. Adding it now would be untested
  production surface (violates TDD) and an un-driven schema migration.
- The first consumer that actually *surfaces* the flag is the **M7 formatter** (output may mark
  heuristic snippets) / CLI. Persisting it should be driven by an M7 RED test that reads it back.
- The indexer still **passes the chunker's `is_heuristic` through in-memory** to `insert_chunks`;
  only the *stored* representation drops it (unchanged from M4). No behavior regresses.
- **Carry-forward:** when M7 needs it, add an UNINDEXED `is_heuristic` column to `symbols` +
  `index_state.version` migration (storage owns the migration), driven by a failing formatter/CLI
  test. This is recorded here and in TODO Phase 5 so the seam is not forgotten.

---
## RED — test lead

### M5.1 — discovery + language detection (2026-06-10)

**Tests added** (`tests/indexer_tests.rs`, new file; repos built at runtime via `tempfile::TempDir`
— no committed fixture tree, `.gitignore` is created in-test):
1. `language_detected_from_extension` — `.py`→Python, `.ts`→TypeScript, `.go`→Go; `README.md` and
   extension-less `Makefile` → `None`.
2. `discovery_only_returns_configured_languages` — `languages=[Python]`, repo `{a.py, b.ts, c.go}`
   ⇒ only `a.py` returned.
3. `discovery_respects_gitignore` — `.gitignore` containing `ignored.py` ⇒ `ignored.py` excluded,
   `kept.py` returned.
4. `discovery_respects_extra_ignore_patterns_from_config` — `ignore_patterns=["*_generated.py",
   "vendor/**"]` ⇒ `schema_generated.py` and `vendor/dep.py` excluded, only `keep.py` returned
   (asserted on root-relative paths, forward-slash normalized).
5. `non_source_files_skipped` — `.md`, `.txt`, extension-less `LICENSE` excluded; only `code.py`
   returned.

All assertions sort results before comparing (discovery order is filesystem-dependent → determinism).

**Public signatures the engineering lead must implement** (decision: free functions in the
`indexer` module = the plan's "discovery.rs" split, promoted `pub` for integration-test reach.
This is the recommended option from the task brief; `Indexer::discover_files` is NOT used by these
tests):
```rust
// in src/indexer/discovery.rs, re-exported from src/indexer/mod.rs as `pub use`:
pub fn detect_language(path: &Path) -> Option<Language>;
pub fn discover_files(config: &Config, root: &Path) -> Result<Vec<PathBuf>, IndexError>;
```
- The tests import them as `codecache::indexer::{detect_language, discover_files}`, so they must be
  reachable at the `indexer` module root (re-export from `mod.rs`).
- `discover_files` returns `Result<Vec<PathBuf>, E>` where `E` is the module's error type. The tests
  only call `.expect(...)` on success, so any `E: Debug` works; the brief/plan name it `IndexError`
  — define it (even a minimal stub) so the signature matches.
- Returned `PathBuf`s must be absolute or root-prefixed (the tests `strip_prefix(root)` and fall
  back to the full path, and also read `file_name()`), so returning paths joined under `root` is
  required.

**Root / `index_paths` default decision (confirm in impl):** discovery walks `config.index_paths`
resolved against `root`; **when `index_paths` is empty (as in `Config::default()`), default to
walking `root` itself.** All five tests rely on this default (they leave `index_paths` empty and
pass the tempdir as `root`).

**Behavior the impl must satisfy (from the assertions):**
- Extension → language map is total over the three v0.1 languages; everything else → `None`.
- Discovery restricts to `config.languages` (filters out non-configured-language source files).
- `.gitignore` is honored (use `ignore::WalkBuilder`).
- `config.ignore_patterns` is applied on top of `.gitignore` (glob semantics: `*_generated.py`
  matches a filename; `vendor/**` matches everything under a directory).
- Non-source files (`.md`/`.txt`/extension-less) never appear in results.

**RED output** (`cargo test --all --test indexer_tests`, PATH-prefixed with `$HOME/.cargo/bin`):
```
   Compiling codecache v0.1.0 (C:\Users\ehlee\workspace\projects\CodeCache)
error[E0432]: unresolved imports `codecache::indexer::detect_language`, `codecache::indexer::discover_files`
  --> tests\indexer_tests.rs:27:26
   |
27 | use codecache::indexer::{detect_language, discover_files};
   |                          ^^^^^^^^^^^^^^^  ^^^^^^^^^^^^^^ no `discover_files` in `indexer`
   |                          |
   |                          no `detect_language` in `indexer`

For more information about this error, try `rustc --explain E0432`.
error: could not compile `codecache` (test "indexer_tests") due to 1 previous error
```
Fails for the right reason: the `indexer` stub does not yet expose the discovery API. No spurious
warnings (the unused-`PathBuf` import was trimmed so `-D warnings` stays clean once green).
Hand off to **principal-engineering-lead**.

<original placeholder removed>

### M5.2 — full index (`index_all`) (2026-06-10)

**Tests added** (`tests/indexer_tests.rs`, appended below the M5.1 block; repos built at runtime
via `tempfile::TempDir`, DB lives under the same tempdir — nothing touches the working tree):
1. `index_all_populates_storage_with_expected_chunk_count` — two `.py` files, each exactly one
   top-level function ⇒ `stats.chunks_indexed == 2`; asserts `alpha_fn` and `beta_fn` are each
   BM25-searchable via `storage.search(...)`.
2. `index_all_writes_files_metadata_for_each_file` — one-function `solo.py`; asserts
   `get_file_meta(root/solo.py)` is `Some` with non-empty `content_hash`, `file_size > 0`,
   `language == Python`, and `chunk_count == 1`.
3. `index_all_updates_index_state_totals` — two one-function files; asserts
   `get_index_state("total_files") == "2"` and `get_index_state("total_chunks") == "2"` (§5.1
   step 4; the `index_state` schema seeds both keys as text — see `storage/schema.rs`).
4. `index_all_returns_indexstats_with_counts_and_duration` — asserts
   `IndexStats { files_processed: 2, chunks_indexed: 2, duration_ms: u64 }`; `duration_ms` is only
   type-checked / bounded (`< u64::MAX`), never asserted on timing (determinism).
5. `malformed_file_in_repo_does_not_abort_index` (**D2**) — `good.py` (valid) + `broken.py`
   (unbalanced delimiters / garbage). `index_all()` must return `Ok` (not abort), and `good_fn`
   must be searchable afterward. The broken file may be heuristically chunked **or** skipped —
   either is acceptable; the test only asserts the batch did not abort and the good file landed.

All chunk-count asserts use **single-top-level-function** fixtures, which the M4 chunker test
`single_symbol_file_yields_one_chunk` proves yield **exactly one chunk** — so "2 files → 2 chunks"
is stable and not brittle. (For reference: `simple_class.py` = class + 2 methods → 3 chunks; I
avoided classes to keep counts trivially controlled.) Searchability assertions read
`hit.chunk.symbol_name` off `SearchResult`.

**Public API the engineering lead must implement** (pinned — match exactly so M5.3/M5.4 stay
consistent):
```rust
// in src/indexer (mod.rs facade + pipeline.rs), re-exported at the indexer module root:
pub struct IndexStats {
    pub files_processed: usize,
    pub chunks_indexed: usize,
    pub duration_ms: u64,
}
impl Indexer {
    pub fn new(config: Config, storage: Storage, root: PathBuf) -> Result<Indexer, IndexError>;
    pub fn index_all(&mut self) -> Result<IndexStats, IndexError>;
}
```
- **Root-passing decision (DECIDED — `root` is an explicit 3rd arg to `new`):**
  `Indexer::new(config, storage, root: PathBuf)`. The §3.2.4 plan signature is
  `new(config, storage)`; this RED **extends it with `root: PathBuf`** because the integration/e2e
  tests must point the indexer at a `TempDir` and the M5.1 `discover_files(config, root)` already
  takes an explicit root. Discovery resolves `config.index_paths` against `root`, defaulting to
  `root` itself when `index_paths` is empty (the confirmed M5.1 default — these tests leave
  `index_paths` empty). **Update `project_plan.md` §3.2.4 to match (`root` param) before/while
  implementing**, per the "change the plan before diverging" rule. `index_all(&mut self)` is the
  plan signature unchanged.
- Tests import `codecache::indexer::{IndexStats, Indexer}` (and `codecache::storage::Storage`), so
  both must be reachable at the `indexer` module root (re-export from `mod.rs`).
- `index_all` returns `Result<IndexStats, IndexError>` (the M5.1 module error type — extend it with
  parse/chunk/store per-file variants as needed for D2; the tests only `.expect(...)`/`is_ok()` so
  any `IndexError: Debug` compiles).

**Behavior the impl must satisfy (from the assertions):**
- Discover (M5.1 `discover_files`) → per file read/hash/parse/chunk/`insert_chunks` →
  `update_file_hash(path, &FileMeta{ content_hash (non-empty), mtime, file_size>0, language,
  chunk_count })`. `FileMeta` rows keyed by the **absolute-under-root** path the tests pass to
  `get_file_meta` (they use `root.join("solo.py")`, matching the paths `discover_files` returns).
- Accumulate `IndexStats`: `files_processed` = source files processed, `chunks_indexed` = total
  chunks inserted, `duration_ms` = wall-clock ms (any real `u64`).
- `set_index_state("total_files", …)` and `set_index_state("total_chunks", …)` with the run totals
  as decimal strings (§5.1 step 4).
- **D2:** per-file work is wrapped so a malformed/parse-failing file is counted/skipped and the
  batch continues; `index_all` returns `Ok`, never `Err`, on a malformed sibling.

**RED output** (`cargo test --all --test indexer_tests`, PATH-prefixed with `$HOME/.cargo/bin`):
```
   Compiling codecache v0.1.0 (C:\Users\ehlee\workspace\projects\CodeCache)
error[E0432]: unresolved imports `codecache::indexer::IndexStats`, `codecache::indexer::Indexer`
  --> tests\indexer_tests.rs:27:59
   |
27 | use codecache::indexer::{detect_language, discover_files, IndexStats, Indexer};
   |                                                           ^^^^^^^^^^  ^^^^^^^ no `Indexer` in `indexer`
   |                                                           |
   |                                                           no `IndexStats` in `indexer`

error: could not compile `codecache` (test "indexer_tests") due to 1 previous error
```
Fails for the right reason: the `indexer` module does not yet expose `Indexer`/`IndexStats`; the
M5.1 discovery API still resolves. `cargo test --all` is blocked only on this one target's import
(M5.1 tests in the same file compiled clean before this edit; once `Indexer`/`IndexStats` land the
whole file compiles and all 10 indexer tests run). Hand off to **principal-engineering-lead**.

### M5.3 — incremental + idempotency + delete (2026-06-10)

**Tests added** (`tests/indexer_tests.rs`, appended below the M5.2 block; repos + DB built at
runtime under `tempfile::TempDir`, reusing the existing helpers `temp_repo`/`write_file`/
`fresh_storage`/`python_indexer`/`index_state_count` + a new `searchable_symbols` helper). All five
sort/dedup before asserting (determinism). Added `use std::path::PathBuf;` (now used by #3).
1. `reindex_unchanged_repo_performs_no_writes` (idempotency) — `index_all` twice over an unchanged
   2-file repo; asserts each file's `get_file_hash`, `index_state` total_files/total_chunks, the
   modified file's `FileMeta.content_hash`/`mtime`/`chunk_count`, and the searchable symbol set are
   all byte-identical before/after.
2. `modify_one_file_reindexes_only_that_file` — index 2 files, rewrite ONE
   (`original_fn`→`renamed_fn`), call `update_files(&[changed])`; asserts the new symbol is
   searchable, the old symbol is gone, and the untouched file's stored hash + symbol are unchanged.
3. `update_files_with_n_changed_reindexes_exactly_n` — index 3 files, modify ALL 3, call
   `update_files(&[f1,f2,f3])`; asserts `stats.files_processed == 3`, all 3 new symbols searchable,
   all 3 old symbols gone (replaced not duplicated).
4. `new_file_added_gets_indexed` — full index, add a new `.py`, re-run `index_all` (reconcile mode);
   asserts the new symbol is searchable, its `FileMeta` exists (language Python, chunk_count 1), the
   pre-existing file survives, and `total_files == 2`.
5. `deleted_file_has_chunks_removed_and_metadata_cleared` — full index of 2 files, `fs::remove_file`
   one, re-run `index_all` (reconcile); asserts the deleted symbol is gone from search,
   `get_file_meta(deleted) == None`, the surviving file is intact, and totals dropped to 1/1.

**Pinned API the engineering lead must implement (match exactly):**
```rust
impl Indexer {
    // M5.3 — incremental update of an explicit file list (§5.2).
    pub fn update_files(&mut self, files: &[PathBuf]) -> Result<IndexStats, IndexError>;
}
```

- **`update_files` semantics (DECIDED — hash-filter + `files_processed` = files actually
  re-indexed):** for each path in `files`, compare `compute_file_hash` vs `get_file_hash`; if equal,
  **skip** (no delete/insert, not counted); else `delete_chunks_for_file` → re-parse → re-chunk →
  stamp `file_path` → `insert_chunks` → `update_file_hash`, and count it in
  `IndexStats.files_processed`. `chunks_indexed` = chunks inserted across the re-indexed files.
  Per-file D2 isolation as in `index_all` (a single bad file is skipped, not propagated). **Test #3
  changes every passed file**, so `files_processed == files.len()` holds whether the impl
  hash-filters or force-reindexes — but hash-filter is the pinned/recommended choice (it keeps
  `update_files` consistent with `index_all`'s skip-unchanged behavior and makes #1's idempotency
  property hold for the explicit-list path too). Should `update_files` also re-stamp `index_state`
  totals? **Recommend yes** — recompute the DB-wide totals (or apply the delta) so an incremental
  update keeps `total_files`/`total_chunks` consistent; #2/#3 don't assert totals after
  `update_files`, so either approach passes those two, but keeping totals correct avoids drift that
  a later `index_all` reconcile would have to repair. State the final choice in GREEN.
- **`index_all` incremental/reconcile semantics on a populated DB (DECIDED):** when the DB already
  has rows, `index_all` runs incrementally — (a) skip files whose `compute_file_hash` equals the
  stored `get_file_hash` (no writes), (b) re-index changed files (delete→re-insert), (c) index newly
  discovered files, and (d) **reconcile deletions**: every path present in `files_metadata` that is
  no longer returned by `discover_files` has `delete_chunks_for_file` called and its metadata row
  removed. Then re-stamp `index_state` `total_files`/`total_chunks` to the post-reconcile DB-wide
  totals. Tests #1 (skip-unchanged → no re-stamp), #4 (discover new), and #5 (reconcile delete +
  totals decrease) depend on exactly this. NOTE: the current M5.2 `index_all` **always** re-indexes
  every discovered file and never reconciles deletions, so #1/#4/#5 compile but FAIL until §5.2 lands;
  #2/#3 compile-fail on the missing `update_files`.
  - **Metadata-row deletion seam:** reconcile + the modify path both need to *remove* a
    `files_metadata` row (delete case) — `storage` currently exposes `delete_chunks_for_file` (chunks
    only) and `update_file_hash` (upsert), but **no `delete_file_meta`/row-delete**. The eng lead
    must either add a `Storage::delete_file_meta(&Path)` (recommended — small, symmetric with
    `delete_chunks_for_file`; storage owns it) or delete the row via an existing path. Pin the choice
    in GREEN; tests assert the *observable* (`get_file_meta(deleted) == None`), not the method name.
  - **Enumerating known files for reconcile:** detecting "in `files_metadata` but not on disk"
    requires listing the stored file paths. There is currently no `Storage` method to enumerate
    `files_metadata` keys. The eng lead will likely need a `Storage::all_indexed_files() ->
    Vec<PathBuf>` (or similar) — add it (storage-owned) and drive it from this slice's reconcile.
    Tests assert only observables, so the exact name is the eng lead's choice.
- `detect_changed_files(&self, files: &[PathBuf]) -> Result<Vec<PathBuf>, IndexError>` may stay
  **private**; it is NOT tested directly (the suite exercises it through `update_files`/`index_all`).

**Idempotency observability approach + limitation (#1):** an integration test cannot spy on SQLite
to prove "no DELETE/INSERT was issued". The proxy used is: (i) stored `get_file_hash` per file
unchanged, (ii) `index_state` totals unchanged, (iii) the modified file's `FileMeta.content_hash` /
`mtime` / `chunk_count` unchanged (a re-index of an unchanged file would re-stamp these via
`update_file_hash`, since `compute_file_hash` re-mixes the same content+mtime → same value but the
write still happens; the **stronger** signal the impl must satisfy is that it SKIPS the write
entirely so nothing is even re-stamped), and (iv) the searchable symbol set unchanged (a
delete+re-insert would change rowids/ordering and risk dupes). Limitation: because the recomputed
hash is identical for unchanged content, asserting the *value* of `content_hash`/`mtime` cannot
distinguish "skipped the write" from "re-wrote the identical value" — it only catches a *wrong*
re-stamp. The load-bearing guarantee that no write was issued is enforced at the unit level by the
eng lead's hash-compare skip in `detect_changed_files` (recommend a `#[cfg(test)]` unit test in
`pipeline.rs` asserting `detect_changed_files` returns empty for an unchanged repo); this integration
test pins the observable invariants. Documented here per the brief's request.

**RED output** (`cargo test --all` and `cargo test --all --test indexer_tests`, PATH-prefixed with
`$HOME/.cargo/bin`):
```
   Compiling codecache v0.1.0 (C:\Users\ehlee\workspace\projects\CodeCache)
error[E0599]: no method named `update_files` found for struct `Indexer` in the current scope
   --> tests\indexer_tests.rs:559:10
    |
558 |       let stats = indexer
    |  _________________-
559 | |         .update_files(&[changed.clone()])
    | |         -^^^^^^^^^^^^ method not found in `Indexer`
    |
error[E0599]: no method named `update_files` found for struct `Indexer` in the current scope
   --> tests\indexer_tests.rs:622:10
    |
621 |       let stats = indexer
    |  _________________-
622 | |         .update_files(&files)
    | |         -^^^^^^^^^^^^ method not found in `Indexer`
    |
error: could not compile `codecache` (test "indexer_tests") due to 2 previous errors
```

**Mixed-RED state (expected, per brief):** `update_files` does not exist → tests #2/#3 **compile-fail**,
which blocks the whole `indexer_tests` target, so #1/#4/#5 (which compile) cannot run yet. Once
`update_files` lands the target compiles and the RED resolves to: #2/#3 pass, and #1/#4/#5 **fail by
assertion** because the current `index_all` always re-indexes and never reconciles deletions —
exactly the §5.2 behavior the eng lead must add. The M5.1+M5.2 tests in the same file are unchanged
and will pass again once the target compiles. Fails for the right reason (missing API + missing
incremental/reconcile logic), no typos/spurious warnings (the new `PathBuf` import is used by #3).
Hand off to **principal-engineering-lead**.

### M5.4 — e2e init → index (2026-06-10)

**Tests added** (`tests/e2e_index.rs`, **new file**; the temp repo + `.codecache/` + DB are all
built at runtime under `tempfile::TempDir` — preferred over a committed `tests/fixtures/repo/**`
tree, consistent with M5.1–M5.3). Public library surface only; no private internals, no CLI.
1. `e2e_init_then_index_populates_queryable_db` — builds a 3-file Python repo (`auth.py` =
   1 function, `service.py` = class `Service` + `__init__` + `process`, `util.py` = 1 function).
   `init(root)` ⇒ asserts `.codecache/` dir, `.codecache/config.toml`, and the resolved DB file
   all exist. `index(root)` ⇒ asserts `IndexStats { files_processed: 3, chunks_indexed: 5 }`
   (1 + 3 + 1; the class file yields 3 chunks per the M4 chunker — one chunk per class/method
   definition), then re-opens the DB via the public `Storage::new`/`search` and asserts
   `authenticate_user` and the `process` method are searchable.
2. `e2e_init_is_idempotent_or_safe` — `init` twice must not error and must not clobber: the
   `config.toml` bytes are captured after the first init and asserted **byte-identical** after the
   second; the project still `index`es to `files_processed == 3` and symbols stay queryable.
3. `e2e_reindex_after_modification_reflects_change` (the optional M5.3-tie-in, kept — cheap):
   `init` → `index` → rewrite `util.py` (`normalize_path`→`canonicalize_path`) → `index` again ⇒
   the new symbol is searchable, the old symbol is gone, and an untouched file's symbol survives.
4. `app_error_is_public_and_debuggable` — type-level assertion that `AppError: std::error::Error`
   is public/reachable at the crate root.

All search-set assertions sort+dedup before comparing (determinism). The e2e re-opens the *same*
on-disk DB `init`/`index` wrote, so the test and impl must agree on the resolved DB path (pinned
below).

**Pinned public surface the engineering lead must implement (match EXACTLY).** Decision:
a thin **`src/app.rs`** facade re-exported at the crate root, so the e2e imports
`codecache::{init, index, AppError, IndexStats}` (clean public surface — recommended in the task
brief over `indexer`-reaching free functions):
```rust
// src/app.rs — re-exported from src/lib.rs as `pub use app::{init, index, AppError};`
// and ALSO re-export `IndexStats` at the crate root: `pub use indexer::IndexStats;`
use std::path::Path;
use crate::indexer::IndexStats;

/// Create `<root>/.codecache/`, write a default `config.toml`, create + `init_schema()` the DB.
pub fn init(project_root: &Path) -> Result<(), AppError>;

/// Load `<root>/.codecache/config.toml`, open `Storage` at the resolved db_path,
/// `Indexer::new(config, storage, root)`, run `index_all`, return its stats.
pub fn index(project_root: &Path) -> Result<IndexStats, AppError>;

#[derive(Debug)]
pub enum AppError {
    Config(crate::config::ConfigError),
    Storage(crate::storage::StorageError),
    Index(crate::indexer::IndexError),
}
// + impl Display + std::error::Error (source() chain) — no reachable unwrap/expect/panic.
```
- **Crate-root re-exports required:** the test imports `codecache::{index, init, AppError,
  IndexStats}` — all four must resolve at the crate root. `IndexStats` currently lives at
  `codecache::indexer::IndexStats`; add `pub use indexer::IndexStats;` (or
  `pub use app::{init, index, AppError};` + an `IndexStats` re-export) to `src/lib.rs`. (Compiler
  even suggests `codecache::indexer::IndexStats` — promote it to the root rather than changing the
  test, since the e2e specifies the facade's public surface.)
- **`AppError` (DECIDED — new top-level enum wrapping the three sub-errors).** Recommended over
  reusing `IndexError` for a clean public boundary: `init` can fail on storage (DB create/schema)
  and IO (config write); `index` can fail on config load, storage open, or indexing. Wrap each in a
  variant with `From` impls + a `source()` chain. The test only needs `AppError: std::error::Error`
  + `Debug`, so the exact variant set is the eng lead's to finalize — but the three-way wrap is the
  pinned shape.
- **DB-path resolution (PINNED):** `init` and `index` load (or default) the `Config`, take
  `config.storage.db_path` (default `.codecache/index.db`), and **resolve it against
  `project_root`**: `project_root.join(&config.storage.db_path)`. For the default config this is
  `<root>/.codecache/index.db` — exactly the path the test re-opens via `Storage::new`. (If
  `db_path` is absolute, `Path::join` already yields it unchanged — fine.) `init` must
  `fs::create_dir_all` the DB's parent (`<root>/.codecache/`) before `Storage::new`, since
  `Connection::open` will not create missing parent directories.
- **Config written by `init` (PINNED):** serialize `Config::default()` to TOML (e.g.
  `toml::to_string`/`toml::to_string_pretty`) and write it to `<root>/.codecache/config.toml`. The
  default config has empty `index_paths` ⇒ discovery walks `root` itself (the confirmed M5.1
  default), and `languages = [python, typescript, go]` (the repo is Python-only, so only `.py` is
  indexed). `index` then loads this file via `Config::load`. Note: `toml` is already a dependency
  and `Config` already `#[derive(Serialize)]`, so no new deps and no new derive are needed.
- **Idempotency semantics (DECIDED — re-init is safe and non-clobbering):** a second `init` must
  **not** error and must **not** rewrite an existing `config.toml` (test #2 asserts the bytes are
  identical across two inits — so guard the config write with an "exists?" check / write-if-absent;
  do NOT unconditionally overwrite). `init_schema()` is already idempotent (`CREATE ... IF NOT
  EXISTS`), so re-creating the `Storage` and re-calling it is a safe no-op. Net: `init` is callable
  any number of times without data loss or error.
- **`index` on a populated DB:** calls `Indexer::index_all`, which is already incremental + reconcile
  (M5.3) — so test #3's second `index` correctly re-indexes the changed file and drops the old
  symbol. No new indexer logic required for M5.4; this slice is **pure thin glue**.

**RED output** (`cargo test --all --test e2e_index`, PATH-prefixed with `$HOME/.cargo/bin`):
```
   Compiling codecache v0.1.0 (C:\Users\ehlee\workspace\projects\CodeCache)
error[E0432]: unresolved imports `codecache::index`, `codecache::init`, `codecache::AppError`, `codecache::IndexStats`
  --> tests\e2e_index.rs:41:17
   |
41 | use codecache::{index, init, AppError, IndexStats};
   |                 ^^^^^  ^^^^  ^^^^^^^^  ^^^^^^^^^^ no `IndexStats` in the root
   |                 |      |     |
   |                 |      |     no `AppError` in the root
   |                 |      no `init` in the root
   |                 no `index` in the root
   |
   = help: consider importing this struct instead:
           codecache::indexer::IndexStats

error: could not compile `codecache` (test "e2e_index") due to 1 previous error
```
Fails for the right reason: the `init`/`index`/`AppError` facade and the crate-root `IndexStats`
re-export do not exist yet. `cargo test --all` aborts on this one target's compile-fail (expected
RED). Verified the rest of the suite is untouched by running the unaffected targets directly:
`cargo test --all --test indexer_tests` ⇒ **15/15 pass**; `cargo test --lib` ⇒ **15/15 pass**
(incl. `indexer::pipeline::tests::detect_changed_files_empty_for_unchanged_repo`). Once the facade
lands the `e2e_index` target compiles and its 4 tests run green. Hand off to
**principal-engineering-lead**.

## GREEN — engineering lead

### M5.1 — discovery + language detection (2026-06-10)

**Implemented** (matches the RED-pinned signatures exactly; no plan deviation):
- `src/indexer/discovery.rs`:
  - `pub fn detect_language(path: &Path) -> Option<Language>` — extension match `.py`→Python,
    `.ts`→TypeScript, `.go`→Go; everything else (incl. extension-less) → `None`. Minimal, only the
    three v0.1 languages the tests assert (no `.tsx`/`.jsx`/etc.).
  - `pub fn discover_files(config: &Config, root: &Path) -> Result<Vec<PathBuf>, IndexError>` —
    walks each `config.index_paths` entry joined under `root`, defaulting to **`root` itself when
    `index_paths` is empty** (the confirmed default; all 5 tests rely on it). Returned paths are
    full paths joined under `root`, so the tests' `strip_prefix(root)` and `file_name()` both work.
  - Private helpers `resolve_walk_roots`, `build_ignore_patterns`, `is_configured_language` keep
    `discover_files` small.
- `src/indexer/mod.rs`: `mod discovery; pub use discovery::{detect_language, discover_files};` plus
  the typed `IndexError` enum (`Io { path, source }`, `Glob { pattern, source }`) following the
  `ConfigError`/`HasherError` style — `impl Display + std::error::Error` with `source()`, no
  reachable `unwrap()/expect()/panic!`. `lib.rs` already declared `pub mod indexer;` (verified, no
  change needed).

**Gitignore / glob approach chosen:**
- `.gitignore` honored via `ignore::WalkBuilder` with **`.require_git(false)`**. This was the one
  non-obvious bit: `WalkBuilder` defaults to `require_git(true)`, which only applies `.gitignore`
  rules *inside a git repo*. The discovery tests build a bare `tempfile::TempDir` (no `.git`), so
  without `require_git(false)` the `.gitignore` was silently ignored (`discovery_respects_gitignore`
  failed: `ignored.py` leaked through). `require_git(false)` is also the correct production
  semantics — the indexer indexes plain source trees, not only checkouts.
- `config.ignore_patterns` applied as **gitignore-style globs** via a separate
  `ignore::gitignore::GitignoreBuilder` anchored at `root`, matched with
  `matched_path_or_any_parents(path, false).is_ignore()`. Chose this over `OverrideBuilder` because
  Override's whitelist-by-default inversion (a plain glob *whitelists*, you must negate to ignore)
  is the opposite of the intended "these patterns are extra ignores" semantics and reads
  confusingly. A `Gitignore` matcher treats a plain glob as an *ignore* (matching the user's mental
  model and the `.gitignore` file), so `vendor/**` and `*_generated.py` Just Work — and
  `matched_path_or_any_parents` covers the `vendor/dep.py`-under-an-ignored-dir case. No new
  dependency (only the `ignore` crate, already present).

**Seam notes for M5.2+:**
- `detect_language` / `discover_files` are `pub` free functions at the `indexer` root. When the
  `Indexer` facade lands (M5.2), `index_all` can call `discover_files` directly; the brief's
  §3.2.4 `Indexer::discover_files` private method is not required by these tests and can wrap or
  delegate to the free function as preferred.
- `IndexError` currently has `Io` + `Glob`; M5.2 will likely add variants for parse/chunk/store
  per-file failures (D2 degrade-and-continue) — extend the enum, keep the `source()` chain.
- Returned paths are absolute-under-`root` `PathBuf`s; downstream hashing/parsing can use them
  directly. If M5.2 wants root-relative storage keys it should `strip_prefix(root)` at the storage
  boundary (consistent with how the tests normalize).

**Gate output (PATH-prefixed with `$HOME/.cargo/bin`, all four green):**
```
cargo build                                  → Finished (clean)
cargo clippy --all-targets -- -D warnings    → Finished (no warnings)
cargo test --all                             → all green; 5/5 indexer_tests pass
cargo fmt --all -- --check                   → clean (exit 0)
```
`tests/indexer_tests.rs`: 5 passed / 0 failed. Whole suite: **81 tests** across all targets
(lib 14, chunker_proptest 3, chunker 10, config 5, hasher 11, **indexer 5**, parser 14, smoke 1,
storage 18; main 0, doctests 0) — up from 76 by exactly the 5 new M5.1 tests.

Hand off to **code-reviewer**.

## Specialist / Perf notes

### M5.2 — cold-index bench skeleton (2026-06-10) — performance-bench-engineer

**Bench:** `benches/indexing.rs` — criterion group `cold_index`, bench function
`index_all_50_py_files`.

**What it measures:** wall-clock time for one full `Indexer::index_all()` call over a synthetic
repo of 50 Python files (~500 LOC total, ~2 functions per file), with a cold (freshly-created,
schema-initialized) SQLite DB per iteration. Exercises the complete hot path: discover →
compute-file-hash → read → parse (Tree-sitter Python) → chunk → insert_chunks (batch
transaction) → update_file_hash → set_index_state. Each iteration constructs a fresh `Storage` +
`Indexer` so there is no warm-cache effect in the index layer.

**Baseline (Windows 11 Home 10.0.26200, Intel/AMD dev machine, Rust 1.85, `--release` profile,
criterion 10 samples, 2026-06-10):**
  - Median (p50): ~1.10 s
  - Observed range: [1.02 s, 1.20 s]
  - Input scale: ~500 LOC / 50 files

**Budget comparison (§5.4 — informational at M5; full validation deferred to M10):**
  - Target: cold 10K LOC < 5 s
  - Measured at 500 LOC: ~1.10 s ≈ 22 ms per 10 LOC / 10 files
  - Naive linear extrapolation to 10K LOC ≈ 22 s (> 5 s budget) and to 100K LOC ≈ 220 s (> 30 s
    budget). Extrapolation is pessimistic (SQLite bulk-insert amortizes over more chunks; parse and
    hash throughput scale sub-linearly for larger files vs many tiny files), but the numbers flag
    that the hot path will need profiling and optimization before M10.
  - Recommendation: at M10 profile `insert_chunks` transaction overhead (many tiny single-chunk
    files vs fewer large files), hasher I/O cost, and tree-sitter parse time. The 50-tiny-file
    fixture is worst-case for per-file fixed overhead (DB open + transaction + metadata write per
    file); real repos have larger files and amortize better.

**CI gating:** NOT gated — wired informational only per brief scope. Do not add threshold
failures to this bench before M10.

**To reproduce / compare:**
```
cargo bench --bench indexing                                    # run baseline
cargo bench --bench indexing -- --save-baseline before         # save named baseline
cargo bench --bench indexing -- --baseline before              # compare against saved
```

## REVIEW — code reviewer
<APPROVE / BLOCK + findings: severity — file:line — problem — fix>

## OUTCOME — manager
<aligned? TODO updated? slice marked done? follow-ups created?>

### M5.1 — discovery + language detection (2026-06-10) — **APPROVE**

Reviewed: `src/indexer/discovery.rs`, `src/indexer/mod.rs`, `tests/indexer_tests.rs`,
`src/indexer/CLAUDE.md`. Re-ran indexer tests (5/5), clippy `--all-targets -D warnings` (clean),
`fmt --check` (clean) — all green.

**Verdict: APPROVE.** Correct, idiomatic, aligned; no blockers, no majors.

Correctness confirmed:
- `.require_git(false)` is sound and correct production semantics — without it `.gitignore` is
  silently inert outside a checkout; gitignore test genuinely exercises gitignore (separate
  `.gitignore` file + `kept.py`/`ignored.py`, not the config-pattern path).
- `config.ignore_patterns` via an anchored `Gitignore` + `matched_path_or_any_parents(path,false)`
  gives the intended "extra ignores" semantics (plain glob = ignore), and the parent-walk correctly
  excludes `vendor/dep.py` under `vendor/**`. Override-vs-Gitignore decision is the right call.
- Language filter, empty-`index_paths`→walk-`root` default, file-type gate, and
  absolute-under-`root` paths all match the RED contract and §5.1 pseudocode (free-fn shape).
- `IndexError` is typed, `impl Display + Error` with a correct `source()` chain; no reachable
  `unwrap()/expect()/panic!` in production. `?`/`map_err` throughout.

Tests: deterministic (all sorted), assertions meaningful (exact-vec equality, not `is_ok()`),
gitignore vs config-pattern paths exercised independently. No scope creep into M5.2.

Nits (non-blocking, optional — do NOT fix this slice):
- minor — `src/indexer/discovery.rs:62-67` — config `ignore_patterns` filter runs per-file and does
  not prune directories, so the walker still descends ignored trees (e.g. `vendor/`). Correct, but
  loses gitignore-style pruning; if M5.2 perf wants it, feed patterns into the `WalkBuilder` overrides
  instead. Out of scope for M5.1.
- minor — `discovery.rs:62` vs `:65` — language filter precedes the ignore-pattern check; order is
  immaterial to results (both must pass) and arguably cheaper as-is. No action.
- minor — `detect_language` is intentionally `.py/.ts/.go`-only (no `.tsx/.jsx/.pyi`); correct per
  M5.1/M9 scope. Note only.

Slice M5.1 is DONE-eligible. Hand back to manager.

### M5.2 — full index (`index_all`) (2026-06-10)

**Implemented** (matches the RED-pinned signatures exactly):
- `src/indexer/mod.rs` — `Indexer { config, storage, root, parser }` facade:
  - `Indexer::new(config: Config, storage: Storage, root: PathBuf) -> Result<Indexer, IndexError>`
    — builds one reusable `Parser` up front (`Parser::new` → `IndexError::Parser` on a bad query).
  - `Indexer::index_all(&mut self) -> Result<IndexStats, IndexError>` — §5.1: `discover_files`
    (M5.1 free fn) → per file `pipeline::index_file` → accumulate `IndexStats` →
    `set_index_state("total_files"/"total_chunks", <decimal>)` → `duration_ms` via
    `std::time::Instant`. `IndexStats { files_processed, chunks_indexed, duration_ms }`
    (`Copy`+`Default`). Both `IndexStats` and `Indexer` re-exported at the `indexer` module root.
  - I did **not** group by language (§5.1 step 2 is optional per the brief): a single reusable
    `Parser` handles all languages and `detect_language` is recomputed per file in the pipeline, so
    the explicit `HashMap<Language, Vec<PathBuf>>` grouping is unnecessary for correctness. Noted as
    a deliberate simplification; revisit only if a per-language parser pool is wanted later.
- `src/indexer/pipeline.rs` (new) — `index_file(parser, storage, path) -> Result<usize, IndexError>`:
  §5.1 step 3a–3e — `hasher::compute_file_hash` → `read_to_string` + `fs::metadata` (size, mtime) →
  `detect_language` → `parser.parse_file` → `chunker::chunk` → **stamp `chunk.file_path = path`** on
  each chunk (parser/chunker leave it empty; the tests query `get_file_meta`/`search` by the
  absolute-under-root path) → `insert_chunks` → build `FileMeta` → `update_file_hash`. Returns the
  chunk count. `file_mtime_secs` defensively yields `0` on a clock quirk (the hash already encodes
  mtime authoritatively) rather than failing the whole file.
- `IndexError` extended (typed, `impl Error` + `source()` chain, no reachable panic) with
  `File{path,source}`, `Hash(HasherError)`, `Parser(ParserError)`, `Chunker(ChunkerError)`,
  `Storage(StorageError)` alongside the M5.1 `Io`/`Glob`.

**D2 per-file isolation approach:** `index_all` wraps each `index_file` call in a `match` — `Ok(n)`
adds to `files_processed`/`chunks_indexed`; `Err(_)` counts the file as skipped and continues. The
batch never aborts on one bad file (`index_all` returns `Ok`). The chunker degrades a malformed
tree internally (heuristic fallback / empty via `error_rate`), so `broken.py` returns `Ok` from the
pipeline and the malformed-file test passes with `good_fn` still searchable. Only non-isolatable
failures (discovery walk/glob, the `index_state` totals write) propagate as `Err` from `index_all`.

**Chunker single-pass cross-ref refactor — DONE (not deferred):** `src/chunker/mod.rs` previously
re-walked the whole tree per chunk in `call_names_in_span` (O(chunks × tree_nodes)). Replaced with
`collect_calls(root, source) -> Vec<CallSite>` (one DFS `TreeCursor` walk collecting every
bare-identifier `call`'s span+name in document order), and `call_names_in_span(&calls, start, end)`
now filters that pre-collected slice by span and dedups in first-seen order — O(nodes + chunks·calls).
`chunk()`'s public signature and observable output (deduped, first-seen DFS order `cross_references`)
are unchanged; the M4 chunker suite (10 integration + 3 proptest) stays green and gates the refactor.

**Plan edit (§3.2.4):** updated `docs/project_plan.md` §3.2.4 to show
`Indexer::new(config, storage, root: PathBuf)` with a comment recording the reason (integration/e2e
tests must point discovery at an explicit `TempDir` root; `discover_files(config, root)` already
takes an explicit root), and annotated `index_all`/`update_files` return types as
`Result<_, IndexError>`. Done before/with the implementation per "change the plan before diverging".

**Gate output (PATH-prefixed `$HOME/.cargo/bin`, all four green):**
```
cargo build                                  → Finished (clean)
cargo clippy --all-targets -- -D warnings    → Finished (no warnings)
cargo test --all                             → all green; indexer 10/10 (5 M5.1 + 5 M5.2)
cargo fmt --all -- --check                   → clean (exit 0)
```
`tests/indexer_tests.rs`: 10 passed / 0 failed. Whole suite: **86 tests** total (up from 81 by
exactly the 5 new M5.2 tests). Chunker tests (10 + 3 proptest) confirmed still green after the
cross-ref refactor.

Note for manager: `cargo fmt --all` reformatted whitespace in `tests/indexer_tests.rs` (the RED
file was not rustfmt-clean — two `assert!` calls wrapped). Only formatting/whitespace changed; no
assertion, name, or test was altered/weakened — all 10 tests still pass. This matches the project's
fmt-on-edit gate that formats every `.rs`.

Hand off to **performance-bench-engineer** (cold-index bench skeleton) then **code-reviewer**.

### M5.3 — incremental + idempotency + delete (2026-06-10) — engineering lead

**Implemented** (matches the RED-pinned signatures exactly; no signature deviation):
- `src/indexer/pipeline.rs`:
  - `detect_changed_files(storage, &[PathBuf]) -> Result<Vec<PathBuf>, IndexError>` — for each
    candidate, `hasher::compute_file_hash` vs stored `storage.get_file_hash`; equal ⇒ skip,
    differ/absent ⇒ changed. A file whose hash can't be computed is treated as *changed* (the
    caller's D2 path isolates the failure rather than silently dropping it). This is the
    **no-write predicate**: an unchanged file is never in the re-index set.
  - `reindex_file(parser, storage, path) -> Result<usize, IndexError>` — `delete_chunks_for_file`
    **first** (no stale/duplicate chunks across re-indexes), then the existing `index_file` path
    (re-parse → re-chunk → `insert_chunks` → `update_file_hash`). Reuses M5.2 `index_file` exactly.
  - `#[cfg(test)] mod tests::detect_changed_files_empty_for_unchanged_repo` — unit test locking the
    no-write guarantee: after indexing a file, `detect_changed_files` over the untouched file is
    empty.
- `src/indexer/mod.rs`:
  - `Indexer::update_files(&mut self, files: &[PathBuf]) -> Result<IndexStats, IndexError>` —
    `detect_changed_files` over the explicit list → `reindex_each` (delete-first, D2-isolated) →
    `restamp_index_state`. `files_processed` = files actually re-indexed (hash-filter, per the
    pinned semantics; tests #2/#3 change every passed file so the count matches either way).
  - `index_all` rewritten to **incremental + reconcile**: discover → `detect_changed_files`
    (skip-unchanged = no writes) → `reindex_each(changed)` → reconcile deletions (every
    `all_indexed_files()` path not in the discovered `HashSet` ⇒ `delete_chunks_for_file` +
    `delete_file_meta`) → `restamp_index_state`.
  - private `reindex_each` (accumulate stats over delete-first re-index, D2 match-isolation) and
    `restamp_index_state` (recompute `total_files` = `files_metadata` row count, `total_chunks` =
    summed `chunk_count`, write both as decimal strings).

**Storage methods added** (internal CRUD, symmetric with existing schema; plan §3.2.2 updated):
- `Storage::delete_file_meta(&Path) -> Result<()>` (`DELETE FROM files_metadata WHERE file_path=?1`)
  — symmetric with `delete_chunks_for_file`; deleting an unknown file is a no-op (0 rows). Used by
  the reconcile path (delete case in `index_all`).
- `Storage::all_indexed_files() -> Result<Vec<PathBuf>>` (`SELECT file_path FROM files_metadata`,
  `prepare_cached` + `query_map`) — enumerates the indexed set to (a) reconcile against disk and
  (b) recompute DB-wide totals. Follows the existing `Arc<Mutex<Connection>>` + typed `StorageError`
  + prepared-statement style. Query constants `DELETE_FILE_META` / `ALL_INDEXED_FILES` in
  `queries.rs`.

**Idempotency / no-writes guarantee (test #1):** an unchanged file fails the
`detect_changed_files` hash compare, so it is never passed to `reindex_each` — no
`delete_chunks_for_file`, no `insert_chunks`, no `update_file_hash`. Its stored hash, `FileMeta`
(content_hash/mtime/chunk_count) and chunk rowids are physically untouched on a re-run. The compare
holds equal because the stored `files_metadata.content_hash` **is** the value `compute_file_hash`
returns (content+mtime xxhash3-128, same 32-hex format) — `index_file` stores exactly the hash
`detect_changed_files` recomputes, so a second unchanged run sees no delta. The restamp on the skip
path is a no-op write of the same totals (totals are recomputed from the unchanged `files_metadata`),
so `total_files`/`total_chunks` are byte-identical. Locked at unit level by
`detect_changed_files_empty_for_unchanged_repo`.

**Reconcile logic (tests #4/#5):** `index_all` builds a `HashSet` of the discovered on-disk paths,
then for each `all_indexed_files()` entry not in that set, deletes its chunks + metadata row.
New files (#4) are picked up because they have no stored hash ⇒ `detect_changed_files` flags them
changed. Deleted files (#5) are evicted by the reconcile pass and `restamp_index_state` then drops
totals to the surviving counts (recompute-from-`files_metadata`, not delta arithmetic, so it can't
drift).

**Plan edit:** `docs/project_plan.md` §3.2.2 — added `delete_file_meta` and `all_indexed_files` to
the `Storage` impl surface with comments explaining the M5.3 reconcile rationale (plan-first, per
"change the plan before diverging"). No signature changes to the indexer API (`update_files` already
matched §3.2.4).

**Gate output (PATH-prefixed `$HOME/.cargo/bin`, all four green):**
```
cargo build                                  → Finished (clean)
cargo clippy --all-targets -- -D warnings    → Finished (no warnings)
cargo test --all                             → all green; indexer 15/15 (5 M5.1 + 5 M5.2 + 5 M5.3)
cargo fmt --all -- --check                   → clean (exit 0)
```
`tests/indexer_tests.rs`: 15 passed / 0 failed. Whole suite: **92 tests** total (up from 86 by the
5 new M5.3 integration tests + 1 new `pipeline` unit test). M5.1/M5.2 tests unchanged and green;
storage 18/18 green after the two new methods.

Hand off to **code-reviewer**.

### M5.2 — full index (`index_all`) + chunker single-pass cross-refs (2026-06-10) — **APPROVE**

Reviewed: `src/indexer/mod.rs`, `src/indexer/pipeline.rs`, `src/chunker/mod.rs` (diff),
`tests/indexer_tests.rs`, `benches/indexing.rs`, `src/indexer/CLAUDE.md`, `docs/project_plan.md`
§3.2.4. Re-ran indexer (10/10), chunker (10/10), chunker_proptest (3/3) — all green.

**Verdict: APPROVE.** Correct, idiomatic, aligned; no blockers, no majors.

Correctness confirmed:
- **Chunker single-pass refactor is behavior-preserving.** The old per-chunk predicate
  `node.start_byte() >= start && node.end_byte() <= end` is byte-for-byte identical to the new
  `call.start >= start && call.end <= end`; `collect_calls` does the same DFS `TreeCursor` walk and
  the same `call_function_name` (bare-identifier-only) filter, so the `CallSite` slice is in the
  exact DFS order the old per-span walk visited. `call_names_in_span` keeps the same first-seen
  dedup. Observable `cross_references` set + order is identical. Nested-function semantics are
  preserved: a call in an inner fn is contained in BOTH the inner and the enclosing chunk's span
  under both old and new code (same containment predicate) — so if the old code "double-counted"
  into the outer chunk, the new code does too; no regression either way.
- **§5.1 alignment.** discover → per-file hash/read+metadata/parse/chunk/insert_chunks/
  update_file_hash → set_index_state(total_files/total_chunks) → IndexStats+duration all match.
  Using `chunker::chunk` instead of the pseudocode's raw `parser.extract_chunks` is correct (M4
  enrichment layer) and an improvement, not drift.
- **FileMeta built correctly:** content_hash (xxhash via compute_file_hash), file_size (fs metadata
  len), mtime (epoch secs, defensive 0), language (detect_language), chunk_count (chunks.len()).
  Chunks are stamped with `file_path = path` before insert (parser/chunker leave it empty), so
  get_file_meta/search key on the absolute-under-root path the tests query — verified.
- **D2 isolation is sound and cannot abort.** Each `index_file` is wrapped in a `match`; `Err` is
  counted-as-skipped, batch continues, `index_all` returns Ok. Only non-isolatable failures
  (discovery walk/glob, the two set_index_state writes) propagate — correct. Confirmed `parse_file`
  returns a tree (with ERROR nodes) on the broken fixture rather than Err, so broken.py takes the
  chunker heuristic/empty path and the good file lands; the test passes for the right reason.
- **Idiomatic / rules.** No reachable unwrap/expect/panic in production (the two `unwrap_or` at
  pipeline.rs:56,101 are infallible defaults). IndexError is typed with a complete Display +
  source() chain across all 7 variants. `?`/map_err throughout. The "skip group-by-language"
  deviation is sound — `Parser::parse_file` sets the grammar per call (parser/mod.rs:147), so one
  reused parser correctly handles every language; the HashMap grouping is unnecessary.
- **Plan edit (§3.2.4) is accurate and minimal:** adds `root: PathBuf` as the 3rd arg with a
  justifying comment (e2e/integration must point discovery at a TempDir; discover_files already
  takes an explicit root), annotates the IndexError return types — no other surface changed.
  Consistent with the RED contract and "change the plan before diverging."

Tests: genuine, not weakened. Exact chunk/file counts (2 files → 2 chunks via single-fn fixtures),
real searchability asserts (`symbol_name == ...` off SearchResult, not is_ok), FileMeta field
asserts, index_state totals parsed as usize, D2 returns-Ok + good_fn-searchable. fmt-only whitespace
change to the RED file (two wrapped asserts) — no assertion altered.

Findings (all non-blocking — do NOT fix this slice):
- minor (observability) — `src/indexer/mod.rs:82-84` — D2 swallows the per-file error silently
  (`Err(_skipped) => {}`); IndexStats has no `errors`/`skipped` field, so a repo where half the
  files fail to index is indistinguishable from a clean run at the API boundary. Acceptable for M5.2
  (no scenario observes it, and the brief notes it), but flag for a follow-up: add a
  `files_skipped` counter to IndexStats and/or `log::warn!` the skipped path+error so real bugs
  aren't masked. Recommend the manager log this as an M5.3/M7 follow-up.
- minor (test coverage gap) — `tests/chunker_tests.rs:191` — the only cross-reference test asserts a
  single bare call (`hash_password`); there is no test exercising nested-function attribution or
  dedup (a name called twice → listed once). The refactor is byte-identical so this is not a
  blocker, but the cross-ref behavior the refactor touches is under-covered. Recommend a follow-up
  RED test (nested call + duplicate call) to lock the dedup/first-seen contract independently of the
  implementation. Logged, not gating.
- minor (doc nit) — `benches/indexing.rs:4` — header comment says "50 files × ~6 functions each ≈
  300 functions, roughly 1 500 LOC", contradicting line 51/72 and `py_function` (2 functions/file,
  ~500 LOC). Cosmetic; align the header to the actual ~500 LOC / 2-fn-per-file figure.

Slice M5.2 is DONE-eligible. Hand back to manager.

### M5.3 — incremental + idempotency + delete (2026-06-10) — **APPROVE**

Reviewed: `src/indexer/mod.rs` (index_all rewrite, update_files, reindex_each, restamp_index_state),
`src/indexer/pipeline.rs` (detect_changed_files, reindex_file, the #[cfg(test)] unit test),
`src/storage/mod.rs` + `queries.rs` (delete_file_meta, all_indexed_files), `tests/indexer_tests.rs`
(5 new tests), `src/indexer/CLAUDE.md`, `src/storage/CLAUDE.md`, `docs/project_plan.md` §3.2.2.
Re-ran: indexer_tests 15/15, clippy --all-targets -D warnings (clean), fmt --check (clean).

**Verdict: APPROVE.** Correct, idiomatic, aligned; no blockers, no majors.

Correctness confirmed (highest-scrutiny items):
- **Idempotency / no-writes (test #1).** `detect_changed_files` reads `get_file_hash` and compares
  to `compute_file_hash`; equal ⇒ the file is NOT in the `reindex_each` set, so no
  delete_chunks_for_file / insert_chunks / update_file_hash fires. CRITICAL equality holds: the
  stored `files_metadata.content_hash` IS exactly what `compute_file_hash` returns — `index_file`
  builds FileMeta.content_hash from the same `compute_file_hash(path)` call and stores it via
  `update_file_hash`, so the second run's recompute (same content + same mtime → xxh3 over
  content+mtime.to_le_bytes()) yields the identical 32-hex string. No mtime read-skew risk: both
  write-time and compare-time mtime come from `fs::metadata(path).modified()` truncated to whole
  epoch seconds, and the file is untouched between runs, so the seconds value is stable. Verified
  by the unit test `detect_changed_files_empty_for_unchanged_repo` and the integration invariants.
- **restamp on the unchanged run is value-stable.** `restamp_index_state` recomputes
  total_files/total_chunks from `files_metadata` (unchanged on a skip run), so it rewrites the same
  two `index_state` values — test #1's value-equality assertions pass. Re-stamping two scalar rows
  every `index_all` is a strictly-bounded write (2 rows, independent of repo size) and does not
  touch `symbols`/`files_metadata`, so it does not violate the load-bearing no-writes intent (no
  chunk churn, no FileMeta re-stamp, no rowid changes). Acceptable; logged as a nit below.
- **Reconcile path equality (test #5) is sound.** Both sides of the `on_disk.contains(&stored)`
  check use the same canonical string form: stored paths are written as `path.to_string_lossy()`
  of the absolute-under-root path `discover_files` returned; `all_indexed_files()` reconstructs
  `PathBuf::from(that_string)`; the `on_disk` HashSet holds the very same `discover_files` output
  this run. Same origin ⇒ component-wise PathBuf equality matches. No risk of failing-to-reconcile
  or wrongly-deleting a live file. delete_chunks_for_file + delete_file_meta both run for an evicted
  path, then restamp drops totals to survivors (recompute-from-table, cannot drift) — totals 1/1.
- **update_files (tests #2/#3).** hash-filter via detect_changed_files; reindex_file does
  delete_chunks_for_file BEFORE index_file, so old symbols are replaced not duplicated;
  files_processed counts only re-indexed files. Untouched sibling's hash/symbol unchanged (D2 +
  per-file isolation preserved). Test #3 changes all 3 ⇒ files_processed == 3.
- **New file (test #4).** No stored hash ⇒ detect_changed_files flags it changed ⇒ indexed;
  pre-existing file is unchanged (skipped, survives); totals → 2.
- **Storage additions.** delete_file_meta: prepared SQL, params-bound, unknown path = 0-row no-op
  (correct, test #5's surviving-file case relies on it not over-deleting). all_indexed_files:
  prepare_cached + query_map, typed `?` propagation, no reachable unwrap/expect/panic, style matches
  existing methods. queries.rs constants documented. plan §3.2.2 edit is accurate and minimal
  (adds exactly the two signatures with rationale comments).
- **Rules.** No reachable unwrap/expect/panic in production; IndexError typed with source() chain;
  Result+? / map_err throughout. clippy --all-targets -D warnings clean; fmt clean.
- **Carry-over (M5.2 follow-up).** reindex_each inherits the same silent D2 swallow
  (`Err(_e) => stats.files_skipped... ` — there is still no files_skipped counter and the error is
  dropped). This is the already-logged M5.2 non-blocking follow-up now also covering reindex_each —
  NOT a new regression. Re-logged below for the manager.

Tests: genuine, not weakened. Real searchable-symbol set comparisons (exact vec equality), FileMeta
field asserts, index_state totals parsed to usize, before/after invariants. The new pipeline unit
test locks the no-write predicate at the level the integration test cannot reach. Deterministic
(sorted/deduped). No assertion relaxed to is_ok().

Findings (all non-blocking nits — do NOT fix this slice):
- minor (carry-over, observability) — `src/indexer/mod.rs` reindex_each `Err` arm — per-file D2
  failures are still counted-as-skipped silently with no IndexStats.files_skipped field and no
  log::warn. Same follow-up flagged in M5.2 review; now also applies to the incremental path. A repo
  where some files fail to (re)index looks identical to a clean run at the API boundary. Recommend
  the manager keep the single open follow-up (add files_skipped + a warn-log) for M5.4/M7.
- minor (efficiency) — `src/indexer/mod.rs` restamp_index_state — recomputes totals via
  all_indexed_files() then a per-path get_file_meta (N+1 queries) where a single SELECT COUNT(*) /
  SUM(chunk_count) FROM files_metadata would do. Correct and bounded for M5 scale; the `if let
  Some(meta)` guard is dead-defensive (every all_indexed_files() path has a meta row by
  construction). Optional storage helper (e.g. `index_totals()`) could fold this into one query if
  M10 perf wants it. No action this slice.
- minor (no-op write note) — restamp runs on every index_all/update_files including pure-skip runs,
  rewriting the two index_state scalars even when nothing changed. Harmless (test #1 asserts value
  not write-count; 2 fixed rows), but if a stricter "zero writes on unchanged" guarantee is ever
  wanted, gate the restamp behind `!changed.is_empty() && no reconcile deletions`. Note only.

Slice M5.3 is DONE-eligible. Hand back to manager.

### M5.4 — e2e init → index (2026-06-10) — engineering lead

**Implemented** (pure thin glue; matches the RED-pinned signatures exactly — no plan deviation):
- `src/app.rs` (**new**) — the public application facade, `#[cfg(test)] mod tests {}` empty
  (e2e lives in `tests/e2e_index.rs`):
  - `pub fn init(project_root: &Path) -> Result<(), AppError>` — `create_dir_all(<root>/.codecache/)`
    → write `toml::to_string(&Config::default())` to `<root>/.codecache/config.toml` **only if the
    file does not already exist** (the non-clobbering guard) → `Config::load` the now-present config
    → resolve db_path → `open_storage(db_path)?.init_schema()` (idempotent `CREATE ... IF NOT
    EXISTS`). Re-`init` never errors and never rewrites an existing config.
  - `pub fn index(project_root: &Path) -> Result<IndexStats, AppError>` — `Config::load(
    <root>/.codecache/config.toml)` → `open_storage(resolved db_path)` → `Indexer::new(config,
    storage, root.to_path_buf())` → `index_all()` → return stats. No new indexer logic — `index_all`
    is already incremental+reconcile from M5.3, so test #3's second `index` re-indexes the changed
    file and drops the old symbol unchanged.
  - private helpers `resolve_db_path`, `open_storage`, `create_dir` keep the two entry points small.
- `src/lib.rs` — added `pub mod app;` and crate-root re-exports
  `pub use app::{index, init, AppError};` + `pub use indexer::IndexStats;`, so the e2e imports
  `codecache::{index, init, AppError, IndexStats}` (and `codecache::storage::Storage`) all resolve.

**AppError shape** (new top-level typed enum; `impl Display + std::error::Error` with a `source()`
chain; no reachable `unwrap()/expect()/panic!`):
```rust
pub enum AppError {
    Config(ConfigError),     // config load/serialize failure
    Storage(StorageError),   // DB open / init_schema failure
    Index(IndexError),       // indexing run failure
    Io { path: PathBuf, source: std::io::Error },  // create-dir / write-config (+ TOML serialize)
}
```
`Config`/`Storage`/`Index` wrap the three sub-errors per the pinned three-way shape; an extra `Io`
variant carries the filesystem context (`create_dir_all`/config-write) so those failures aren't
flattened into a sub-error they don't belong to. TOML-serialize failure (infallible for the static
`Config::default`, but surfaced rather than unwrapped) is mapped into `Io` via
`std::io::Error::other`. `source()` returns the wrapped sub-error / `io::Error` for each variant,
so the e2e's `assert_error::<AppError>()` (`AppError: std::error::Error`) holds and the chain is
inspectable.

**DB-path resolution (matches the pinned contract):** `resolve_db_path(root, config) =
root.join(&config.storage.db_path)`. For the default config (`db_path = ".codecache/index.db"`)
this is `<root>/.codecache/index.db` — exactly the path the e2e re-opens via `Storage::new`. An
absolute `db_path` is returned unchanged by `Path::join`. `open_storage` defensively
`create_dir_all`s the db's parent before `Storage::new` (Connection::open won't create missing
parents); `init` already creates `.codecache/` first, so this is belt-and-suspenders.

**Idempotency guard:** `init` writes the config behind an `if !config_path.exists()` check, so a
second `init` leaves the existing `config.toml` byte-for-byte untouched (test #2 asserts the bytes
are identical across two inits). `init_schema()` is independently idempotent, so re-creating
`Storage` and re-calling it is a safe no-op — `init` is callable any number of times without error
or data loss.

**Note for manager:** `tests/e2e_index.rs` (the RED file) was not rustfmt-clean (three
calls/`let`s wrapped by fmt). I ran `cargo fmt --all` to satisfy the fmt gate; the change is
**whitespace/line-wrapping only** — the fixture string literals (`"def authenticate_user():\n..."`)
and every assertion are byte-identical, no test weakened/skipped/deleted. This matches the
project's fmt-on-edit gate that formats every `.rs`. All 4 e2e tests pass.

**Gate output (PATH-prefixed `$HOME/.cargo/bin`, all four green):**
```
cargo build                                  → Finished (clean)
cargo clippy --all-targets -- -D warnings    → Finished (no warnings)
cargo test --all                             → all green; e2e_index 4/4
cargo fmt --all -- --check                   → clean (exit 0)
```
`tests/e2e_index.rs`: 4 passed / 0 failed. Whole suite: **96 tests** total (lib 15, chunker 10,
chunker_proptest 3, config 5, **e2e_index 4**, hasher 11, indexer 15, parser 14, smoke 1,
storage 18; main 0, doctests 0) — up from 92 by exactly the 4 new M5.4 tests. M5.1–M5.3 +
storage/config all unchanged and green.

Hand off to **code-reviewer**.

### M5.4 — e2e init → index (2026-06-10) — **APPROVE** (runner-performed; reviewer agent hit a session limit)

Reviewed `src/app.rs` (new), `src/lib.rs` (module decl + re-exports), `tests/e2e_index.rs` (4 tests),
`src/indexer/CLAUDE.md`. Gates independently re-verified green by the runner (build, clippy
--all-targets -D warnings, test --all = 96, fmt --check).

**Verdict: APPROVE.** Pure thin glue, correct and aligned; no blockers.

Correctness confirmed:
- **`init` idempotency.** Config write guarded by `if !config_path.exists()` ⇒ a second `init` leaves
  `config.toml` byte-identical (test #2); `init_schema` is independently idempotent. Re-init never
  errors, never clobbers.
- **db_path consistency.** `init`, `index`, and the test's `default_db_path` all resolve to
  `<root>/.codecache/index.db` via `project_root.join(config.storage.db_path)` — the test re-opens
  the same DB `index` wrote, so a path mismatch would (and does not) surface. `open_storage`
  defensively `create_dir_all`s the db parent before `Storage::new`.
- **`AppError`.** Typed enum (Config/Storage/Index + an Io variant for create-dir/write-config);
  `Display` + `std::error::Error` with a correct `source()` chain; no reachable unwrap/expect/panic.
  Folding the (infallible-for-default-Config) TOML-serialize error into `Io` via `io::Error::other`
  is acceptable — surfaced not unwrapped.
- **Public-surface hygiene.** Crate-root re-exports `pub use app::{index, init, AppError}` +
  `pub use indexer::IndexStats` give M7/M8 a coherent library entry point; no leaked internals.
- **Tests genuine.** 4 e2e tests use the public surface only, assert real outcomes (dirs/files exist,
  IndexStats counts 3/5, symbol searchable in the re-opened DB), deterministic (sorted/deduped);
  test #3 exercises M5.3 incremental reconcile through the facade.

Findings (non-blocking, fixed by runner): two rustdoc lines in `src/app.rs` (`init` doc + `create_dir`
doc) said filesystem failures map to `AppError::Config` when they actually map to `AppError::Io` —
corrected during closeout (doc-only).

Slice M5.4 DONE-eligible.

## OUTCOME — manager (2026-06-10)

**M5 — indexer: COMPLETE.** All four slices RED→GREEN→reviewer-APPROVED and committed one-per-slice.

- **Aligned:** yes. API matches `project_plan.md` §3.2.4 (Indexer/IndexStats, extended doc-first with
  the `root` param) + §3.2.2 (storage `delete_file_meta`/`all_indexed_files`, added doc-first) + §5.1
  (full index) + §5.2 (incremental + reconcile). D2 (degrade-and-continue) and D3 (enrichment
  flow-through) honored.
- **Commits:** M5.1 `ef36942`, M5.2 `8482f05`, M5.3 `707daba`, M5.4 (this slice). Per-slice as
  recommended; the M4 cross-ref single-pass perf fix rode in M5.2.
- **Tests:** 96 total, all four gates clean on Rust 1.85.0. New: indexer 15 + e2e 4 + 1 pipeline unit.
- **Docs updated (same change set):** `docs/TODO.md` Phase 5 (all slices checked, milestone marked
  DONE), `src/indexer/CLAUDE.md`, `src/storage/CLAUDE.md`, `src/CLAUDE.md` (+`app` row), `src/lib.rs`
  module map (+`app`, refreshed stale "M0 skeleton" line), `benches/CLAUDE.md`,
  `docs/TEST_STRATEGY.md#indexer`, `docs/project_plan.md` §3.2.2/§3.2.4.
- **Decision honored:** `is_heuristic` persistence deferred to M7 (no M5 scenario observes it; un-driven
  schema migration avoided per TDD). Indexer passes the flag in-memory; stored repr drops it (unchanged
  from M4).

**Open follow-ups carried to M6/M7/M10 (all non-blocking, logged in `docs/TODO.md` Phase 5):**
1. **Observability** — D2 silently swallows per-file errors in `index_all`/`update_files`; `IndexStats`
   has no `files_skipped` counter. Add the counter (+`log::warn!` of path+error) so a run where many
   files fail isn't indistinguishable from a clean run. → engineering-lead.
2. **Cross-ref test coverage** — add a RED test for nested-function call attribution + duplicate-call
   dedup (first-seen) to lock the contract independently of the single-pass impl. → test-lead.
3. **Perf (M10)** — cold-index bench baseline ~1.1s/500 LOC; naive extrapolation exceeds the §5.4
   10K-LOC budget. Profile the per-file transaction overhead (`insert_chunks`/`update_file_hash` once
   per file) before M10; consider `restamp_index_state` N+1 → single `SELECT COUNT/SUM`. → perf.

Next milestone: **M6 — retriever** (BM25 + snippet + token budget). Build order unblocked (M5 done).
