# CodeCache — Roadmap

Milestones for v0.1, each gated by tests. Build order is bottom-up (see
[`ENGINEERING_PLAN.md`](ENGINEERING_PLAN.md) §2). The live, checkable status lives in
[`TODO.md`](TODO.md); this file defines **entry/exit criteria** and the **Decision Log**.

A milestone is *done* only when its exit criteria are met under the Definition of Done
(`ENGINEERING_PLAN.md` §4).

---

## Milestones

### M0 — Scaffolding & CI
- **Entry**: empty repo.
- **Work**: `cargo` project per §10.4 layout; `Cargo.toml` deps per §10.3; root + per-dir
  `CLAUDE.md`; CI (`ci.yml`: fmt/clippy/test) green on an empty `lib.rs`.
- **Exit**: `cargo build`/`cargo test` run; CI green; hooks fire on `.rs` edits.

### M1 — `config` + `storage` (SQLite schema + FTS5)
- **Work**: `.codecache/config.toml` load/validate; SQLite schema (`symbols` FTS5,
  `files_metadata`, `index_state`); create/migrate; CRUD; FTS5 virtual table with `bm25()`.
- **Exit**: round-trip insert/query/delete tested; FTS5 `MATCH` + `bm25()` ordering tested;
  schema creation idempotent.

### M2 — `hasher`
- **Work**: xxHash3-128 of file contents; compare against cached hash for change detection.
- **Exit**: stable hashes; change/no-change detection tested; large-file and binary handling.

### M3 — `parser` (Python first)
- **Work**: load `tree-sitter-python`; `LanguageConfig`; run `.scm` queries to extract
  function/class/method nodes with exact byte spans; ERROR-node detection.
- **Exit**: byte spans correct on fixtures incl. nested/async/decorated symbols; malformed
  files don't panic (degradation path exercised — Decision Log #2).

