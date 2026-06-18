# src/storage/ — CLAUDE.md

**Module:** `storage` · **Owner:** `principal-engineering-lead` + `rust-treesitter-specialist`
(FTS5 tuning) · **Milestone:** M1 (stub at M0).

## Purpose
SQLite interface: create/migrate schema (`symbols` FTS5, `files_metadata`, `index_state`),
insert/query/delete chunks, BM25 search. `Storage` wraps `Arc<Mutex<Connection>>` (**D8**) so it
is cheaply `Clone`-able and the MCP server can lend one connection to `Retriever`/`Indexer`.

## API anchor
`docs/project_plan.md` §3.2.2 (API) + §4.1 (schema). Honors **D6** (`update_file_hash(path,
&FileMeta)`) and **D7** (`start_line`/`end_line` UNINDEXED columns; D3 enrichment columns indexed).

## Tests / scenarios
`docs/TEST_STRATEGY.md#storage-sqlite--fts5` — idempotent schema; round-trip CRUD; `MATCH` +
`bm25()` ordering; corrupt/locked DB → error not panic; empty-DB query → empty result.

## Shipped API (M1)
- `Storage { conn: Arc<Mutex<Connection>> }` (D8), `#[derive(Clone)]` (clones share one conn).
- `new(&Path)`, `init_schema()` (idempotent; migrates older `index_state.version` forward),
  `insert_chunks(&[Chunk])` (single transaction batch), `delete_chunks_for_file(&Path)`,
  `search(&str, usize) -> Vec<SearchResult>` (BM25 best-first, deterministic `rowid` tie-break),
  `get_file_hash`/`get_file_meta`, `update_file_hash(&Path, &FileMeta)` (D6 upsert),
  `get_index_state`/`set_index_state`.
- **M5.3 additions** (deletion reconciliation, plan §3.2.2 updated): `delete_file_meta(&Path)`
  (drop a `files_metadata` row — symmetric with `delete_chunks_for_file`; unknown file = no-op),
  `all_indexed_files() -> Vec<PathBuf>` (enumerate every indexed path — drives the indexer's
  on-disk-vs-known reconcile and the DB-wide totals recompute).
- **M8.3 addition** (**D19**, plan §3.2.2): `symbols_for_path(&Path) -> Vec<SymbolOutline>` — the
  `codecache_outline` lookup. A plain parameterized column `SELECT` off the contentful `symbols`
  table `WHERE file_path = ?1 OR file_path LIKE ?2 ESCAPE '\'` (exact file OR `<dir>/%` directory
  prefix; the path's literal `%`/`_`/`\` are escaped by the private `escape_like` helper so a path
  with a wildcard char never over-matches), ordered `(file_path, start_line, end_line)`. Returns the
  slim `SymbolOutline` (name/type/parent/path/start_line/end_line) — **zero source reads** (D7), no
  `chunk_text`. Unknown path → empty `Vec`; corrupt `symbol_type` → `CorruptRow`, never a panic.
- **R2.2a addition** (**D24**, plan §3.2.2): `search_with_weights(&str, usize, Option<&[f64; 7]>) ->
  Vec<SearchResult>` — `search` with caller-supplied per-column BM25 weights (one f64 per indexed FTS5
  column, `schema::CREATE_SYMBOLS` order). `None` reuses the cached `queries::SEARCH` verbatim (default
  path byte-identical; `search` now delegates here with `None`). `Some(w)` builds the same statement
  via `queries::search_with_weights_sql(w)` with the 7 weights **formatted** into the `bm25(symbols,
  …)` expression — FTS5 `bm25()` weights are auxiliary-fn args that **cannot** be bound as `?` params,
  so they are rendered as numeric literals (`{:?}` on f64 → always a decimal point, locale-free;
  injection-safe because each is a finite f64, never raw text). `MATCH ?1`/`LIMIT ?2` stay bound; the
  weighted SQL is dynamic ⇒ `prepare` (not `prepare_cached`). FTS5 accepts zero/negative weights (rank
  normally); a non-finite weight (NaN/±inf) is rejected as `StorageError::NonFiniteWeight` (defensive —
  the CLI validates first) rather than emitted into SQL. Ordering invariant unchanged (`bm25 ASC, rowid
  ASC`); reuses `map_search_row`/`RawSearchRow`.
