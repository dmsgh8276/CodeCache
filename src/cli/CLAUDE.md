# src/cli/ — CLAUDE.md

**Module:** `cli` · **Owner:** `principal-engineering-lead` · **Milestone:** M7 (stub at M0).

## Purpose
`clap`-based argument parsing and command dispatch: `init`, `index`, `update`, `query`,
`status`, `config`, `serve`. User-facing errors with helpful messages + nonzero exit.

## API anchor
`docs/project_plan.md` §7 (command structure + per-command specs).

## Shipped surface (M7.2 — parsing)
`clap` derive in `mod.rs`: `Cli` (global `-v/--verbose`, `-V/--version` from `CARGO_PKG_VERSION`,
`-h/--help`) + a `Command` subcommand enum mirroring §7.1–§7.2 EXACTLY:
- `init` — `--db-path` [default `.codecache/index.db`], `--index-path` (multi, default `.`),
  `--ignore` (multi), `--languages` (comma-delimited, default `python,typescript,go`)
- `index` — `--full`, `--db-path`, `--progress`
- `update <FILE>...` (required positional) — `--db-path`
- `query <QUERY>` — `--max-tokens 4000`, `--max-results 20`, `--format` toon|json|text [text],
  `--file-filter`, `--bm25-weights <W>` (R2.2a/D24; 7 csv f64, absent ⇒ default weights), `--db-path`
- `status` — `--db-path`
- `config` — positional `KEY [VALUE]` + `--db-path` (minimal/forward-compatible; read/write
  semantics land in M7.3)
- `serve` — `--transport` stdio|sse [stdio], `--port 3000`, `--db-path`

Two clap `ValueEnum`s: `OutputFormat` (toon|json|text) and `Transport` (stdio|sse), so out-of-set
values produce clap's own nonzero parse error. `From<OutputFormat> for formatter::Format` is the
seam keeping clap concerns inside `cli`. All `--db-path` share `DEFAULT_DB_PATH`.

`run()` → `Cli::parse()` then `dispatch()`; errors return `anyhow::Result` (Err → nonzero exit via
`main`). No reachable `unwrap()/expect()/panic!`. **Handlers are inert M7.3 placeholders** at this
slice — real delegation to `app`/`Indexer`/`Retriever`/`Config`/`Storage` lands in M7.3; `serve` is
an M8 stub.

## Tests / scenarios
`tests/cli_tests.rs` (5 tests via `assert_cmd`/`predicates`, D17): documented-flag parsing,
query defaults, help/version, bad-args → nonzero, unknown-command → nonzero.
`docs/TEST_STRATEGY.md#cli` — E2E `init → index → query` through the built binary (M7.4).

## Shipped handlers (M7.3)
Dispatch in `mod.rs`; one handler per command (`init/index/update/query/status/config/serve.rs`) +
`paths.rs` (db/config path resolution `<cwd>/<config.storage.db_path>`, matching the `app` facade).
Handlers are thin adapters returning `anyhow::Result<()>` (Err → nonzero exit; no reachable
`unwrap/expect/panic`):
- `init` → `codecache::init(&cwd)`; `index` → `codecache::index(&cwd)` (prints `IndexStats`).
- `update <FILE>...` → `Indexer::update_files(&paths)` (paths resolved under cwd).
- `query <QUERY>` → `Retriever::query` → `formatter::format(&qr, &query, fmt)` (fmt from the
  `OutputFormat` `From` seam). `--file-filter` ships as a single-entry exact-`PathBuf` post-filter
  (no glob expansion in v0.1). **Empty-result + text format prints `No results found.`** instead of
  the formatter's query-echoing empty header — an intentional CLI UX choice (the pure `formatter`
  empty-text golden is unchanged; the CLI just declines to render it). JSON always pipes through the
  formatter (parseable); empty TOON stays the query-free empty string. **R2.2a/D24:** `--bm25-weights
  "<7 csv f64>"` threads a per-column BM25 weight override into `QueryOptions.bm25_weights` (absent ⇒
  `None` ⇒ default weights). Parsed by the module-private `parse_bm25_weights` helper (unit-tested):
  split on `,`, parse each as f64, require **exactly 7**, reject non-finite (NaN/±inf); **zero and
  negative weights are allowed** (FTS5 honors them; the R2 sweep uses them). Any malformed/wrong-arity
  value → a typed `anyhow` error (validated BEFORE opening storage) → clean nonzero exit, never a
  panic. MCP `codecache_search` stays default-weighted (CLI-only surface — it builds `QueryOptions`
  via `..Default::default()`, inheriting `None`).
- `status` → reads `Storage::get_index_state("total_files"/"total_chunks")` + db file size +
  per-language counts from `files_metadata`; prints version + Files + Chunks + size. **Deferred (no
  schema change this slice):** Created/Last-index timestamps + per-symbol_type breakdown.
- `config` (**D18**) → no args prints the resolved config as TOML; `config <KEY> <VALUE>` sets a
  documented dotted scalar key (≥ `storage.max_db_size_mb`) and persists via `Config::save`; unknown
  key / bad value → nonzero error.
- `serve` (**M8.1**) → resolves the db path, opens `Storage`, builds
  `mcp_server::CodeCacheServer::new(storage)`, and runs `mcp_server::serve(stdin().lock(),
  stdout().lock(), server)` for `--transport stdio`. `--transport sse` → clean
  `anyhow` "unsupported in v0.1 (stdio only)" error → nonzero exit (D4 seam; pinned by
  `e2e_serve_unsupported_transport_sse_errors_cleanly`). `dispatch` threads `transport`/`db_path`
  through; `--port` parses but is inert until the v0.2 SSE adapter (no test pins port behavior).

## Status
M7.2 DONE (2026-06-12): clap parsing + error/exit-code mapping; reviewer APPROVED.
M7.3 DONE (2026-06-12): command handlers + `status` aggregates + `config` read/write (D18) shipped +
green (cli_tests 11/11); reviewer APPROVED (0 findings). Binary E2E → M7.4.
M8.1 DONE (2026-06-12): `serve` stub replaced — stdio wires the hand-rolled `mcp_server`; SSE returns a
clean unsupported error (D4 seam); reviewer APPROVED; all four gates green.
R2.2a / D24 GREEN (2026-06-14): `query --bm25-weights <W>` added (clap arg + `dispatch` thread +
`parse_bm25_weights` helper in `query.rs`). +3 cli_tests (help lists flag / valid vector runs /
malformed → nonzero no-panic) + 4 parser unit tests; cli_tests 14/14, lib unit 33; all four gates clean.
R2.3a / D25 GREEN (2026-06-14): hidden `ingest <CHUNKS_JSON>` subcommand added (`#[command(hide = true)]`
— 8th command, reachable but NOT in `--help`; `mod ingest` + `cli/ingest.rs` thin handler →
`crate::ingest_chunks`). Inserts caller-supplied pre-chunked records straight into storage (the research
chunker-ablation seam, bypassing discover→parse→chunk); the format-local input DTO + `ingest_chunks`
facade live in `app.rs` (serde off `types::Chunk`, D4/D5; reuses `insert_chunks`/`update_file_hash`/
`set_index_state` — no new `Storage` method). +3 cli_tests (hidden-but-reachable `ingest --help` /
hidden-from-top-level-help / required `<CHUNKS_JSON>` positional). E2E in `tests/e2e_ingest.rs` (10) + lib
surface in `tests/e2e_ingest_lib.rs` (3). cli_tests 17/17; **224 tests total**; all four gates clean.
