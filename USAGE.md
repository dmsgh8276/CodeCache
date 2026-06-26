# Using CodeCache

A practical, task-oriented guide to **using** CodeCache — installing it, indexing a repo, querying
it from the terminal, and wiring it into an AI coding agent as an MCP tool.

> New here? Read the one-paragraph pitch in [`README.md`](README.md) first. This document is the
> "how do I actually drive it" manual. For **running the test suites and quality gates**, see
> [`docs/TESTING_AND_USAGE.md`](docs/TESTING_AND_USAGE.md). For **MCP / Claude Code wiring**, see
> [`docs/CLAUDE_CODE_SETUP.md`](docs/CLAUDE_CODE_SETUP.md). For **verifying the research track**, see
> [`docs/RESEARCH_VERIFICATION.md`](docs/RESEARCH_VERIFICATION.md).

CodeCache is a single Rust binary. It parses your code into semantic units (functions, classes,
methods) with Tree-sitter, indexes them in SQLite + FTS5, and at query time returns only the
relevant, token-budgeted snippets — so an agent does one structured lookup instead of N rounds of
grep. No embedding model, no vector DB, no language server, no cloud account.

**v0.1 languages:** Python, TypeScript, Go.

---

## 1. Install

### Build from source (current path)

Requires **Rust 1.85.0** (pinned by [`rust-toolchain.toml`](rust-toolchain.toml); `rustup`
auto-selects it). On Windows you also need the MSVC toolchain (VS 2022 Build Tools) for the bundled
SQLite + Tree-sitter C dependencies.

```bash
git clone <repo-url> CodeCache && cd CodeCache
cargo build --release          # → target/release/codecache
```

The first build compiles bundled C (SQLite + grammars) and takes ~1–1.5 min cold; later builds are
fast. Put the binary on your `PATH` (or call it by full path):

```bash
# bash/zsh — e.g. add to ~/.local/bin
install -m 0755 target/release/codecache ~/.local/bin/codecache
codecache --version            # codecache 0.1.0
```

> The crate is published as **`codecache-rs`** on crates.io; the binary it installs is **`codecache`**.

---

## 2. The 60-second tour

Run these from the root of the project you want to index. CodeCache writes a `.codecache/`
directory there (the index DB + config) — add it to your `.gitignore`.

```bash
codecache init                 # create .codecache/ + config.toml + the index DB (idempotent)
codecache index                # parse + chunk + index every supported file
codecache status               # counts, DB size, per-language breakdown
codecache query "authenticate user password"   # retrieve relevant snippets
```

Real output on a small mixed Python/TypeScript/Go repo:

```
$ codecache init
Initialized CodeCache index in /path/to/project

$ codecache index
Indexed 3 file(s), 7 chunk(s) in 95 ms

$ codecache status
CodeCache index status
  Version:   0.1.0
  Database:  /path/to/project/.codecache/index.db (49152 bytes)
  Files:     3
  Chunks:    7
  Files by language:
    go: 1
    python: 1
    typescript: 1
```

---

## 3. Indexing

| Command | What it does |
|---|---|
| `codecache index` | Full/incremental index of all supported files under the configured paths. On a populated DB it **skips unchanged files** (xxHash content+mtime compare), re-indexes changed/new ones, and reconciles deletions. Safe to re-run. |
| `codecache update <FILE>...` | Re-index only the named file(s). Use after editing a few files for a fast, targeted refresh. |

Discovery honors `.gitignore` and any extra ignore patterns in your config. Only files whose
extension maps to a configured language (`.py`, `.ts`, `.go`) are indexed.

```bash
$ codecache index            # second run, nothing changed
Indexed 0 file(s), 0 chunk(s) in 26 ms      # idempotent no-op

$ codecache update src/auth/authenticate.py # after editing that file
Updated 1 file(s), 5 chunk(s) in 59 ms
```

**Re-index is incremental and self-healing.** You rarely need to think about freshness: `update`
covers explicit edits, and the MCP server's `codecache_search` re-checks and re-indexes stale result
files transparently at query time (see §6).

---

## 4. Querying

```bash
codecache query "<natural-language or keyword query>" [flags]
```

CodeCache lowercases the query, drops a small set of natural-language filler words (`the`, `find`,
`show`, `how`, …) — **not** programming keywords — and runs an FTS5 BM25 search over the indexed
columns (symbol name weighted highest, then parent symbol, then body/enrichment). Results are
de-duplicated, ranked deterministically, and packed to fit a **hard token budget**.

### Flags

| Flag | Default | Meaning |
|---|---|---|
| `--max-tokens <N>` | `4000` | Hard ceiling on total tokens returned. The packer keeps the highest-ranked prefix that fits and never exceeds `N`. |
| `--max-results <N>` | `20` | Max number of FTS5 hits to consider. |
| `--format <FMT>` | `text` | Output format: `text`, `json`, or `toon`. |
| `--file-filter <PATH>` | — | Keep only results whose file path matches (exact path post-filter). |
| `--bm25-weights "<7 csv>"` | built-in | Override the 7 per-column BM25 weights (research/tuning aid). |

### Output formats

**`text`** (default) — agent-first: a one-line locator (`symbol (type) file:start-end (score)`)
followed by the signature and body. The header reports the result count and total tokens.