- **D20 addition** (cold-index batching, plan §3.2.2): `write_in_transaction<T, F>(items: &[T], each:
  F) -> Result<Vec<Result<()>>>` where `F: FnMut(&BatchWriter<'_>, &T) -> Result<()>` — runs `each`
  once per item inside **one outer `conn.transaction()`** (the connection is locked ONCE — D8), each
  item wrapped in its own **SAVEPOINT** (`tx.savepoint()`): on `Ok` the savepoint is RELEASED (commit),
  on `Err` it is ROLLED BACK TO (the item's partial writes discarded) then released and the `Err`
  recorded, and the batch CONTINUES. The outer tx commits once. Returns one inner `Result` per item
  (same order/length); the OUTER `Result` is `Err` only for a non-isolatable failure (outer begin/
  commit, poisoned lock). This amortizes ~N per-file commit fsyncs (the old per-file
  `insert_chunks`+autocommit `update_file_hash`/`delete_chunks_for_file`) into ONE commit — the D20
  10K-cold-index fix — while keeping **D2 per-file isolation** (one item's DB error rolls back only
  that item). The indexer drives the delete-first → insert → upsert per-file unit through it.
  - `BatchWriter<'a>` lends `insert_chunks`/`delete_chunks_for_file`/`update_file_hash`, each executing
    against the CURRENT savepoint by borrowing the open transaction (a `rusqlite::Savepoint` derefs to
    `Connection`) — it does NOT re-lock `Storage` (no re-entrant-lock deadlock, D8). The autocommit
    `insert_chunks`/`delete_chunks_for_file`/`update_file_hash` stay for single-shot callers (e.g.
    `app::ingest_chunks`); both paths delegate to shared private `*_on(&Connection, …)` helpers so the
    rows written are byte-identical whether autocommit or savepoint.
- `SearchResult { chunk, bm25_score }`. `StorageError::{Sqlite, LockPoisoned, CorruptRow,
  NonFiniteWeight, BatchItem}` (typed, impl `std::error::Error`; no reachable panic — poisoned lock,
  unknown stored enum, a non-finite BM25 weight, and a rolled-back batch item are all typed errors).
  **D20** added `BatchItem(String)`: the storage-layer signal a `write_in_transaction` item maps a
  non-storage per-item failure into so its savepoint rolls back; carries the underlying error's
  description and is never surfaced to the user (the indexer just counts the file skipped, D2).

## Schema / FTS5 notes (`schema.rs`, `queries.rs`)
- Default (contentful) FTS5 `symbols` table — **D11** drops the invalid `content='symbols'` from
  §4.1; FTS5 stores + returns all columns, so chunks round-trip without a companion table.
- Indexed (D3): symbol_name, symbol_type, chunk_text, parent_symbol, imports, cross_references,
  file_docstring (last indexed column — a term only in the module docstring is matchable).
  UNINDEXED: file_path, start_byte, end_byte, start_line, end_line (D7), language.
- `imports`/`cross_references` stored as `\n`-joined text (FTS5 has no array type).
- `tokenize='unicode61 remove_diacritics 2'`; BM25 per-column weights (one per indexed column,
  7 total) weight `symbol_name` highest (10.0), `parent_symbol` 5.0, the rest 2.0/1.0;
  `file_docstring` weighted 2.0. `ORDER BY bm25 ASC, rowid ASC`. These are the **defaults** baked
  into `queries::SEARCH`; R2.2a (D24) lets a caller override them per-query via `search_with_weights`
  (`queries::search_with_weights_sql` renders the override; `fmt_weight` is the finite-f64→SQL-literal
  helper). The default `SEARCH` literal stays the `None`/default path (byte-identical).

## Status
**M1: DONE (2026-06-10).** All four gates green on Rust 1.85.0 (18 storage tests pass).
**R2.2a / D24 (2026-06-14):** `search_with_weights` added (per-column BM25 override; `search`
delegates with `None`). +4 storage tests (reorder / default-identical / determinism / zero-negative
edge); all 25 storage tests green; all four gates clean (Rust 1.85).
**D20 (2026-06-17):** `write_in_transaction` + `BatchWriter` savepoint primitive added (cold-index
batching; `insert_chunks`/`delete_chunks_for_file`/`update_file_hash` refactored onto shared `*_on`
helpers so autocommit + savepoint write identical rows); `StorageError::BatchItem`. +3 storage tests
(`write_in_transaction_*`: happy-path commit-all, mid-batch savepoint isolation, failed-first-item
survivor commit). All four gates clean (Rust 1.85); reviewer APPROVED. Brief:
[`.claude/briefs/BRIEF-M10-D20-batch-inserts.md`](../../.claude/briefs/BRIEF-M10-D20-batch-inserts.md).