### M4 — `chunker`
- **Work**: turn AST nodes into `Chunk`s with metadata enrichment (`parent_symbol`,
  `file_docstring`, `imports`, `cross_references` — Decision Log #3).
- **Exit**: non-overlapping chunks within file bounds (property test); enrichment fields populated.

### M5 — `indexer`
- **Work**: file discovery honoring `.gitignore`/ignore patterns; orchestrate
  parse→chunk→hash→store; incremental update (only changed files).
- **Exit**: full index of a fixture repo; re-index unchanged = no-op (idempotent); change 1
  file ⇒ only it re-indexed; e2e `init→index` green.

### M6 — `retriever`
- **Work**: FTS5 BM25 search; snippet extraction; token counting; greedy token-budget packing.
- **Exit**: ranking deterministic; `--max-tokens` respected exactly; empty/no-match queries
  handled; latency bench wired (perf engineer).

### M7 — `formatter` + `cli`
- **Work**: TOON/JSON/text formatters; `clap` commands `init/index/update/query/status/config/serve`;
  **agent-first output ordering** (signature/skeleton before bodies — D13).
- **Exit**: golden-output tests per format; CLI arg parsing + error messages tested; e2e
  `init→index→query` through the binary.

### M8 — `mcp_server`
- **Entry**: evaluate the official MCP Rust SDK (`rmcp`) vs hand-rolled JSON-RPC; pin a version
  either way (D15).
- **Work**: stdio MCP adapter; tool registration (`codecache_search`, `codecache_update`,
  `codecache_outline` — D13); **self-healing search** (hash-check + transparent re-index of
  result files at query time — D14); `serve` command.
- **Exit**: protocol handshake + tool-call round-trip tested against a mock client; all three
  tools registered; a query against a stale file returns fresh content (D14 test).

### M9 — TypeScript + Go parsers
- **Work**: add `tree-sitter-typescript` and `tree-sitter-go` configs + queries.
- **Exit**: per-language fixture suites green; language coverage = Python/TS/Go.

### M10 — Benchmarks + Release
- **Work**: criterion suite vs all systems budgets; **Layer-1 retrieval-quality scoring**
  (ContextBench-Lite gold contexts + hand-verified micro-suite — D16 replaces the old
  "5 real tasks" benchmark); release workflow; crates.io publish.
- **Exit**: systems budgets met (p95<500ms, index<100MB, incr<2s); Layer-1 retrieval metrics
  recorded (the Layer-2 token-economy headline is the research track's R3 exit, not a release
  gate); `v0.1.0` tagged and published; install smoke test passes.

---

## Research track (post-M8; M9 can interleave) — `project_overview.md` §5–§6

The measurement study the repositioning (D12) is built on: a controlled, same-agent comparison
of retrieval *interfaces* (grep-only vs index-as-tool vs embedding-as-tool) under explicit token
budgets. Arms A0–A5, three metric layers (retrieval quality / token & turn economy / systems
costs), and the experiment matrix are specified in `project_overview.md` §5. Budget: ~$1K API
spend (R3). Kill criterion and null-result handling: `project_overview.md` §7.

| Milestone | Work | Exit criteria |
|---|---|---|
| **R1 — Harness** | Minimal agent loop (or mini-SWE-agent fork) with pluggable retrieval tools; trajectory logging; ContextBench gold-context scorer | One task runs end-to-end in A0/A1/A4; metrics computed from logs |
| **R2 — Offline ablations** | Layer-1 sweeps: chunking × ranking × enrichment on ContextBench-Lite + RepoEval slice | Published BM25 baselines reproduced within tolerance; top configs picked |
| **R3 — Agent-in-loop study** | Full matrix on 30–50 tasks; promote winners to 100; budget/scale sweeps | RQ1–RQ3 plots with CIs; raw trajectories published |
| **R4 — Write-up & release** | Preprint + artifact (binary, harness, data); blog distillation; one workshop submission | arXiv live; artifact reproduces the headline figure from a clean machine |

---

## Decision Log

Design critiques raised during review of the original spec, with their disposition. The
manager updates this as decisions are made.

### D1 — Hybrid retrieval (AST+BM25 → optional embeddings)  · **Deferred to v0.2**
AST+BM25 misses ~30–40% of semantic queries ("find all error-handling patterns"). v0.1 ships
BM25-only; a `--enable-embeddings` flag may log a low-recall warning. Plan: optional
CodeBERT/UniXcoder index in v0.1.5, hybrid default in v0.2. Keep `Retriever` behind a trait so
a `HybridRetriever` can wrap it without churn. *Cost: +2 wks, +~3GB/index.*

### D2 — Graceful Tree-sitter degradation  · **Adopted for v0.1**
Tree-sitter produces `ERROR` nodes on 5–15% of real files. Count ERROR nodes; above a
threshold (~20%) fall back to heuristic/regex chunking and mark chunks `heuristic` in metadata.
Indexing must never hard-fail on malformed input. Owned by `rust-treesitter-specialist`;
enforced by M3/M5 tests. *Cost: +~1 wk, +~200 LOC.*

### D3 — Chunk metadata enrichment  · **Adopted for v0.1**
Extend `Chunk` with `parent_symbol`, `file_docstring`, `imports`, `cross_references` (indexed
in FTS5) to lift recall on indirect queries. Extracted during AST traversal in M4. *Cost:
~+1MB/index (negligible).*

### D4 — Integration decoupling (HTTP / LSP beyond MCP)  · **Partially deferred**
Avoid single-vendor lock-in on MCP. v0.1: keep the retrieval core transport-agnostic so a thin
HTTP REST adapter (`codecache serve --http`) can be added without refactoring; document an LSP
path. Full HTTP API hardening and LSP land in v0.2. *Cost: +~1 wk (HTTP), +~2 wks (LSP, v0.2).*

---

## Clarifications raised during phase planning (`docs/plans/`)

These were surfaced while writing the per-milestone phase plans. They refine — not contradict —
the spec; where a public API or schema is affected, `project_plan.md` is updated **first**.

**Ratified 2026-06-09** (manager, during M0 kickoff): D5–D8 dispositions below are final for
v0.1 and now reflected in `project_plan.md`. D5–D7 affect M0/M1 and were ratified before any
code was written, per the "change the plan before diverging" rule. The M0 scaffolding therefore
declares a `types` module in `src/lib.rs` (D5); D6/D7 are realized by M1's schema + M4/M5's
populate logic; D8 is realized at M8.

### D5 — Shared core types location  · **Ratified for v0.1** (plan: M0/M1) — *spec: §4.3, §3.2.1*
`Chunk`, `Language`, `SymbolType` (and `FileMeta`) live in a dependency-free `crate::types`
module rather than inside `parser`, so `storage` need not depend on `parser` and the bottom-up
build order (`ENGINEERING_PLAN.md` §2) stays acyclic. **M0 action:** `src/lib.rs` declares
`pub mod types;` and `src/types/mod.rs` is created as an (initially empty) stub module.

### D6 — `files_metadata` write signature  · **Ratified for v0.1** (plan: M1, M5) — *spec: §3.2.2*
`update_file_hash(file_path, meta: &FileMeta)` takes a `FileMeta { content_hash, mtime,
file_size, language, chunk_count }` so M5's incremental indexer persists every §4.1 column in
one call. `project_plan.md` §3.2.2 updated to match.

### D7 — Store line numbers at index time  · **Ratified for v0.1** (plan: M7, affects M1/M4/M5) — *spec: §3.2.2, §4.1, §4.3*
`symbols` gains `start_line`/`end_line` UNINDEXED columns and `Chunk` gains `start_line`/
`end_line` (1-based, inclusive), populated at index time. This lets the TOON/text formatters
emit `file:start-end` line ranges without re-reading source at query time (preserves the §11.2
budget). Ratified before M1 ships to avoid a later schema migration.

### D8 — MCP server resource ownership  · **Ratified for v0.1** (plan: M8) — *spec: §3.2.2, §3.2.3, §8.3*
`Storage` wraps `Arc<Mutex<rusqlite::Connection>>` (Connection is not `Clone`); cloning
`Storage` is a cheap Arc clone, so the MCP server lends the same connection to both `Retriever`
and `Indexer`. Single-writer semantics are preserved by the Mutex. `project_plan.md`
§3.2.2/§3.2.3/§8.3 updated to match. No M0 action (module boundary already exists).

**Ratified 2026-06-10** (during M0 build verification): D9–D10 below correct two issues caught
by the first real `cargo build` (the toolchain was installed locally this session).

### D9 — rusqlite FTS5 feature  · **Ratified for v0.1** (plan: M0, affects M1) — *spec: §10.3*
rusqlite 0.32 has **no `fts5` cargo feature**; FTS5 is compiled into the `bundled` SQLite
amalgamation by default. The original `features = ["bundled", "fts5"]` failed dependency
resolution. Corrected to `features = ["bundled"]` in both `Cargo.toml` and `project_plan.md`
§10.3. FTS5 availability is proven by M1's first `CREATE VIRTUAL TABLE ... USING fts5`.

### D10 — Toolchain/MSRV bump 1.82.0 → 1.85.0  · **Ratified for v0.1** (plan: M0, affects CI + all phases) — *spec: §10.3 (`edition`)*
The 1.82.0 pin was a planning-time guess (Oct 2024). With no committed `Cargo.lock`, cargo
resolved transitive deps to latest, and `hashbrown 0.17` (pulled in via `toml`/`indexmap`) now
requires **edition 2024**, which Cargo only understands from **1.85** onward; `cargo build`
fails on 1.82 with *"feature `edition2024` is required ... not stabilized in this version of
Cargo (1.82.0)"*. Compounding this, 1.82 predates the **MSRV-aware dependency resolver**
(stabilized in 1.84), so it cannot auto-select MSRV-compatible deps and would re-break on every
`cargo update`.

**Decision (Path A — bump the pin).** Rejected the alternative of staying on 1.82 with a
hand-pinned `Cargo.lock` holding every too-new transitive dep down: that is fragile whack-a-mole
that fights the ecosystem and, lacking the 1.84+ resolver, re-breaks indefinitely. We pin a
**deliberate MSRV of 1.85.0** — the exact floor that (a) stabilizes edition 2024 (what
`hashbrown 0.17` demands) and (b) ships the MSRV-aware resolver — rather than chasing latest
stable (1.96.0). This keeps our MSRV as conservative as dependency reality allows, gives
downstream consumers a meaningful compatibility contract, and lets the `rust-version = "1.85"`
key in `Cargo.toml` hold transitive deps to 1.85-compatible versions.

**Disposition.** `rust-toolchain.toml` `channel = "1.85.0"` is the **single source of truth**;
`Cargo.toml` `rust-version = "1.85"` is the MSRV; CI honors the toolchain file (so local == CI
parity is unchanged — same gates, same flags). `project_plan.md` §10.3 keeps `edition = "2021"`
for our own crate (the edition-2024 requirement is a *transitive dependency's*, not ours).
Owner of `rust-toolchain.toml` + `ci.yml` + `.github/CLAUDE.md`: `devops-release-engineer`;
ROADMAP/ENGINEERING_PLAN/phase-plan edits: manager. A generated `Cargo.lock` is committed
(ROADMAP follow-up R1) so the resolved versions are reproducible.

### D11 — FTS5 table form: drop `content='symbols'`, use a contentful table  · **Ratified for v0.1** (plan: M1, affects M10) — *spec: §4.1*
project_plan §4.1's pseudo-DDL set `content='symbols'`. In FTS5 the `content=` option names a
**separate external-content table** the FTS index reads from; pointing it at the FTS5 table's own
name is not a valid external-content configuration. M1 therefore creates a **default (contentful)
FTS5 table**: FTS5 stores every column value itself and returns it on `SELECT`, so a `Chunk`
round-trips through `insert_chunks` → `search` with no companion table and the round-trip tests
assert real column values. The FTS5 list columns `imports`/`cross_references` (no array type in
FTS5) are stored as `\n`-joined text and split back on read.

**Why this is safe for the budgets.** §4.2 estimates ~6MB index at Django scale — far under the
<100MB target (§1.3) — so the modest duplication of a contentful table is acceptable for v0.1. An
external-content layout (a `files`/`chunks` base table + `content_rowid`) can be revisited at M10
only if the index-size budget is ever threatened. §4.1 annotated to reflect this. Owner: manager
(spec) + rust-treesitter-specialist (FTS5).

**Follow-up (M1 gate reopen, 2026-06-10): `file_docstring` is an indexed `symbols` column.**
The §4.1 DDL listed only four of D3's enrichment fields (`parent_symbol`, `imports`,
`cross_references`) as indexed and omitted `file_docstring`, even though the `Chunk` struct (§4.3)
and D3 both declare `file_docstring` as enrichment "indexed in FTS5 to lift recall." The DDL was
the documentation bug, not the struct: a `Chunk.file_docstring` must persist and be searchable so
file-intent queries (e.g. a term that appears only in the module docstring) match. Fixed §4.1
(both DDL blocks) to add `file_docstring` as the **last indexed column**, immediately before the
UNINDEXED block — preserving the "indexed columns first, then UNINDEXED" ordering. This bumps the
indexed-column count 6→7, so the `bm25()` per-column weight list grows by one entry
(`file_docstring` weighted modestly, like `parent_symbol`). No schema-version bump: M1 is not yet
released (the pre-release schema is being corrected in place). Owner: manager (spec) +
rust-treesitter-specialist (FTS5 weights).

**Ratified 2026-06-11** (director's assessment + landscape research —
[`../project_overview.md`](../project_overview.md)): D12–D16 below adopt the report's
repositioning and its four plan deltas (Δ1–Δ4). M0–M10 architecture and build order stand;
these change framing, two tool-surface additions, one M8 entry check, and the M10 evaluation.

### D12 — Repositioning: index-as-tool inside the agent's search loop  · **Adopted for v0.1** — *overview §2–§3*
The original framing ("indexed retrieval replaces context dumping") is stale: agentic
grep-in-a-loop search won the default (Claude Code A/B-tested RAG and dropped it), and the
frontier moved to hybrid/trained retrieval. CodeCache's claim is now: **a zero-dependency,
deterministic code index that agents call as a tool — replacing N rounds of grep with one
structured lookup** ("the SQLite of code context"). The agent is the user; we compose with grep,
not compete with it. Architecture unchanged (MCP is already the loop-tool interface); framing +
evaluation change (D16). Spec §1.2/§1.3 updated.

### D13 — `codecache_outline` tool + agent-first output ordering (Δ1)  · **Adopted for v0.1** (plan: M7/M8) — *overview §3, §6*
Add a third MCP tool returning the symbol skeleton of a file/directory straight from the index
(name, type, parent, line ranges — D7 makes it zero-read), and order all tool/format output
agent-first: signature/skeleton before bodies, bodies only within budget. Spec §8.2 updated.

### D14 — Self-healing search (Δ2)  · **Adopted for v0.1** (plan: M8; seam noted in M6) — *overview §3, §6*
Staleness is the strongest anti-index argument; kill it structurally. Before answering,
`codecache_search` hash-checks files implicated by the top results (hashes already stored — §4.4)
and transparently re-indexes changed ones. Scheduled at **M8** (where search is served; M6 is
mid-flight and its scope is frozen) — the retriever keeps results carrying `file_path` so the
server can hash-check without new retriever API. Spec §8.2 updated. Adds the *staleness window*
metric (overview §5.2 Layer 3).

### D15 — Evaluate official MCP Rust SDK `rmcp` (Δ3)  · **Adopted** (plan: M8 entry) — *overview §2.5*
Spec §10.2's "Custom (no SDK yet)" assumption is stale: an official MCP Rust SDK (`rmcp`,
modelcontextprotocol org) now exists. At M8 entry, spike it for API stability; adopt and pin a
version if sound, else keep the hand-rolled JSON-RPC plan. New dep needs manager sign-off per
engineering standards. Spec §10.2 updated.

