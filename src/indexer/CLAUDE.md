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

### D2 per-file isolation
`index_all` wraps each `index_file` call in a `match`: on `Ok(n)` it adds to the stats; on `Err`
it counts the file as skipped and continues. The batch never aborts on one bad file — `index_all`
returns `Ok`. The chunker already degrades a malformed tree internally (heuristic fallback / empty
via `error_rate`), so a syntactically broken file usually returns `Ok(0..)`; any residual per-file
error (unreadable, store failure) is still caught here. Only non-isolatable failures (discovery,
the `index_state` totals write) propagate as `Err`.

## Planned layout (M5.3+)
- `pipeline.rs` — change detection (§5.2): `detect_changed_files` via `compute_file_hash` vs
  `get_file_hash`; `update_files` for an explicit list; deletion reconciliation vs `files_metadata`.
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

## Status
**M5.2: GREEN (2026-06-10)** — full index (`index_all`) + per-file `pipeline.rs` shipped; D2
isolation + chunker single-pass cross-ref refactor landed. 10/10 `indexer_tests` (5 M5.1 + 5
M5.2), 86 tests total, all four gates clean (Rust 1.85). M5.3–M5.4 (incremental/delete + e2e)
pending. Brief: [`.claude/briefs/BRIEF-M5-indexer.md`](../../.claude/briefs/BRIEF-M5-indexer.md).
