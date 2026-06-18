# src/indexer/ — CLAUDE.md

**Module:** `indexer` · **Owner:** `principal-engineering-lead` · **Milestone:** M5 (stub at M0).

## Purpose
Orchestrate the indexing pipeline: file discovery (honoring `.gitignore` + extra ignore
patterns) → parse → chunk → hash → store. Incremental: only changed files re-indexed; deleted
files' chunks removed; re-index of unchanged input is a no-op (idempotent).

## API anchor
`docs/project_plan.md` §3.2.4 (`Indexer`, `IndexStats`) + §5.1/§5.2 (algorithms).

## Tests / scenarios
`docs/TEST_STRATEGY.md#indexer` — discovery honors ignores; full index populates storage;
incremental idempotency; modify N ⇒ exactly N re-indexed; delete removes chunks.

## Shipped API (M5.1 — discovery + language detection)
- `discovery.rs` (re-exported from `mod.rs`):
  - `detect_language(path: &Path) -> Option<Language>` — by extension (`.py`/`.ts`/`.go`); all
    else → `None`.
  - `discover_files(config: &Config, root: &Path) -> Result<Vec<PathBuf>, IndexError>` — walks
    `config.index_paths` joined under `root` (empty ⇒ walk `root`) via `ignore::WalkBuilder`,
    `.require_git(false)` so `.gitignore` is honored outside a checkout. `config.ignore_patterns`
    applied as gitignore-style globs (`ignore::gitignore::GitignoreBuilder` anchored at `root`,
    `matched_path_or_any_parents`). Results restricted to files whose `detect_language` ∈
    `config.languages`. Paths returned are absolute-under-`root`.
- `mod.rs` — `IndexError { Io { path, source }, Glob { pattern, source } }`, typed (`impl
  std::error::Error` + `source()`), no reachable `unwrap()/expect()/panic!`.

## Shipped API (M5.2 — full index `index_all`)
- `mod.rs` — `Indexer` facade:
  - `Indexer::new(config: Config, storage: Storage, root: PathBuf) -> Result<Indexer, IndexError>`
    — `root` is an explicit 3rd arg (extends §3.2.4's `new(config, storage)`; plan §3.2.4 updated
    to match). Builds the reusable Tree-sitter `Parser` once.
  - `Indexer::index_all(&mut self) -> Result<IndexStats, IndexError>` — §5.1: `discover_files` →
    per file `pipeline::index_file` → accumulate `IndexStats` → `set_index_state("total_files"/
    "total_chunks")` (decimal strings) → `duration_ms` via `std::time::Instant`.
  - `IndexStats { files_processed, chunks_indexed, duration_ms }` (`Copy`, `Default`).
- `pipeline.rs` — `index_file(parser, storage, path) -> Result<usize, IndexError>`: §5.1 step
  3a–3e (hash → read content+metadata → `detect_language` → `parse_file` → `chunker::chunk` →
  stamp `file_path` on chunks → `insert_chunks` → build `FileMeta{content_hash, mtime, file_size,
  language, chunk_count}` → `update_file_hash`). Returns chunk count.
- `IndexError` extended with per-file/store variants: `File{path,source}`, `Hash`, `Parser`,
  `Chunker`, `Storage` (in addition to M5.1 `Io`/`Glob`). Typed, `impl Error` + `source()` chain.

### D2 per-file isolation (batched per D20 — see below)
Each changed/new file's per-file work runs inside its own **SAVEPOINT** within the run's single
outer transaction (`reindex_each` → `Storage::write_in_transaction`): on success the savepoint is
released; on a per-file failure (read/parse/chunk/store) it is rolled back ONLY for that file and the
file is **counted-as-skipped**, while the committed siblings survive the single outer commit. The
batch never aborts on one bad file — `index_all` returns `Ok`. The chunker already degrades a
malformed tree internally (heuristic fallback / empty via `error_rate`), so a syntactically broken
file usually returns `Ok(0..)`; a read-stage error (unreadable/invalid-UTF-8 file) or store failure
is isolated by the savepoint. Only non-isolatable failures (discovery, the outer begin/commit, a
savepoint begin/release/rollback, a poisoned lock, the `index_state` totals write) propagate as
`Err`. Guards: parse-stage `malformed_file_in_repo_does_not_abort_index` + read-stage
`unreadable_file_mid_batch_does_not_discard_committed_siblings`.

