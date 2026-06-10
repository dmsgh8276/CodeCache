# BRIEF — M5 / indexer (M5.1–M5.4)

- **Milestone:** M5 — indexer (discovery → parse → chunk → hash → store; incremental)  ·  **Module(s):** `indexer` (+ thin `init`/`index` glue)
- **Owner (manager):** principal-engineering-manager  ·  **Created:** 2026-06-10
- **Status:** RED ▢  GREEN ▢  REVIEW ▢  DONE ▢
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