### D16 — Benchmark-suite evaluation replaces "5 real tasks" (Δ4)  · **Adopted** (plan: M10 + research track) — *overview §4–§5*
The "≥40% token reduction on 5 tasks" criterion convinces nobody (claude-context markets the
same number) and benchmarks the wrong baseline (file dumping instead of grep-in-a-loop). v0.1
success becomes: **Layer-2 dominance over grep-only (arm A0) at matched Layer-1 retrieval
recall, with bootstrap CIs**, measured per overview §5 (ContextBench-Lite, three metric layers,
arms A0–A5). M10 keeps the systems budgets as release gates and adds Layer-1 scoring; the
Layer-2 headline is the research track's R3 exit. Spec §1.3/§9.3 tables updated.

### D17 — M7 dev-deps `assert_cmd` + `predicates` for CLI E2E  · **Ratified for v0.1** (plan: M7, dev-only) — *spec: §10.3 (dev-deps)*
The M7 CLI E2E slice (M7.4) drives the built `codecache` binary end-to-end (init → index → query)
and asserts on stdout/stderr + process exit codes. `assert_cmd` (locate + run the cargo bin, capture
output, assert exit status) and `predicates` (its `stdout`/`stderr` matcher vocabulary) are the
idiomatic Rust pairing for this; rolling our own `std::process::Command` harness would re-invent
exactly these matchers with less clarity. **Manager sign-off: APPROVED, dev-dependencies only** —
they ship in no release artifact, do not touch the lean runtime dep set (§10.3 runtime list
unchanged), and are scoped to `tests/`. Precedent: `proptest` (M0) + `criterion`/`tempfile` were
approved on the same "test-only, keeps Cargo.toml runtime-lean" basis. Pin to current minor
(`assert_cmd = "2"`, `predicates = "3"`); `Cargo.lock` holds exact versions for CI cache parity.
Owner: manager (sign-off) + devops (CI parity) + test-lead (usage in `tests/cli_tests.rs`,
`tests/e2e_cli.rs`). Recorded in `docs/plans/M7-formatter-cli.md` deviations.