### D20 — cold-index transaction batching (2026-06-17)
The old per-file `index_file` (its own `insert_chunks` transaction + autocommit `update_file_hash`)
paid ~N commit fsyncs for an N-file index — the M10.1 10K-cold-index miss (6.04 s vs < 5 s). The
indexer now drives all changed/new files through ONE `Storage::write_in_transaction` call (plan
§3.2.2): `pipeline::reindex_file_batched(parser, &BatchWriter, path)` does delete-first → insert →
`update_file_hash` inside the file's savepoint, and `pipeline::extract_file` is the shared read-only
half (hash → read → parse → chunk → stamp `file_path`) that does no DB writes. `reindex_each` now
returns `Result<IndexStats, IndexError>` (was infallible) and maps each per-file `IndexError` to a
savepoint-rollback signal via the internal `index_error_as_storage_signal`; `index_all`/`update_files`
`?` it. `detect_changed_files` still runs BEFORE the batch, so an unchanged file opens no savepoint
and is not re-stamped (idempotency held). Measured on this WSL2/Linux machine: 10K cold-index p50
5.84 s → 1.37 s (−76.5%), well under < 5 s here (Windows CI is the authoritative budget gate). Brief:
[`.claude/briefs/BRIEF-M10-D20-batch-inserts.md`](../../.claude/briefs/BRIEF-M10-D20-batch-inserts.md).

## Shipped API (M5.3 — incremental + idempotency + delete)
- `pipeline.rs`:
  - `detect_changed_files(storage, &[PathBuf]) -> Result<Vec<PathBuf>, IndexError>` — returns the
    candidates whose `hasher::compute_file_hash` differs from the stored `get_file_hash` (new files
    have no stored hash ⇒ changed). Unchanged files are skipped — this is the no-write predicate.
    A file whose hash can't be computed is treated as changed (so the caller's D2 path handles it).
  - `reindex_file(parser, storage, path) -> Result<usize, IndexError>` — `delete_chunks_for_file`
    first (no stale/duplicate chunks), then the normal `index_file` path (re-parse → re-chunk →
    `insert_chunks` → `update_file_hash`).
- `mod.rs`:
  - `Indexer::update_files(&mut self, files: &[PathBuf]) -> Result<IndexStats, IndexError>` —
    `detect_changed_files` over the explicit list → `reindex_each` (delete-first, D2-isolated) →
    `restamp_index_state`. `files_processed` = files actually re-indexed.
  - `Indexer::index_all` is now **incremental + reconcile** on a populated DB: skip unchanged (no
    writes), re-index changed/new, then reconcile deletions (every `all_indexed_files()` path not in
    the discovered set ⇒ `delete_chunks_for_file` + `delete_file_meta`), then `restamp_index_state`.
  - private `reindex_each` (accumulate stats over a delete-first re-index) + `restamp_index_state`
    (recompute `total_files`/`total_chunks` from `files_metadata` so totals never drift).
- **Idempotency / no-write guarantee:** an unchanged file fails the `detect_changed_files` hash
  compare, so it is never in the `reindex_each` set — no `delete_chunks_for_file`, no `insert_chunks`,
  no `update_file_hash` re-stamp. The stored hash, `FileMeta`, and chunk rowids are untouched. Note
  the stored `content_hash` IS `compute_file_hash` (content+mtime, same 32-hex format), so a second
  unchanged run compares equal. Locked at unit level by `pipeline::tests::
  detect_changed_files_empty_for_unchanged_repo`.
