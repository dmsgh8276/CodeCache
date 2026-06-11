# src/ — CLAUDE.md

CodeCache library + binary crate root. **Owner agent:** `principal-engineering-lead`
(implementation); module ownership per [`../docs/ENGINEERING_PLAN.md`](../docs/ENGINEERING_PLAN.md) §1–2.

## Layout (per project_plan §10.4, build order bottom-up per ENGINEERING_PLAN §2)
| Path | Module | Milestone | Notes |
|---|---|---|---|
| `lib.rs` | crate root | M0 | module decls + `pub const VERSION` (only valued public symbol at M0) |
| `main.rs` | binary entry | M0 | `fn main() -> anyhow::Result<()>` → `cli::run()` |
| `types/` | `types` | M1 | shared dependency-free types (`Chunk`, `Language`, `SymbolType`, `FileMeta`) — Decision Log D5 |
| `config/` | `config` | M1 | load/validate `.codecache/config.toml` |
| `storage/` | `storage` | M1 | SQLite + FTS5 schema, CRUD, BM25; `Arc<Mutex<Connection>>` (D8) |
| `hasher/` | `hasher` | M2 | xxHash3-128 + change detection |
| `parser/` | `parser` | M3 (TS/Go M9) | Tree-sitter grammars/queries, byte spans, ERROR-node detection (D2) |
| `chunker/` | `chunker` | M4 | AST → enriched `Chunk`s (D3) |
| `indexer/` | `indexer` | M5 | discovery → parse → chunk → hash → store; incremental |
| `app.rs` | `app` | M5 | thin `init`/`index` facade over config/storage/indexer; `AppError` (M5.4) |
| `retriever/` | `retriever` | M6 | BM25 + snippet + token budget (trait-backed for D1) |
| `formatter/` | `formatter` | M7 | TOON/JSON/text; line ranges from stored line numbers (D7) |
| `cli/` | `cli` | M7 | `clap` commands |
| `mcp_server/` | `mcp_server` | M8 | stdio JSON-RPC; transport-agnostic core (D4) |

## Rules
- TDD: a failing test exists before any line here (`docs/ENGINEERING_PLAN.md` §3).
- Match the documented APIs (`docs/project_plan.md` §3.2); change the plan first if diverging.
- No reachable `unwrap()/expect()/panic!`; `Result` + `?`; typed errors / `anyhow`.
- Code change ⇒ update `docs/TODO.md` + the local module `CLAUDE.md` in the same change.
- At M0 every module body is an empty stub (doc comment + `#[cfg(test)] mod tests {}`).

## Tests
Unit tests live in each module's `#[cfg(test)] mod tests`. Integration/E2E/property tests live
in [`../tests/`](../tests/CLAUDE.md). Scenario matrix: [`../docs/TEST_STRATEGY.md`](../docs/TEST_STRATEGY.md).