```
$ codecache query "authenticate user password" --max-results 3
────────────────────────────────────────────────────────
Query: "authenticate user password"
Found 3 results (showing top 3, 108 tokens)
────────────────────────────────────────────────────────

[1] authenticate_user (function) .../src/auth/authenticate.py:4-7 (score: -4.07)
def authenticate_user(username: str, password: str) -> bool:
    """Verify a user's password against the stored hash."""
    stored = lookup_hash(username)
    return verify_password(password, stored)
...
```

**`json`** — machine-readable; ideal for piping into other tools.

```
$ codecache query "verify password" --format json --max-results 1
{
  "query": "verify password",
  "total_results": 1,
  "total_tokens": 33,
  "chunks": [
    {
      "symbol_name": "verify_password",
      "symbol_type": "function",
      "file_path": ".../src/auth/authenticate.py",
      "start_byte": 269,
      "end_byte": 403,
      "language": "python",
      "bm25_score": -3.107,
      "chunk_text": "def verify_password(password: str, stored_hash: str) -> bool:\n    ..."
    }
  ]
}
```

> **Score convention:** `bm25_score` is FTS5's BM25 — **more negative = more relevant**.

**`toon`** — locator-only; one `file:start-end` line per hit. Pipes straight into an editor or `cat`.

```
$ codecache query "AuthenticateUser" --format toon
.../web/login.ts:4-4
.../svc/server.go:4-6
.../web/login.ts:1-3
```

A query with no lexical match prints `No results found.` (text) or an empty result (json/toon).

> **BM25 is lexical, not semantic.** It matches query *terms* against indexed text, so a query whose
> vocabulary doesn't appear in the code may return nothing. Use the code's own words. (Hybrid
> embeddings are deferred to v0.2 — Decision Log D1.)

---

## 5. Configuration

```bash
codecache config                 # print the resolved config as TOML
codecache config <KEY> <VALUE>   # set a documented dotted key and persist to .codecache/config.toml
```

Config lives at `.codecache/config.toml`. Defaults cover the common case (index `.`, languages
`python,typescript,go`); `init` writes a default file only if one isn't already present (it never
clobbers an existing config).

---

## 6. Use it inside an AI coding agent (MCP)

The highest-value mode: run CodeCache as an **MCP server** so your agent calls it as a tool instead
of grepping in a loop.

```bash
codecache serve                  # stdio JSON-RPC MCP server (Ctrl-C to stop)
```

Wire it into Claude Code (stdio is the default transport; `--transport sse` returns a clean
"unsupported in v0.1" error):

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

The server exposes three tools (fixed, deterministic order):

| Tool | Purpose | Required arg |
|---|---|---|
| `codecache_search` | BM25 retrieval with **self-healing** (re-indexes stale result files before answering). | `query` |
| `codecache_update` | Force re-index of the named files. | `files` (string array) |
| `codecache_outline` | List all indexed symbols for a file (zero source reads). | `path` |

A live stdio session looks like this (one JSON-RPC object per line in, one per line out):

```
→ {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05"}}
← {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{},
   "serverInfo":{"name":"codecache","version":"0.1.0"}}}
→ {"jsonrpc":"2.0","id":2,"method":"tools/list"}
← {... "tools":[{"name":"codecache_search"...},{"name":"codecache_update"...},{"name":"codecache_outline"...}]}
→ {"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"codecache_search",
   "arguments":{"query":"authenticate user"}}}
← {... "result":{"content":[{"type":"text","text":"...ranked snippets..."}]}}
```

> **`codecache_outline` path tip:** match the path **as it was indexed**. `codecache_search` returns
> absolute paths, so when chaining search → outline, pass the absolute path back. A path that does
> not match an indexed file returns `Found 0 symbols`.

Full setup, the three tool schemas, and the self-healing notes are in
[`docs/CLAUDE_CODE_SETUP.md`](docs/CLAUDE_CODE_SETUP.md).

---

## 7. Exit codes & errors

- Every command returns **0 on success, non-zero on failure**, with a human-readable message on
  stderr. There are no reachable panics: bad input (e.g. a malformed `--bm25-weights` vector, an
  unknown config key, `--transport sse`) produces a clean typed error and a non-zero exit.
- Indexing **never hard-fails on one malformed source file** — a broken file degrades gracefully
  (heuristic chunking or skip) while the rest of the batch indexes.

---

## 8. Troubleshooting

| Symptom | Fix |
|---|---|
| `No results found` for something you expect | Confirm `init`+`index` ran; check the file extension is `.py`/`.ts`/`.go`; remember BM25 is lexical — use terms that appear verbatim in the code. |
| `cargo: command not found` | Install Rust via `rustup`; ensure `~/.cargo/bin` is on `PATH`. |
| Index seems stale after edits | Run `codecache update <files>`, or rely on MCP self-healing search. |
| `SSE transport is not supported in v0.1` | v0.1 is stdio-only; drop `--transport sse` (SSE is a v0.2 item). |
| MCP `codecache_outline` returns 0 symbols | Pass the path exactly as indexed (absolute paths as returned by `codecache_search`). |

---

## 9. Where to go next

- [`README.md`](README.md) — overview + quickstart.
- [`docs/TESTING_AND_USAGE.md`](docs/TESTING_AND_USAGE.md) — run the suites, gates, benches, and the research harness.
- [`docs/CLAUDE_CODE_SETUP.md`](docs/CLAUDE_CODE_SETUP.md) — MCP configuration and the three tools.
- [`docs/RESEARCH_VERIFICATION.md`](docs/RESEARCH_VERIFICATION.md) — reproduce and verify the R1–R2 research results.
- [`docs/project_plan.md`](docs/project_plan.md) — full technical spec and API contracts.
