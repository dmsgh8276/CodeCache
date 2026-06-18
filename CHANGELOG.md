# Changelog

All notable changes to CodeCache are documented in this file.

Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Changed
- **Package renamed `codecache` → `codecache-rs`** for crates.io publication (the name `codecache`
  was already registered). The installed **binary remains `codecache`** — `cargo install codecache-rs`
  produces a `codecache` binary (crate name ≠ binary name, the ripgrep model), so CLI usage and MCP
  config are unchanged. (Decision Log D30.)
- **Published crate scoped to product code** via a `[package]` `include` allowlist: the crates.io
  tarball ships `src/`, benches, examples, and README/licenses/CHANGELOG only — `research/`,
  `.claude/`, and `docs/` are excluded so they never reach the permanent public registry tarball.
  (Decision Log D31.)

### Performance
- **Batched cold-index inserts (Decision Log D20)** — the indexer now writes a whole index run's
  per-file rows inside **one outer transaction** (a SAVEPOINT per file) instead of a commit per
  file, amortizing ~N fsyncs into one. New additive `Storage::write_in_transaction` primitive;
  **D2 per-file error isolation preserved** (a malformed/unreadable/failing file rolls back only its
  own savepoint and is skipped — the batch still succeeds and siblings commit). Resolves the only
  M10.1 budget miss: 10K-LOC cold index measured **5.84 s → 1.37 s (−76.5%, WSL2/Linux, well under
  the < 5 s budget)**; Windows CI is the authoritative gate for the budget verdict.

---

## [0.1.0] - 2026-06-12

First public release of CodeCache — a zero-dependency, deterministic code index for coding
agents. Parses source code with Tree-sitter into semantic units, indexes them in SQLite + FTS5,
and retrieves token-budgeted, BM25-ranked snippets. Serves agents via CLI stdout or an MCP
stdio server. No embedding model, no vector database, no cloud account.

### Added

**Parsing (Tree-sitter, v0.1 languages: Python, TypeScript, Go)**
- AST-driven chunking: functions, classes, methods, and top-level declarations extracted as
  semantic units with byte spans, line numbers, parent symbol, imports, cross-references, and
  file docstrings.
- ERROR-node detection: malformed files are indexed with `parse_error = true`; degradation is
  documented and tested (Decision Log D2).
- Language detection from file extension; unknown extensions are skipped cleanly.

**Storage (SQLite + FTS5)**
- Single `.codecache/index.db` file; zero system dependencies (rusqlite bundled build).
- FTS5 virtual table `symbols` with BM25-weighted columns (symbol_name 10.0, chunk_text 1.0,
  imports 2.0, cross_references 2.0, etc.).
- Incremental change detection via xxHash3-128 stored in `files_metadata`; unchanged files are
  skipped on re-index.
- Self-healing search: stale files detected at query time and transparently re-indexed before
  results are returned (Decision Log D14).

**Retrieval (BM25 + token budget)**
- Query pre-processing: tokenization, stopword removal, FTS5-safe MATCH expression construction.
- BM25 ranking via SQLite FTS5's built-in `bm25()` function; deterministic tie-breaking by rowid.
- Token-budget packing: greedy hard-stop at `max_tokens` ceiling; `max_results` cap.
- `--file-filter` option for scoping retrieval to a single file path.
- Deduplication across BM25 result set before budget packing.

**CLI (`codecache` binary)**
- `init` — creates `.codecache/` directory and `index.db` with schema.
- `index` — full index build from the configured paths.
- `update <FILE>...` — incremental re-index of specified files.
- `query <QUERY>` — retrieves ranked snippets; `--max-tokens`, `--max-results`, `--format`
  (text/json/toon), `--file-filter`.
- `config` — read/write `.codecache/config.toml` key-value pairs (Decision Log D18).
- `serve` — MCP stdio server (see MCP Server below).

**MCP Server (stdio JSON-RPC, hand-rolled — no rmcp/tokio dependency; Decision Log D15)**
- Three MCP tools registered:
  - `codecache_search` — full-text BM25 search with token budget; returns ranked snippets.
  - `codecache_update` — incremental re-index of a list of file paths.
  - `codecache_outline` — returns all indexed symbols for a given file path (Decision Log D19).
- Self-healing search integrated: stale files are re-indexed before `codecache_search` returns.
- Transport: stdio only (SSE is v0.2; Decision Log D4).

**Benchmarks and quality metrics (M10)**
- Criterion bench suite covering all systems budgets (§1.3, §5.4, §11):
  - Query latency p95: 0.51 ms (budget < 500 ms). PASS.
  - Index size (100K LOC): 12.3 MB (budget < 100 MB). PASS.
  - Incremental re-index (10 files): 190 ms (budget < 2 s). PASS.
  - Cold index 100K LOC: 13.5 s (budget < 30 s). PASS.
  - Hash 1K files: 459 ms (budget < 500 ms). PASS.
- FTS5 EXPLAIN QUERY PLAN baseline captured: inverted-index scan confirmed (no full scan);
  details in `benches/CLAUDE.md`.
- Layer-1 retrieval quality (Decision Log D16): offline micro-suite, 3 corpora × 5 queries.
  Keyword Recall@10 = 1.000, F1@10 = 0.51 (file) / 0.49 (block). Research track R2 carries
  the expansion to the real ContextBench corpus.

**Documentation and release infrastructure**
- `README.md` — quickstart, build instructions, MCP setup link.
- `docs/CLAUDE_CODE_SETUP.md` — full MCP integration guide for Claude Code.
- `CONTRIBUTING.md` — TDD workflow, quality gates, MSRV, bench instructions.
- `LICENSE-MIT` and `LICENSE-APACHE` — dual MIT/Apache-2.0 license.
- `.github/workflows/ci.yml` — fmt/clippy/test gate on push/PR, matrix Linux/macOS/Windows.
- `.github/workflows/bench.yml` — scheduled (weekly) criterion trend-tracking.
- `.github/workflows/release.yml` — tag-triggered: smoke test + cargo publish + binary artifacts.

### Known Issues

**Cold-index 10K LOC: 6.04 s vs < 5 s budget (Decision Log D20 — tracked, not a release
blocker).** Measured on Windows 11 / Rust 1.85 with the criterion bench; the root cause is
per-file SQLite INSERT cost growing with inverted-index size (FTS5 write amplification). The
100K LOC budget (< 30 s) passes with > 2x margin (13.5 s measured). A v0.1.x transaction-
batching optimization slice is planned. This budget miss is recorded here and tracked via the
scheduled bench.yml artifact — it will NOT permanently break CI (trend-tracked, not asserted).

**BM25-only semantic-query recall gap (Decision Log D1).** Pure semantic queries ("error
handling", "settings not found") score Recall@10 = 0.000 because BM25 is a lexical retriever
and the query vocabulary does not appear verbatim in the indexed chunks. This is the expected
v0.1 limitation; hybrid embeddings are planned for v0.2.

**Layer-1 retrieval quality measured on a 15-query offline micro-suite proxy (Decision Log
D21).** The real ContextBench corpus (arXiv:2602.05892) requires network access and is not
vendorable offline. The micro-suite uses the identical scoring protocol (Recall@k, Precision@k,
F1 at file + block granularity). Research track R2 carries the expansion to the full corpus.

**SSE transport unsupported in v0.1 (Decision Log D4).** `codecache serve --transport sse`
returns a clean error and exits non-zero. Stdio is the only supported transport.

---

[Unreleased]: https://github.com/AdvancedUno/codecache/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/AdvancedUno/codecache/releases/tag/v0.1.0