- **Storage additions (M5.3, plan §3.2.2 updated):** `Storage::delete_file_meta(&Path)` and
  `Storage::all_indexed_files() -> Vec<PathBuf>` — internal CRUD symmetric with the existing
  `delete_chunks_for_file`/`update_file_hash`, used by the reconcile + restamp paths.
- Slices M5.1–M5.4 + execution sequence: [`.claude/briefs/BRIEF-M5-indexer.md`](../../.claude/briefs/BRIEF-M5-indexer.md).

## Decisions / seams
- **`is_heuristic` persistence — deferred to M7.** M5 passes the chunker's `is_heuristic` through
  in-memory to `insert_chunks`, but the M1 `symbols` schema has no column for it, so the stored
  representation drops it (round-trip reconstructs `false`, unchanged from M4). No M5 scenario
  observes it; persistence is driven by an M7 formatter/CLI RED test (storage adds an UNINDEXED
  column + version migration). See BRIEF §Follow-ups (b).
- **M4 cross-ref re-walk fix** — DONE in M5.2: `chunker` now collects every bare-identifier `call`
  in a single DFS walk (`collect_calls`) into a `Vec<CallSite>`, then each chunk's
  `call_names_in_span` filters that pre-collected slice by span (O(nodes + chunks·calls) vs the old
  O(chunks × tree_nodes)). Public `chunk()` signature + observable output (deduped, first-seen DFS
  order) unchanged; M4 chunker tests (10 + 3 proptest) stay green.

## Shipped API (M5.4 — e2e init → index)
The thin public `init`/`index` library facade lives in **`src/app.rs`** (single-file module; doc in
the file header), re-exported at the crate root in `src/lib.rs`:
- `codecache::init(project_root: &Path) -> Result<(), AppError>` — `create_dir_all(.codecache/)` →
  write `toml::to_string(&Config::default())` to `.codecache/config.toml` **only if absent**
  (non-clobbering) → `init_schema()` the DB at the resolved `db_path`. Idempotent: re-init never
  errors and never rewrites an existing config.
- `codecache::index(project_root: &Path) -> Result<IndexStats, AppError>` — `Config::load` →
  `Storage::new(resolved db_path)` → `Indexer::new(config, storage, root)` → `index_all()`. Pure
  glue; relies on M5.3 incremental+reconcile.
- DB-path resolution: `project_root.join(&config.storage.db_path)` ⇒ `<root>/.codecache/index.db`
  for the default config. `open_storage` `create_dir_all`s the db parent before `Storage::new`.
- `codecache::AppError { Config(ConfigError), Storage(StorageError), Index(IndexError), Io{path,
  source} }` — typed, `impl Display + std::error::Error` with `source()` chain; no reachable panic.
- Crate-root re-exports: `pub use app::{index, init, AppError};` + `pub use indexer::IndexStats;`.

## Status
**M5.4: GREEN (2026-06-10)** — public `init`/`index` facade (`src/app.rs`) + `AppError` shipped;
4/4 `e2e_index` tests green (init creates `.codecache/`+config+DB; index populates a queryable DB
with correct `IndexStats`; idempotent re-init; reindex-after-modification). **96 tests total**, all
four gates clean (Rust 1.85). M5.1–M5.4 complete pending review. Brief:
[`.claude/briefs/BRIEF-M5-indexer.md`](../../.claude/briefs/BRIEF-M5-indexer.md).

**M5.3: GREEN (2026-06-10)** — incremental `update_files` + idempotent/reconciling `index_all`
(skip-unchanged no-writes, re-index changed/new, reconcile deletions, restamp totals) shipped.
15/15 `indexer_tests` (5 M5.1 + 5 M5.2 + 5 M5.3) + 1 new `pipeline` unit test; **92 tests total**,
all four gates clean (Rust 1.85). Brief:
[`.claude/briefs/BRIEF-M5-indexer.md`](../../.claude/briefs/BRIEF-M5-indexer.md).
