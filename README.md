# CodeCache

**A zero-dependency, deterministic code index that coding agents call as a tool.**

CodeCache parses your codebase into semantic units (functions, classes, methods) with
Tree-sitter, indexes them in SQLite + FTS5, and retrieves only the relevant snippets at query
time — replacing N rounds of grep with one structured, token-budgeted lookup. No embedding
model, no vector database, no language server, no cloud account: one Rust binary, one `.db`
file, works air-gapped. Grep-in-a-loop is excellent; CodeCache composes with it and earns its
keep where a structured index saves the agent turns and tokens.

- **Deterministic** — AST boundaries, not drifting embeddings.
- **Always fresh** — incremental re-index via xxHash; self-healing search re-indexes stale
  files transparently at query time.
- **Agent-first** — MCP tools (`codecache_search`, `codecache_update`, `codecache_outline`)
  with output ordered for the agent's next action; CLI-native too.
- **v0.1 scope** — Python, TypeScript, Go. AST + BM25 (embeddings deferred to v0.2).

> Status: **M0–M9 complete and green** (196 tests, four gates clean on Rust 1.85).
> **v0.1.0 release staged** — Python, TypeScript, Go parsers; AST + BM25 retrieval; MCP stdio server.
> Milestones in [`docs/ROADMAP.md`](docs/ROADMAP.md); positioning, landscape research, and the R1–R4
> research track in [`project_overview.md`](project_overview.md).

## Quickstart (target UX)
```bash
codecache init                  # create the index database (configures paths at init time)
codecache index                 # build the full index
codecache query "authenticate user" --max-tokens 4000
codecache update src/auth.py    # incremental re-index
codecache serve                 # MCP server for Claude Code
```

## How this project is built
CodeCache is developed **test-first (TDD)** by a coordinated team of Claude Code agents, with
quality gates enforced by hooks and CI. If you're contributing (human or agent), start here:

- [`CLAUDE.md`](CLAUDE.md) — project overview + golden rules.
- [`docs/ENGINEERING_PLAN.md`](docs/ENGINEERING_PLAN.md) — team, build order, TDD workflow, Definition of Done.
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — milestones + decision log.
- [`docs/TEST_STRATEGY.md`](docs/TEST_STRATEGY.md) — the test scenario matrix.
- [`docs/TODO.md`](docs/TODO.md) — what's next.
- [`docs/project_plan.md`](docs/project_plan.md) — full technical spec.

## Build & test
```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo bench
```

**Detailed walkthrough** — running every suite/gate, a full CLI session, the MCP server, and the
R1 research harness (setup + offline end-to-end run): [`docs/TESTING_AND_USAGE.md`](docs/TESTING_AND_USAGE.md).

## Claude Code MCP Setup

Wire CodeCache as an MCP server so Claude Code calls it automatically as a tool:

```json
{
  "mcpServers": {
    "codecache": {
      "command": "codecache",
      "args": ["serve", "--transport", "stdio"],
      "cwd": "/path/to/your/project"
    }
  }
}
```

Full guide (install, index, all three MCP tools): [`docs/CLAUDE_CODE_SETUP.md`](docs/CLAUDE_CODE_SETUP.md).

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md) — TDD workflow, quality gates (fmt/clippy/test/build),
MSRV 1.85, bench instructions, and the no-reachable-unwrap/expect/panic rule.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE).
Copyright 2026 EunHo Lee.