### D18 — additive `Config::save` for the `codecache config` write path  · **Ratified for v0.1** (plan: M7.3) — *spec: §7.2 (`config`), §7.3*
M7.3's `config` command must read AND write settings (§7.2 lists `config` as "Manage
configuration"), but the M1 `config` module shipped only `Config::load(&Path)`. **Decision: add an
additive `Config::save(&self, path: &Path) -> Result<(), ConfigError>`** that serializes the in-memory
`Config` back to `.codecache/config.toml` via `toml::to_string` (the same serializer `app::init`
already uses), mapping a serialize/write failure to `ConfigError` (no panic, no reachable
`unwrap/expect`). This is purely additive — it does not change `load`, the §7.3 schema, or any
existing caller. `config` semantics for M7.3: no args ⇒ print the current resolved config (read);
`config <KEY> <VALUE>` ⇒ set a documented top-level/scalar key + persist via `save`. Owner: eng-lead
(impl under `config`) + manager (this decision + spec note). project_plan §7.2 updated to pin the
read/write `config` behavior; `src/config/CLAUDE.md` records the new `save` API at GREEN.

### D7 (re-verified at M7 entry) — line-number seam is real and fully wired  · **Confirmed 2026-06-12** — *spec: §4.1, §4.3*
The M7 formatter plan flagged D7 ("store `start_line`/`end_line` at index time") as a seam to
verify before slicing. **Verification result: the seam exists end-to-end; no gap, no fix needed.**
Evidence chain: (1) `Chunk` carries `start_line`/`end_line: usize` (1-based inclusive) —
`src/types/mod.rs:30-33`, pinned by `chunk_carries_all_documented_fields_incl_line_range_and_enrichment`.
(2) Schema declares both as **UNINDEXED** columns — `src/storage/schema.rs:38-39` (`CREATE_SYMBOLS`);
INSERT writes them (`queries.rs` `INSERT_CHUNK` cols 11-12) and SEARCH selects + maps them back into
the reconstructed `Chunk` (`storage/mod.rs` `build_search_result`, `start_line/end_line` cols 10-11).
(3) **Both** chunker paths populate real values: the AST path from Tree-sitter
(`src/parser/mod.rs:309-310` → `start_position().row + 1` / `end_position().row + 1`), the heuristic
path via `chunker::line_range` counting newlines (`src/chunker/mod.rs:256,300-310`). So the M7
formatter reads stored line numbers straight off the `Chunk` in each `SearchResult` — **zero source
file reads at format time**, honoring the §11.2 format budget (D13 `codecache_outline` at M8 reuses
the same stored fields). Owner: manager (verification).

---

## Deferred to v0.2+ (from project_plan §9.2)
Embeddings retrieval (D1), call-graph analysis, additional languages (Rust/Java/C++), real-time
file watching, web UI, multi-repo support, full HTTP/LSP integrations (D4).
