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

### M7 — `formatter` + `cli`  · **DONE 2026-06-12** (M7.1 e360818 · M7.2 50e3eb0 · M7.3 d0d6a0f · M7.4 5c8b3b8)
- **Work**: TOON/JSON/text formatters; `clap` commands `init/index/update/query/status/config/serve`;
  **agent-first output ordering** (signature/skeleton before bodies — D13, text format; TOON locator-only).
- **Exit** ✓: golden-output tests per format (6, JSON round-trips); CLI arg parsing + error/exit-code
  tested (11 cli_tests); e2e `init→index→query` + failure paths through the binary (5 e2e_cli). 141
  tests green, all four gates clean (Rust 1.85). Added D18 (additive `Config::save`) for `config`
  read/write; `serve` is a clean M8 stub. Reviewer APPROVED all four slices (0 findings).

### M8 — `mcp_server`  · **DONE 2026-06-12** (M8.1 08fc6c7 · M8.2 7021b78 · M8.3 66ec107 · M8.4 42f6575)
- **Entry** ✓ **RESOLVED (D15, 2026-06-12)**: hand-roll JSON-RPC 2.0 over stdio for v0.1
  (`serde`/`serde_json` only — no new runtime deps); `rmcp` re-evaluated at v0.2 behind the D4
  transport seam. Human-ratified; eval in `.claude/briefs/BRIEF-M8-mcp-server.md`.
- **Work** ✓: stdio MCP adapter (line-delimited JSON-RPC 2.0, `initialize` handshake, -32700/-32601/
  -32602/-32603 error mapping); tool registration (`codecache_search`, `codecache_update`,
  `codecache_outline` — D13) with exact §8.2 schemas; `tools/call` round-trip; **self-healing search**
  (hash-check + transparent re-index/evict of result files at query time — D14, `StalenessStats`
  metric hook); `serve` command (stdio wired; SSE → clean "unsupported in v0.1", D4 seam). New
  additive `Storage::symbols_for_path` (D19) backs the outline tool.
- **Exit** ✓: protocol handshake + all three tool round-trips tested against an in-memory mock client;
  all three tools registered with §8.2 schemas; a query against a stale file returns fresh content
  (D14). 19 `mcp_tests` + 3 D19 `storage_tests` + 1 `e2e_cli` serve test; **166 tests green**, all four
  gates clean (Rust 1.85). Reviewer APPROVED all four slices.

### M9 — TypeScript + Go parsers ✅ DONE (2026-06-12)
- **Work**: add `tree-sitter-typescript` and `tree-sitter-go` configs + queries.
- **Exit**: per-language fixture suites green; language coverage = Python/TS/Go. **MET.**
- **Shipped**: M9.1 TS (`fa0c0705`), M9.2 Go (`7f6823f4`), M9.3 mixed-repo validation. Per-language
  `recognize_definition` dispatch + `.scm` (§5.3); TS function/arrow/class/method, Go
  function/method+receiver/struct→`Struct`; D2/D7 parity; byte-exact spans. `181 tests green`, all
  four gates clean (Rust 1.85). Decisions: `LANGUAGE_TYPESCRIPT` (`.tsx` deferred); interfaces/type
  aliases not emitted; no enum/dep change. Brief: `.claude/briefs/BRIEF-M9-typescript-go.md`.

### M10 — Benchmarks + Release  · **M10.1–M10.3 DONE 2026-06-12 · M10.4 STAGED (publish human-gated)** (M10.1 92fe491 · M10.2 5650596 · M10.3 9ceb324 · M10.4 cf5a3d3)
- **Work**: criterion suite vs all systems budgets; **Layer-1 retrieval-quality scoring**
  (ContextBench-Lite gold contexts + hand-verified micro-suite — D16 replaces the old
  "5 real tasks" benchmark); release workflow; crates.io publish.
- **Exit**: systems budgets met (p95<500ms, index<100MB, incr<2s); Layer-1 retrieval metrics
  recorded (the Layer-2 token-economy headline is the research track's R3 exit, not a release
  gate); `v0.1.0` tagged and published; install smoke test passes.
- **Status** ✓ (budgets/quality/CI met; release staged): query p95 = **0.51 ms**, index = **12.3 MB**,
  incremental = **190 ms**, cold-100K = **13.5 s**, hash 1K = **459 ms** — all PASS; **cold-10K = 6.04 s
  vs <5 s MISS** (tracked, **D20**, v0.1.x txn-batching, not a release blocker). Layer-1 scoring shipped
  as a 15-query offline proxy (**D21**; keyword Recall@10=1.000, semantic=0.000 = expected BM25 gap, D1).
  EXPLAIN QUERY PLAN baseline captured (FTS5 index used, no full scan). `bench.yml` (scheduled) + `release.yml`
  (fires only on human-pushed `v*` tag) authored; **196 tests green**, all four gates clean (Rust 1.85).
  **NOT yet met (the release gate itself):** `v0.1.0` is **not tagged/published** — publish is human-gated.
  Pre-publish blocker #1 (crates.io name conflict) is **RESOLVED 2026-06-17 (D30):** crate renamed
  `codecache` → `codecache-rs` (binary stays `codecache`), validated green (224 tests, dry-run exit 0). Three
  human-gated steps remain (real repository URL, `CARGO_REGISTRY_TOKEN`, tag push). Reviewer APPROVED all four
  slices (M10.1/M10.2/M10.4 one BLOCK→fix→APPROVE
  each). Brief: `.claude/briefs/BRIEF-M10-benchmarks-release.md`.

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
| **R2 — Offline ablations** ✅ **CLOSED (R2.7, D29, 2026-06-16)** | Layer-1 sweeps: chunking × ranking × enrichment on **ContextBench-Lite** (RepoEval slice CUT — D27) | **MET (softened — D27):** real-corpus Layer-1 ablation over ContextBench-Lite (R2.7: 10 tasks, 2 repos, file-level NDCG@10) + qualitative CodeRAG-Bench BM25 NDCG@10 = 0.932 reference; BM25 vectors separate on NDCG, chunker A/B directional + no winner asserted (D29) |
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

### D15 — Evaluate official MCP Rust SDK `rmcp` (Δ3)  · **RESOLVED 2026-06-12: hand-roll JSON-RPC over stdio for v0.1; do NOT adopt `rmcp`** (plan: M8) — *overview §2.5; eval: `.claude/briefs/BRIEF-M8-mcp-server.md`*
Spec §10.2's "Custom (no SDK yet)" assumption was stale: an official MCP Rust SDK (`rmcp`,
modelcontextprotocol org) now exists. The M8-entry evaluation is complete and the **human has
ratified the disposition: hand-roll JSON-RPC 2.0 over stdio for v0.1 using only `serde`/`serde_json`
(already in the tree — zero new runtime deps).** Decisive reasons, in priority order:
1. **MSRV conflict with the deliberate 1.85.0 pin (D10).** `rmcp` declares no `rust-version`, is
   developed on a 1.92 toolchain, documents a 1.90 minimum, and uses `edition = "2024"` (1.85 is the
   absolute floor, unverified). Adopting it breaks the 1.85 contract or forces pin-chasing — exactly
   the whack-a-mole D10 rejected.
2. **Zero-dependency identity (D12 / §10.3).** `rmcp` drags in tokio + schemars + async-trait +
   proc-macro trees (~dozens of crates) onto the *one* optional surface; CodeCache's durable wedge is
   "zero-dependency, deterministic, single static binary, air-gapped."
3. **Async-over-sync friction (D8).** `rmcp` forces a tokio runtime onto a synchronous SQLite core;
   correct use needs `spawn_blocking` bridging. Hand-roll has zero async/sync boundary.
4. **Modest, frozen scope.** stdio + 3 tools + handshake is ~250–450 LOC over `serde_json` we already
   ship — well within our TDD discipline and cheaper to own than to bridge.

**Re-evaluate `rmcp` at v0.2**, when SSE/HTTP transports (D4) and richer protocol features make the
SDK's breadth pay for itself and an MSRV bump can be a deliberate choice. `mcp_server` stays behind
the **D4 transport-agnostic seam** so swapping in `rmcp` later is an adapter change, not a refactor.
New dep needs manager sign-off per engineering standards — none required here. Spec §10.2 updated.

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

### D19 — additive `Storage::symbols_for_path` for the `codecache_outline` tool  · **Ratified for v0.1** (plan: M8.3) — *spec: §3.2.2, §8.2 (Tool 3)*
M8.3's D13 `codecache_outline` returns a file/directory **symbol skeleton straight from the index**
(name, type, parent, `start_line`–`end_line`) with **zero source reads** (D7). The existing storage
API has no path-scoped symbol lookup — `search` is FTS5 `MATCH`-ranked (wrong access path for "all
symbols in this path"), and `all_indexed_files` returns only paths, not symbols. **Decision: add an
additive read-only `Storage::symbols_for_path(path: &Path) -> Result<Vec<SymbolOutline>>`** that
`SELECT`s the skeleton columns directly off the contentful `symbols` FTS5 table
(`symbol_name, symbol_type, parent_symbol, file_path, start_line, end_line`) `WHERE file_path = ?1`
(exact file) **OR** `file_path LIKE ?2` with a directory prefix (`<dir>/%`, with SQL `LIKE` wildcards
in the path escaped), ordered deterministically by `(file_path, start_line, end_line)`. A plain
column `SELECT` on a contentful FTS5 table is valid and reads the UNINDEXED line columns with no
re-parse and no file I/O (D7). Returns a small `SymbolOutline` row struct (or reuses a slim
projection) — **not** a full `Chunk` (the skeleton needs no `chunk_text`/`imports`, keeping the
result lean for the §11.2 budget). Purely additive: no change to `search`, `insert_chunks`, the
schema, or any existing caller; no new dependency. Owner: eng-lead (impl) + rust-treesitter-specialist
(FTS5 `SELECT`/`LIKE`-escaping detail) + manager (this decision + the §3.2.2/§8.2 spec note).
project_plan §3.2.2 gains the method signature and §8.2 Tool 3 notes the index-only lookup; the
`mcp_server` outline handler formats these rows via the M7 text skeleton-line shape (D13). RED for
M8.3 drives the new method + the `tools/call` outline round-trip.

### D20 — cold-index 10K-LOC budget miss: tracked, deferred to v0.1.x  · **Adopted at M10.1** (plan: M10.1) — *spec: §5.4*
M10.1 measured the cold-index hot path at the §5.4 budget checkpoints (Windows 11, Rust 1.85,
release). Result: **10K LOC = 6.04 s (p50) vs the < 5 s budget — MISS**; 100K LOC = 13.54 s vs
< 30 s — PASS (>2× headroom); incremental 10-file = 190 ms vs < 2 s — PASS; index size = 12.3 MB
vs < 100 MB — PASS; query p95 = 0.51 ms vs < 500 ms — PASS; hash 1K files = 459 ms vs < 500 ms —
PASS. The 10K result is **non-monotonic vs budget** (the larger 100K corpus clears its budget while
the smaller 10K does not clear its tighter one), which points at **fixed per-iteration overhead**
dominating at 10K scale: each `pipeline::index_file` calls `Storage::insert_chunks`, which opens and
commits **its own SQLite transaction per file** (`storage/mod.rs:124-149`), so a 200-file index pays
~200 commit fsyncs; plus a cold DB file is created per bench iteration (Windows fsync cost not present
on Linux CI). This is the exact per-file-transaction overhead the M5.2 TODO flagged for "M10
profiling." **Decision: do NOT block the v0.1 release on this.** Rationale: (1) the harder absolute
target (100K < 30 s) passes with margin; (2) the M10 plan's assertion policy explicitly anticipated
a possible 10K miss and prescribes record-the-number + trend-track in `bench.yml` rather than a hard
CI assert (machine variance); (3) the fix — batch inserts across files into one transaction (or wrap
`index_all` in a single transaction) — is a **production change that must be a deliberate, test-first
slice preserving the D2 per-file isolation guarantee**, not scope-creep folded into a benches/
release-prep milestone. **Follow-up (v0.1.x, tracked in `benches/CLAUDE.md` + `docs/TODO.md`):**
cross-file transaction batching in the indexer to bring 10K cold-index under 5 s; re-measure. The miss
is documented honestly in the release notes (CHANGELOG, M10.4). Owner: manager (this decision) +
performance-bench-engineer (measurement) + engineering-lead (the future optimization slice).

**RESOLVED 2026-06-17 (v0.1.x perf slice; test-first; reviewer-APPROVED; uncommitted at hand-off).**
The follow-up shipped as a full TDD cycle (brief: `.claude/briefs/BRIEF-M10-D20-batch-inserts.md`).
**Fix:** an additive storage primitive `Storage::write_in_transaction<T, F>(items, each) ->
Result<Vec<Result<()>>>` (plan §3.2.2) runs the indexer's per-file work inside **one outer
transaction**, isolating each file in its own **SAVEPOINT** (`Ok` ⇒ RELEASE, `Err` ⇒ ROLLBACK TO +
record + CONTINUE), committing the outer tx once. `BatchWriter<'a>` lends `insert_chunks` /
`delete_chunks_for_file` / `update_file_hash` against the current savepoint over the same D8
connection (no re-lock/deadlock). `indexer::index_all`/`update_files` drive all changed/new files
through one such call; `detect_changed_files` still runs first, so unchanged files open no savepoint
(idempotency held). **D2 preserved naturally** — the savepoint-per-file design means a parse-, read-,
or DB-stage per-file failure rolls back only that file and is counted-as-skipped, the batch returns
`Ok`, and siblings commit (verified by RED tests at both the storage primitive and indexer surfaces;
no existing test weakened). One additive, internal `StorageError::BatchItem(String)` variant carries
the rolled-back-item signal (never surfaced; the indexer just skip-counts). **Measured (this WSL2/
Linux machine, Rust 1.85, release):** 10K cold-index p50 **5.84 s → 1.37 s (−76.5%)**, p95 1.57 s —
both well under < 5 s here (the unbatched 5.84 s closely matches the Win11 6.04 s, confirming commit/
fsync count was the bottleneck, not parse/FTS5). **Windows CI is the authoritative budget gate** — the
`benches/CLAUDE.md` budget table stays MISS-on-Win11-pending until a Windows CI/local run confirms
< 5 s there; the speedup mechanism (≈200 fsyncs → 1) is platform-independent, so a Windows pass is
strongly expected. Owners as above; reviewer APPROVED (savepoint commit/rollback semantics verified
against rusqlite 0.32.1 source).

### D21 — M10.2 Layer-1 scoring ships a 15-query offline micro-suite proxy (not the full ContextBench / not 5×15)  · **Adopted at M10.2** (plan: M10.2; research: R2) — *overview §5.1–5.2; Decision Log D16*
D16 reframed v0.1 evaluation to Layer-1 retrieval-quality scoring vs gold contexts, with **no hard
gate at M10** ("recorded vs gold"; the Layer-2 dominance headline is R3). M10.2 delivers a
deterministic, offline scorer (`tests/retrieval_quality.rs`) computing Recall@k / Precision@k /
F1@k at **file** and **block(function)** granularity over the public `Retriever` surface, plus a
committed gold-context fixture (`tests/fixtures/retrieval_quality/micro_suite.json`, the single
source of truth, loaded via `serde_json`). **Two scope realities, stated plainly:** (1) the real
ContextBench corpus (arXiv:2602.05892) requires network/LLM access and is **not vendorable offline**
here — the committed fixture is a hand-verified **micro-suite proxy** using the *identical* scoring
protocol, so R2 swaps in the real corpus with the scorer unchanged; (2) the fixture is **3 corpora ×
5 queries = 15 queries**, ~5× under the plan's aspirational "5 repos × ~15 queries" (≈75). **Decision:
accept the 15-query proxy as the v0.1 deliverable.** Rationale: the v0.1 value is the *reusable,
unit-tested scorer + protocol*, not the sample size; there is no hard gate; expanding to the full
corpus is exactly R2's job. **Measured (offline, deterministic, 2026-06-12; @k=10 macro):** keyword
queries (N=13) file Recall = 1.000 / F1 ≈ 0.510, block Recall = 1.000 / F1 ≈ 0.494; semantic queries
(N=2, e.g. "error handling") file Recall@10 = 0.000 — the expected **BM25-only semantic recall gap**
(**D1** informational, the v0.2 hybrid rationale; quantified properly at R2/RQ2), NOT a gate.
Qualitatively in-range vs published BM25 baselines (CodeRAG-Bench). **Follow-up (R2):** expand to the
real ContextBench-Lite corpus + the full 5×~15 micro-suite using this scorer; add NDCG@10. Owner:
manager (this decision) + performance-bench-engineer (scorer) — recorded in
`.claude/briefs/BRIEF-M10-benchmarks-release.md`.

### D22 — R1 eval-harness: fork mini-SWE-agent; main session drives; smallest single-task R1  · **Adopted 2026-06-13 (human-ratified)** (plan: research track R1) — *overview §4.2, §5–§7; spike: `.claude/briefs/BRIEF-R1-harness.md`*

> **Spike → human ratify → build (the D15 pattern), now ratified.** The R1-entry spike is complete and
> the disposition below was **ratified by the human on 2026-06-13**; R1 build is underway, driven by the
> main session. The ~$1K R3 API spend and any paid benchmark/API access remain **separate downstream
> human gates** — not authorized by this ratification (R1's single-task wiring runs on a free/local or
> deterministic model, no paid API).

The research track (project_overview §5–§6) needs an agent harness to run the A0–A5 retrieval-interface
ablation. R1's exit is narrow: **one task end-to-end in arms A0/A1/A4, metrics computed from logs.** The
spike evaluated stack, arm wiring, logging/scorer reuse, data, ownership, and scope.

**Recommended stack — fork/vendor `mini-SWE-agent` (Python, MIT); do NOT write a from-scratch loop.**
Decisive reasons, in priority order:
1. **Bash-only is a *fit*, not a blocker.** mini-SWE-agent is deliberately bash-only (no LLM tool-API,
   no MCP plugin layer — verified 2026-06-12; the "no MCP" point is a grounded inference from its documented
   stance). CodeCache **already ships the surfaces an agent calls**: A0 is mini's default (bash→grep/glob/cat),
   A1/A4 are "the agent runs `codecache query …`" via the M7 CLI on PATH (or the M8 MCP server). The host
   agent consumes the binary as a black box — which is the experiment's whole point.
2. **R4 reproducibility/credibility.** A fork of the community-standard minimal SWE agent (MIT, actively
   maintained — v2.4.1 2026-06-11; the SWE-agent team now positions mini as SWE-agent's successor) is
   auditable and citable; a bespoke loop is a "did your harness confound the result?" liability for the
   clean same-agent ablation that *is* the contribution (overview §4.3).
3. **Effort & logging.** We inherit the ~100-line loop, litellm multi-provider access, and a linear
   trajectory log; we add only a thin per-turn JSONL sidecar. From-scratch re-derives all of this for no
   scientific gain.

**Language/runtime boundary (explicit):** the harness is **Python**; the CodeCache core stays **Rust**.
The boundary is a **process boundary** (shell out to the built `codecache` binary / MCP server), **not**
FFI/PyO3 — zero async/sync bridge, zero new crate dep, the D12/D15 zero-dependency single-binary identity
preserved. Research-only, out-of-crate, ships in no release artifact (extends the D17 "test-only, keep
Cargo.toml lean" precedent to "research-only").

**Arm wiring — already-shipped vs R1-builds.** Reuse: M6 retriever (deterministic BM25, `--max-tokens`),
M7 `init|index|query` CLI + agent-first ordering (D13), M8 MCP server + 3 tools + self-healing (D14), D3
enrichment fields already indexed (M4). R1 builds only: the JSONL turn-logger; the A1 tool-doc prompt;
the A4 one-shot-then-deny wiring; a Python port of the M10.2 scorer protocol. **R1's exit needs only
A0/A1/A4.** **A2** (D3 toggle) is trivially adjacent and MAY be included if cheap; **A3** (embedding tool —
needs a model; D1 defers embeddings) and **A5** (hybrid RRF) are **explicitly deferred to R2/R3**.

**Logging + scorer:** log per turn — action (incl. any `codecache query`), prompt+completion tokens
(cumulative), files/symbols surfaced into context, wall-clock, outcome — the substrate for Layer-1
(Recall@k/Precision@k/F1 at file+block) and Layer-2 (tokens-to-coverage, tokens-per-task, turns-to-coverage).
**Reuse the M10.2 scorer PROTOCOL verbatim** (`tests/retrieval_quality.rs` + `micro_suite.json` gold schema:
`gold_files` + `gold_blocks={file_path,symbol_name}`); the Python scorer re-implements the same five-line
formulas over the same gold schema — the protocol (pinned by the Rust unit tests) is the contract, honoring
D21's "R2 swaps in the real corpus, scorer unchanged."

**Data — updates D21's offline finding.** D21 (M10) recorded the real ContextBench corpus as *not vendorable
offline*. Re-checked at this spike (2026-06-12): **ContextBench (arXiv:2602.05892, `EuniAI/ContextBench`,
Apache-2.0)** ships **human-annotated gold contexts as static parquet on HuggingFace** and evaluates locally —
the gold-context DATA appears **offline-downloadable** (generating new agent trajectories still needs an
agent+LLM = the R3 Layer-2 spend; that distinction stands). **This proposal supersedes the *data* half of D21**
(the corpus is now vendorable), while **D21's scorer/micro-suite-proxy disposition stays intact.** *Honest
caveat:* the "offline gold contexts" reading is a grounded inference from the repo docs — confirm against the
repo README before treating as absolute. **CodeRAG-Bench (RepoEval slice; BM25+dense baselines) and SWE-bench
Verified are offline-capable but their LICENSES are UNVERIFIED — confirm before vendoring.** RepoEval/RepoCoder
is MIT-offline; astchunk (cAST) is MIT-offline (R2 chunking baseline). **Smallest R1 task set = ONE task** —
the in-tree M10.2 micro-suite already provides a gold-labeled task; no corpus acquisition is on R1's exit path
(corpus expansion is R2).

**Ownership:** **the main session drives R1 directly; do NOT create a new persistent agent now.** The Rust
specialist agents (clippy/fmt/cargo-test gates) structurally do not fit a Python harness; R1 is thin,
single-author glue work; the manager stays gatekeeper for scope/DoD/doc-sync. **If R2/R3 grow** (corpus +
full matrix + ~$1K spend), introduce a dedicated **`research-harness-engineer`** agent (Python gates:
ruff/pytest) then — a **D22-deferred** item, not an R1 prerequisite.

**Smallest R1 + exit test.** Build: vendor mini (out-of-tree), add JSONL logger, wire A0/A1/A4, port the
M10.2 scorer, run ONE gold-labeled task. **Exit (R1 done when):** for one task, three trajectory logs
(A0/A1/A4) + a metrics report computed *from the logs* giving per-arm Layer-1 Recall@k/Precision@k/F1
(file+block, vs gold) and Layer-2 cumulative tokens + turns-to-coverage; fixed model/temp/prompt across arms;
reproducible from a clean checkout given an API key. **No outcome claim is made** — which arm wins is R3.
**Null result / kill criterion (§7):** R1 builds the outcome-agnostic *apparatus* only; a rigorous null
result is itself publishable (§4.3); the product kill criterion is an **R3** determination, not R1.

**Downstream human gates (NOT in this proposal):** ratify this D22; the **~$1K R3 API spend**; any paid
API/benchmark access; confirming CodeRAG-Bench + SWE-bench Verified licenses; the R3 model choice.
Owner: manager (proposal + spike) → main session (R1 build). **Ratified 2026-06-13; R1 build underway.**

**R1 DONE (2026-06-13).** Harness built and run end-to-end. *Offline* (DeterministicModel, byte-stable):
A0/A1/A4 each drive mini's loop on `auth_q1` and cover the gold block. *Live* (zero-cost, local Ollama
`qwen2.5:7b`, temp 0): **all three arms cover the gold block** — A1's in-loop `codecache query` returned
the gold symbol `authenticate_user` at **rank 1 on turn 1**. Two findings worth carrying into R2/R3:
(1) Ollama **native** tool-calling is too fragile for this 7B model on the in-loop arm (empty responses →
`RepeatedFormatError`, zero actions); the **text-based** model class drives all arms reliably and is also
what the no-native-tools models (llama3/phi3) need — selectable via `run_live.py --model-class
{litellm|litellm_textbased}`. (2) Fixed a measurement bug — grep's `./` path prefix double-counted a file,
corrupting Recall@1 (+regression test; pytest 38→39). **No arm-winner claim — that is an R3 determination.**
Live trajectories + reports under `research/r1_harness/runs/` (gitignored); the native-vs-text-based runs
are preserved locally as `runs/live_run{1,2,3}_*`.

### D23 — R2 offline ablations: reproduce one published BM25 baseline + pick top configs; introduce `research-harness-engineer`  · **Adopted 2026-06-14 (human-ratified)** (plan: research track R2) — *overview §5–§7; spike: `.claude/briefs/BRIEF-R2-offline-ablations.md`*

> **Spike → human ratify → build (the D15/D22 pattern), now ratified.** The R2-entry spike is complete and
> the disposition below was **ratified by the human on 2026-06-14** with two choices: (a) **ungated apparatus
> first** — build R2.1–R2.4 (NDCG@10 scorer, BM25 weight-sweep + the small crate flag, chunker seam, ablation
> reporter) over the in-tree micro-suite, and decide the external-corpus gates (license/network/astchunk —
> R2.5–R2.7) afterward; (b) **introduce the `research-harness-engineer` agent** to own the R2–R4 build. The
> ~$1K R3 API spend and any paid benchmark/API access remain separate downstream R3 gates — **not** authorized
> here (R2 is zero-spend offline Layer-1 scoring; no agent-in-loop, no LLM).

The research track (overview §5.3) needs the chunking × ranking × enrichment ablation R2 owns. R2's exit is
narrow and offline: **reproduce a published BM25 baseline within tolerance on a named corpus slice, and select
the top retrieval config(s)** — no agent, no LLM, no Layer-2.

**Smallest R2 + exit.** Run the CodeCache retriever offline through the R1 harness, scored by the R1 Layer-1
scorer extended with **NDCG@10** (the CodeRAG-Bench metric R2 adds), over a **~12-cell ablation**:
{CodeCache AST chunks vs cAST/astchunk} × {~3 per-column BM25 weight settings} × {D3 enrichment ON/OFF}.
**Exit:** on a named public slice with a published BM25 number, the retriever reproduces it within a stated
tolerance (proposed: **CodeRAG-Bench RepoEval function-level slice; ± 0.03 absolute** on the headline metric;
fallback **ContextBench-Lite**, Apache-2.0), AND the ablation table is emitted with a stated top-config
criterion (proposed: **highest-and-separated beyond the noise floor**; ties reported as ties) naming the
promoted config(s) — R3's agent-in-loop inputs. Deterministic, reproducible from a clean checkout.

> **EXIT AMENDED by D27 (2026-06-15):** the "reproduce a published BM25 baseline within ±0.03 on a named
> corpus slice" exit is **dropped** (R2.5b/R2.7 in-repo reproduction CUT — the CodeRAG-Bench RepoEval data is
> HF-gated and its 20-line-window methodology validates a generic BM25 apparatus, not CodeCache's AST-symbol
> chunking). **Softened exit:** run the real-corpus Layer-1 ablation over **ContextBench-Lite** (Apache-2.0,
> R2.5a — DONE) + cite CodeRAG-Bench's published **BM25 NDCG@10 = 0.932** qualitatively (no in-repo
> reproduction). Next research step = **R2.6** (astchunk/cAST chunker), GATED on the astchunk dep. See D27.

**Reuse vs new.** Reuse the R1 scorer/protocol (Recall/Precision/F1 @k file+block — M10.2 contract, D21), the
gold schema, `corpus.py`, `codecache_tool.py` (process boundary), `report.py`'s pure-emit pattern. New:
**NDCG@10**; an **external-corpus loader** mapping published gold → our gold schema (scorer unchanged, D21); a
**chunker-swap seam** + the **astchunk** baseline; a **per-column BM25 weight-sweep driver**; the
**baseline-reproduction + ablation runner**.

**Staging — ungated first.** UNGATED (start on ratification; no external data/dep; TDD vs in-tree gold):
(R2.1) NDCG@10 scorer extension; (R2.2) BM25 weight-sweep scaffolding; (R2.3) chunker-swap seam with a stub
chunker; (R2.4) ablation-table reporter. GATED (each on a specific human decision): (R2.5) vendor the external
slice — license + network/HF; (R2.6) the astchunk dep; (R2.7) the baseline-reproduction exit run — depends on
R2.5 + the chosen published number. If a gate stalls, R2.1–R2.4 still ship the tested apparatus over the
micro-suite (R2 is not all-or-nothing).

**BM25 weights live in the crate (verified).** The per-column `bm25()` weights are hardcoded in `src/storage`
(`symbol_name` 10.0, `parent_symbol` 5.0, rest 2.0/1.0; `ORDER BY bm25 ASC, rowid ASC`) with **no CLI/config
surface** (config exposes only `bm25_k1`/`bm25_b`). So R2.2's weight sweep **requires a small test-first crate
change** to expose the 7 weights (a CLI flag / config key, `Cargo.toml` untouched) — the one place R2 touches
the Rust crate, routed through the normal TDD team + manager gate. Flagged so it is not a build-time surprise.

**Cuts (§7, to R3+).** RQ4 (freshness), SWE-ContextBench, SWE-bench Verified, arm A3 (embedding tool — D1
defers embeddings), arm A5 (hybrid RRF), all Layer-2 token economy + bootstrap CIs + the ~$1K spend, and
line-level granularity. R2 keeps file+block granularity + adds only NDCG@10.

**Ownership — introduce `research-harness-engineer` now (proposed).** R1 (D22) had the main session drive and
pre-authorized this agent "if R2/R3 grow"; **R2 is that growth point.** Stand up a dedicated
**`research-harness-engineer`** (model: sonnet; scope: `research/` only; gates: **ruff + pytest**;
process-boundary to the binary; honors research/CLAUDE.md — never touches the crate/`Cargo.toml`). The manager
stays gatekeeper for scope/DoD/doc-sync. (Lighter alternative: defer the agent to R3.)

**Human gates (NOT in this proposal):** ratify this D23; verify the CodeRAG-Bench/RepoEval **license** (fall
back to ContextBench-Lite Apache-2.0 if unclear); authorize the **research-harness network/HF download** of
benchmark data (separate from the product's air-gapped guarantee); approve the **astchunk** research dep;
confirm the **scope cuts + the named published baseline/tolerance**. The **~$1K R3 spend** and any paid access
stay R3 gates. Owner: manager (proposal + spike) → research-harness-engineer (R2 build, on ratification).

### D24 — CLI-reachable per-column BM25 weight override (`--bm25-weights` / `QueryOptions.bm25_weights` / `Storage::search_with_weights`)  · **Adopted 2026-06-14** (plan: research track R2.2a) — *spec: §3.2.2, §3.2.3, §7.2 (`codecache query`)*
The 7 per-column FTS5 `bm25()` weights were hardcoded in `src/storage/queries.rs::SEARCH`
(`symbol_name` 10.0, `parent_symbol` 5.0, the rest 2.0/1.0) with **no CLI/config surface** — the
config's `bm25_k1`/`bm25_b` are inert w.r.t. FTS5's `bm25()` (D23 verified this). R2's weight-sweep
(R2.2b) drives the retriever over the `codecache_tool.py` **process boundary**, so it must vary the
weights **per `codecache query` invocation without recompiling**. **Decision: add a CLI-reachable
per-invocation override** — the **one place R2 touches the Rust crate** (flagged in D23), built
test-first through the normal TDD team, `Cargo.toml` untouched. Surface (additive, plan-amended
before code): `QueryOptions.bm25_weights: Option<[f64; 7]>` (the `[f64; 7]` array makes "exactly 7
weights" a **compile-time invariant** below the CLI parse boundary); `Storage::search_with_weights(
&str, usize, Option<&[f64; 7]>)` (`search` delegates with `None`); CLI `query --bm25-weights
"w0,…,w6"` (7 comma-separated f64 in `schema::CREATE_SYMBOLS` indexed-column order). **`None` ⇒ the
built-in defaults (10,1,1,5,2,2,2), byte-identical to pre-R2.2a** — the `None` path reuses the
unchanged `queries::SEARCH` const verbatim. FTS5 `bm25()` weights are **auxiliary-function
arguments** that **cannot be `?`-bound**, so the 7 f64 are **formatted** into the ranking expression
(`{:?}` → always a valid, locale-free SQLite numeric literal) — injection-safe **only** because each
is a validated finite f64, never raw CLI text (`MATCH ?1`/`LIMIT ?2` stay parameterized). Guards:
the CLI rejects wrong-arity / non-numeric / non-finite input as a typed error → clean nonzero exit
(no panic); a defensive `StorageError::NonFiniteWeight` blocks a non-finite weight from ever reaching
SQL. Zero/negative weights are **allowed** (FTS5 honors them; the sweep wants them). MCP
`codecache_search` stays default-weighted (builds `QueryOptions` via `..Default::default()`). Owner:
manager (this decision + the §3.2.2/§3.2.3/§7.2 amendment) → test-lead (RED) → eng-lead (GREEN) +
rust-treesitter-specialist (FTS5 bind-vs-format) → code-reviewer (APPROVE, 0 findings). 208 tests
green; all four gates clean (Rust 1.85). Brief: `.claude/briefs/BRIEF-R2.2a-bm25-weights-flag.md`.

### D25 — CLI-reachable chunk-ingestion seam (`codecache ingest <CHUNKS_JSON>` / `app::ingest_chunks`)  · **Adopted 2026-06-14 (human-ratified, spike→decision)** (plan: research track R2.3a) — *spec: §3.2.4, §3.2.2, §7.1/§7.2*

> **Spike → human ratify → build (the D15/D22/D23 pattern), now ratified.** The R2.3-entry spike (main
> session) is complete and this disposition was **ratified by the human on 2026-06-14**. Zero-spend, offline,
> additive; `Cargo.toml` untouched. This is the **single crate touch** for R2.3 (flagged in D23); the
> harness-side stub chunker + A/B plumbing is the pure-`research/` follow-on **R2.3b**.

R2's chunker ablation (D23) must vary the chunker while holding storage + ranking + enrichment constant.
**Spike findings:** the chunker is an index-time **hardcoded free fn** (`chunker::chunk(tree, source,
lang)`) — not a trait, not swappable — and the research harness is **process-boundary-only** (shells to the
built binary; no FFI/PyO3). So the only way to ablate the chunker is a **CLI-reachable ingestion path** that
inserts **caller-supplied, pre-chunked** records directly, bypassing discover→parse→chunk, so any external
chunker's output (an in-harness stub at R2.3b; astchunk/cAST at the gated R2.6) flows through CodeCache's
**same** `Storage` + FTS5-BM25 + `Retriever`. `Storage::insert_chunks(&[Chunk])` already exists
(single-transaction batch); the seam reuses it.

**Decision (surface, plan-amended before code):**
- A new **hidden** `codecache ingest <CHUNKS_JSON>` subcommand (clap `hide = true`; research-only, not in
  `--help`) — **not** an `index --from-chunks` mode (`index` = discover/parse/chunk, a mature tested
  contract; overloading it muddies a clean command and risks regression). `--db-path` like the others.
- An `app::ingest_chunks(project_root, chunks_json) -> Result<IngestStats, AppError>` facade beside
  `index`/`init` (`cli/ingest.rs` is the thin handler).
- A **format-local input DTO** (in the ingesting module) deserializing a JSON **array of chunk records** →
  `types::Chunk`. **serde stays OFF `types::Chunk`** (D4/D5 transport separation — mirrors
  `formatter::json::JsonChunk`). Required fields: `symbol_name, symbol_type, file_path, start_byte,
  end_byte, start_line, end_line, chunk_text, language`; optional+defaulted: `parent_symbol` (null),
  `file_docstring` (null), `imports` (`[]`), `cross_references` (`[]`), `is_heuristic` (false). Enum strings
  via the existing total, no-panic `SymbolType::from_str_lenient` / `Language::from_str_lenient`. The DTO is
  **fuller** than the §6.4.2 query-output JSON (which is lossy — omits line range + enrichment) because
  R2.3b holds **enrichment constant** to isolate chunking. Full schema in §7.2.

**Behavior + invariants.** Insert in **JSON-array order** (rowid order ⇒ deterministic `bm25 ASC, rowid
ASC` tie-break). Write one `files_metadata` row per **distinct** `file_path` (so `status` and the D19
`codecache_outline` work on an ingested DB — hash is a deterministic sentinel since a fresh DB per arm makes
the real content hash irrelevant), then restamp `total_files`/`total_chunks`. **Re-ingest / incremental /
idempotency is OUT of scope** — the harness `init`s a fresh DB per arm. `is_heuristic` is passed in-memory
but the schema still has no column (the deferred M5/M7 seam), so a round-trip reconstructs `false`. Empty
input (`[]`) is a valid no-op (exit 0). Malformed JSON / missing required / unknown enum / wrong type →
typed `AppError` → clean **nonzero exit**, no reachable `unwrap/expect/panic` (the R2.2a/D24 validation
pattern).

**Constraints.** Additive only (`index`/`query`/`update`/`status`/`config`/`serve` + all existing tests
stay green). **`Cargo.toml` UNTOUCHED** — serde + serde_json are already deps. **No new `Storage` method**
(reuses `insert_chunks` / `update_file_hash` / `set_index_state`; totals computed from the ingested data or
recomputed via `all_indexed_files`+`get_file_meta`). Owner: manager (this decision + the §3.2.4/§3.2.2/§7
amendment) → test-lead (RED) → eng-lead (GREEN) → code-reviewer (APPROVE). The Rust crate change is
gatekept by the normal TDD team; **research-harness-engineer does NOT touch the crate** (R2.3b is its pure
`research/` follow-on). Brief: `.claude/briefs/BRIEF-R2.3a-chunk-ingestion-seam.md`.

### D26 — R2.5 external-corpus gates: Corpus = BOTH (ContextBench-Lite + CodeRAG-Bench RepoEval); Network/HF = AUTHORIZED (one-time cached download, zero paid spend)  · **Adopted 2026-06-15 (human-ratified, spike→ratify→build)** (plan: research track R2.5) — *overview §5–§7; D23 GATED items R2.5–R2.7*

> **Spike → human ratify → build (the D15/D22/D23/D25 pattern), now ratified.** D23 deferred the external-corpus
> gates (license #1, network/HF #2) pending a human decision; this entry records the **ratified disposition
> (2026-06-15)**. Zero paid spend, pure `research/`, additive. This authorizes a **one-time, cached, no-auth
> HuggingFace download** for the research harness **only** — the **product stays air-gapped** (the
> zero-dependency single-binary identity, D12/D15, is unchanged; nothing here is a Rust dependency or ships in a
> release artifact). The **~$1K R3 API spend** and any paid benchmark/API access remain **separate downstream R3
> gates — NOT authorized by this ratification.**

R2's exit (D23) needs (a) a real-corpus Layer-1 ablation table and (b) a published-BM25-baseline reproduction.
The human chose **BOTH corpora**, each for the job it fits:
- **ContextBench-Lite** — the real-corpus ablation table (cleanest license). **Apache-2.0** (confirmed:
  `github.com/EuniAI/ContextBench`, paper arXiv:2602.05892). HF dataset `Contextbench/ContextBench`, **parquet**;
  verified/"Lite" subset = **500 tasks** (config `contextbench_verified`; full = 1,136). Human-annotated gold
  contexts (file/block/line). **No static BM25 baseline** — its native eval is agent-trajectory based, so we use
  it **ONLY as a Layer-1 gold corpus** (our scorer over its static gold), **NOT for number-reproduction.**
- **CodeRAG-Bench RepoEval (function slice)** — the published-BM25 reproduction + cAST comparability.
  `github.com/code-rag-bench/code-rag-bench`, paper arXiv:2406.14497 (NAACL'25 Findings). Data license
  **CC-BY-SA 4.0** (per paper/release — **the engineer MUST confirm the exact LICENSE file as the first R2.5b
  build step**; the GitHub-API license read 504'd through the env proxy this turn). **NDCG@10 is its primary
  retrieval metric**; the **RepoEval function** split is the reported one; BEIR format
  (`corpus.jsonl`/`queries.jsonl` + qrels) on HF. This is the source of the published BM25 NDCG@10 number **R2.7**
  reproduces (±0.03 absolute, per D23). RepoEval/RepoCoder underlying data is **MIT**.

**Network/HF authorization (scoped).** Authorize adding `datasets` + `huggingface_hub` to the research venv and a
**one-time, cached corpus download** — **no auth token, zero paid spend.** Tests run against the cached/fixture
slice, **never re-downloading** (hermetic + deterministic — follows the `runs/` ignore precedent). **Environment
de-risked this turn:** PyPI + huggingface.co are reachable (HTTP 200) from the build/bash env;
`datasets`/`huggingface_hub` are not yet installed; Python is system `/usr/bin/python3` 3.12.3 (the engineer
decides venv-vs-system install and records it in requirements + `research/CLAUDE.md`).

**Licensing housekeeping (HARD).** **Download-and-cache locally (gitignored); do NOT vendor** the CC-BY-SA data
into the tracked tree (avoids share-alike redistribution obligations). ContextBench Apache-2.0 is fine either
way but follows the same cache-not-vendor pattern for consistency. Provenance/attribution notes go in
`research/CLAUDE.md` (or a NOTICE).

**Sub-slicing (mirrors R2.2/R2.3).** **R2.5a (BUILD NOW):** dependency + loader framework + the Apache-2.0
**ContextBench-Lite loader** — a pure-logic, binary-free, network-free mapper (unit-tested against a tiny
inline/fixture sample) that maps ContextBench-Lite gold → the existing **`SweepQuery` shape** (`corpus_id`,
`query_id`, `query`, `query_type`, `gold_files: frozenset[str]`, `gold_blocks: frozenset[(file_path,
symbol_name)]`) so it drops into `score_vectors`/`run_ab`/the R2.4 reporter **unchanged** (the "scorer
unchanged, D21" constraint); network confined to a thin **fetch entrypoint** that downloads once → pins a
**cached slice** under a gitignored cache dir. Honest scope note: ContextBench-Lite gives a real-corpus
ablation, **NOT** a published-number reproduction. **R2.5b (NEXT, separate slice):** the CodeRAG-Bench RepoEval
**BEIR loader** (`corpus.jsonl`/`queries.jsonl` + qrels → `SweepQuery` gold); confirm the CC-BY-SA LICENSE; this
is what **R2.7** reproduces the BM25 NDCG@10 against. Owner: manager (this decision + the gate ratification) →
research-harness-engineer (R2.5a build, then R2.5b). Brief:
`.claude/briefs/BRIEF-R2.5-external-corpus-loader.md`.

> **SUPERSEDED-IN-PART by D27 (2026-06-15):** R2.5b (the CodeRAG-Bench RepoEval BM25-reproduction loader) is
> **CUT/de-scoped**. The "Corpus = BOTH" decision stands **only for ContextBench-Lite** (R2.5a, DONE); the
> CodeRAG-Bench RepoEval half — including the in-repo published-number reproduction — is dropped. R2's exit is
> softened accordingly (see D27). Only ContextBench-Lite proceeds; CodeRAG-Bench's BM25 NDCG@10 = 0.932 is
> retained as a **qualitative published reference**, not an in-repo reproduction.

### D27 — De-scope R2.5b (CodeRAG-Bench RepoEval reproduction); soften R2's exit to a real-corpus ablation over ContextBench-Lite + a qualitative 0.932 reference  · **Adopted 2026-06-15 (human-ratified, spike→ratify)** (plan: research track R2; supersedes-in-part D23 exit + D26 "Both corpora" gate) — *overview §5–§7*

> **Spike → human ratify (the D15/D22/D23/D25/D26 pattern), now ratified.** A spike this turn
> (principal-ml-eval-engineer, main session) verified the facts below; the human ratified the de-scope on
> 2026-06-15 via gate questions. Pure documentation; zero code, zero spend, product air-gapped unchanged.

**Decision.** **R2.5b (the CodeRAG-Bench RepoEval BM25-reproduction loader) is DE-SCOPED / CUT.** R2's exit
criterion is **softened**: from D23's *"reproduce a published BM25 baseline within ±0.03 absolute on a named
corpus slice"* → to *"run the real-corpus Layer-1 ablation over **ContextBench-Lite** (Apache-2.0, R2.5a —
DONE), and cite CodeRAG-Bench's published **BM25 NDCG@10 = 0.932** qualitatively as a reference number (no
in-repo reproduction)."* The next research step becomes **R2.6** (astchunk/cAST baseline chunker), which
remains **GATED on the astchunk PyPI dependency** (D23 gate #3) — **not authorized yet**.

**Rationale (the *why*).** A spike verified three facts that made strict RepoEval reproduction
low-value/high-friction:
1. **Methodology mismatch.** CodeRAG-Bench's RepoEval gold is a **20-line code window**, not a symbol; BM25
   hits 0.932 *because* the query is lexically near that window. Reproducing 0.932 requires replicating
   **their** 20-line chunking + BM25 — which does **not** exercise CodeCache's **AST-symbol** chunking. So a
   faithful reproduction validates a generic BM25/scorer apparatus, not CodeCache's retrieval — orthogonal to
   R2's purpose.
2. **Gated data.** `code-rag-bench/repoeval` on HF returns **401 (gated — token/terms required)**, unlike the
   open corpus pools; sourcing it needs either an HF token+terms or a multi-source generate-from-RepoCoder
   build. Friction not justified by (1).
3. **Block-scoring mismatch.** RepoEval gold has no symbol names (the same issue as ContextBench), forcing
   file-level or a chunk-ID proxy.

ContextBench-Lite (R2.5a, DONE, reviewer-APPROVED, commit `ee918d1`) already provides a clean, well-licensed
real-corpus ablation, so R2 keeps its real-corpus result without the RepoEval cost.

**Verified external facts (cite; do NOT re-derive — confirmed this turn).**
- **CodeRAG-Bench BM25 NDCG@10 = 0.932 (93.2)** on RepoEval — paper Table 3, arXiv:2406.14497. Retained as a
  **qualitative published reference**, not an in-repo reproduction.
- **CodeRAG-Bench data license = CC-BY-SA-4.0** — confirmed via HF Hub API `cardData.license` + `license:`
  tags + README front-matter across `code-rag-bench/{library-documentation,github-repos,github-repos-python}`
  (the GitHub repo's missing LICENSE file was a red herring — it governs code, not the HF data). This
  **closes** the D26/R2.5b "confirm the LICENSE first" open item: license is now known (CC-BY-SA-4.0), and the
  loader that would have consumed it is cut.
- **`code-rag-bench/repoeval` is gated (HF 401), not a public packaged dataset**; the org hosts only retrieval
  *corpus pools* (11 datasets), and RepoEval queries/qrels are generated by the repo's `create/` scripts from
  MIT RepoCoder data.

**Amendments to prior decisions.**
- **D23 exit criterion amended:** the "reproduce a published BM25 baseline within ±0.03 on a named corpus
  slice" exit is replaced by the softened exit above. R2.5b/R2.7's in-repo reproduction is no longer R2's exit
  path; the ContextBench-Lite real-corpus ablation (R2.5a, DONE) + the qualitative 0.932 reference satisfy R2.
- **D26 superseded-in-part:** D26 is kept intact as the historical record but annotated above as
  superseded-in-part — its "Corpus = BOTH" disposition now holds **only for ContextBench-Lite**; the
  CodeRAG-Bench RepoEval half (R2.5b loader + R2.7 reproduction) is dropped.

**What stands unchanged.** Zero paid spend; the product (codecache binary) stays fully air-gapped (D12/D15);
the ContextBench-Lite one-time-cached HF download authorized at D26 is unaffected; the ~$1K R3 API spend and
any paid benchmark/API access remain **separate downstream R3 gates — NOT authorized here**. The **astchunk
PyPI dependency (R2.6) is the next human gate — NOT yet granted.** Owner: manager (this decision + the doc
sync) → research-harness-engineer (R2.6 build, once the astchunk dep is human-approved). Brief:
`.claude/briefs/BRIEF-R2.5-external-corpus-loader.md` (R2.5b section marked CANCELLED per D27).

### D28 — R2.6 astchunk/cAST baseline chunker: native vs astchunk TIE at file level (Recall@10 saturation), block-level diverges  · **Adopted 2026-06-15 (R2.6 closeout, reviewer-APPROVED)** (plan: research track R2.6; builds on D23 gate #3 + D25 ingest seam + D27 softened exit) — *overview §5.3, §7*

R2.6 replaces the R2.3b stub chunker with the **astchunk** PyPI package (cAST baseline; **MIT,
0.1.0** — the **D23 gate #3 dependency, human-GRANTED**) dropped into the **same** R2.3b A/B
plumbing over the **D25 `codecache ingest` seam**, so storage + FTS5-BM25 + retriever + enrichment
are held constant and the chunker is the only ablated axis. `r1harness/astchunk_chunker.py` wraps
`astchunk.ASTChunkBuilder` → materialize-consistent D25 ingest records (synthesised
`"{file}::L{start}-L{end}"` symbol names, a `function` sentinel `symbol_type`, all enrichment
defaulted — astchunk emits none); `run_ab_astchunk.py` is the entrypoint and `ab_runner.py` drives
the native-vs-astchunk A/B over the same scorer + same gold. astchunk 0.1.0 supports
**Python/TypeScript only** → **Go is skipped** (no grammar). Pure `research/`, **zero crate change,
zero spend**; `Cargo.toml`/`src/`/Rust-`tests/`/`.claude/settings.json` untouched.

**Result (directional, PROXY — NOT a published finding; n=10 over python+typescript).** At
**file-level granularity native and astchunk TIE**: NDCG@10 file = **0.800**, Recall@10 file =
**0.800**, F1@10 file = **0.413** — driven by **Recall@10 saturation** (top-10 ≈ the whole ≤9-chunk
micro-suite corpus, the same saturation D21/D23/D24/the R2.4 reporter found for the weight sweep).
But the **block-level metric diverges** (native **0.800** vs astchunk **0.000**) because astchunk's
synthesized `file::L{s}-L{e}` block keys cannot match gold function-name keys — proving the two arms
**genuinely chunk differently**, so the file-level tie is a **corpus-size artifact, not a no-op**.
This is the empirical case (already argued in D23/D27) that a **real corpus is required to separate
the chunkers** — it **sets the stage for R2.7** (the softened-exit real-corpus ablation over
ContextBench-Lite, D27, now carrying the astchunk chunker as the chunker axis).

**Dependency + hermeticity.** astchunk verified **MIT** and pinned in
`research/r1_harness/requirements.txt` with its Tree-sitter transitives (tree-sitter 0.25.2,
tree-sitter-python 0.25.0, tree-sitter-typescript 0.23.2, numpy, pyrsistent; java/c-sharp grammars
pulled but unused). Unlike the R2.5a fetch deps, **astchunk is imported at test runtime**, so the
research suite now **requires the venv** (`research/r1_harness/.venv`) — it FAILS on the bare system
`python3`. Green baseline = **138 passed, 1 skipped** (the skip = the Windows-only path test); ruff
check + format clean. Env requirement documented in `docs/TESTING_AND_USAGE.md` §3.0 + `research/CLAUDE.md`.

**Reviewer APPROVE, 0 blockers** (independently re-ran the venv suite + ruff, recomputed the
byte-offset invariant including the TS space-padded fallback path — benign because `chunk_text` is
the FTS5-indexed column while `start/end_byte` are UNINDEXED tie-break-only, confirmed the D25 field
set matches the Rust `IngestChunk` DTO, verified no crate touch). Three NON-blocking findings tracked
in `docs/TODO.md`: (#1, optional hardening) whitespace-only input yields a degenerate zero-width
record violating the wrapper's own `end_byte>start_byte` property — **not reachable in the actual
run** (both corpora produce 0 such records); (#2) the TODO doc-sync, reconciled by this commit; (#3
nit) a test re-defines a production helper locally. Cross-references **D23** (R2 staging + gate #3),
**D25** (the ingest seam this rides), **D27** (the softened exit + R2.7 next). Owner: manager (this
decision + doc-sync) + research-harness-engineer (R2.6 build) + code-reviewer (APPROVE).

### D29 — R2.7 scoped real-corpus exit run: ContextBench-Lite (10 tasks, 2 repos) is the **R2 EXIT**; R2 track **CLOSED**  · **Adopted 2026-06-16 (human-ratified scoped run + gated build route, reviewer-APPROVED after a BLOCK→fix→APPROVE on a measurement bug)** (plan: research track R2.7; satisfies the D27 softened exit; builds on D24 `--bm25-weights` + D25 `ingest` + D28 astchunk arm) — *overview §5–§7*

R2.7 is the **softened R2 exit (D27)**: a **scoped, directional** real-corpus Layer-1 ablation over
**ContextBench-Lite** (`contextbench_verified`, Apache-2.0, EuniAI; arXiv:2602.05892) — **NOT** the full
500 and **NOT** a ±0.03 reproduction of any published number (R2.5b CUT, D27). It closes the **R2.5a
"mapper-only" gap**: R2.5a's `contextbench.py` mapped records → `SweepQuery` (query + gold labels) only —
it **did not materialize a searchable corpus**. The Lite HF schema ships `repo`/`repo_url`/`base_commit`/
`gold_context`/`problem_statement`/`language`/`patch` but **no repository source and no retrieval pool**;
the searchable corpus exists only by **cloning each task's repo at its `base_commit`** and indexing the
whole tree (indexing only gold files would make recall trivially 1.0). R2.7 builds that materializer.

**Apparatus (NEW, pure `research/`).** `r1harness/contextbench_corpus.py` = a **deterministic task selector**
(filter `language ∈ {python, typescript}` → stable sort by `(repo, instance_id)` → greedy repo admission to
`max_repos` → task cap to `max_tasks`) + a **corpus materializer** (`git clone --no-checkout` once per repo
into a **gitignored** `cache/contextbench_repos/`, then `git worktree add <task_dir> <base_commit>` per task;
clone/worktree failure → typed `CorpusMaterializeError` / skip-with-log, no crash; idempotent re-run).
`run_contextbench_exit.py` = the run entrypoint (missing-cache → clean nonzero exit). **R2.7 needs
network + git** for the one-time clones (D26 envelope — research-harness only, zero paid spend); **the
product (codecache binary) stays fully air-gapped**, and the **pytest suite stays hermetic** (selector +
materializer pure helpers + git-failure tested with mocks; the real clone+index is the integration RUN, not
a unit test). No crate change — rides the existing `--bm25-weights` (D24) and `ingest` (D25) seams.

**Result (directional, scoped — n=10 over 2 repos; NO winner asserted).** Selected **10 tasks**: 5 ×
`pytest-dev/pytest` (python) + 5 × `vuejs/core` (typescript) — astropy excluded for clone/index cost
(~13 min/task on the debug binary). **Run 1 (BM25 6-vector sweep, native chunker)** at file-level NDCG@10:
`body_heavy` **0.197** (best) > default/flat/name_strong **0.173** > enrich_heavy 0.160 > name_only 0.153;
**Recall@10 saturates flat at 0.233 across all 6 vectors** — so the **D21/D28 Recall-saturation finding
persists on the real corpus, but the NDCG ordering is no longer masked** (unlike the D28 micro-suite where
5/6 vectors tied on NDCG). **Run 2 (chunker A/B, default weights, under PROPER per-arm DB isolation)** at
file-level: native NDCG@10 **0.173** vs astchunk **0.249**; Recall@10 native 0.233 vs astchunk 0.367. The
gap is **real but small, n=10, and language-confounded**: the 3/10 astchunk-wins are all python (pytest);
the typescript arm (vuejs/core) is mostly both-zero — a **`.ts`-only file cap excludes `.tsx`/`.vue`**, the
leading hypothesis for vuejs gold files going unindexed, so the TS near-zero is a **coverage artifact, not a
chunker signal**. **No winner asserted** — a defensible claim needs R3 scale (≥50 tasks/language on the
release binary).

**Measurement bug caught + fixed in review (the value of the BLOCK gate).** The first Run-2 build ran BOTH
arms against the **same `task_dir`**: native `init`+`index` then astchunk `init`+`ingest`. Because
`codecache init` is idempotent (no DB reset) and `ingest` **appends** (`insert_chunks`, no DELETE), the
astchunk arm queried the **union of native + astchunk chunks** — a comparison that trivially cannot lose; the
original +44% was an **artifact**. Reviewer **BLOCKED**; the fix isolates each arm into its own scratch dir
(`native_{i}/`, `astchunk_{i}/`, each a fresh `.codecache/`, identical source file set via `_copy_source_files`)
mirroring the reviewed `ab_runner.run_ab_astchunk` pattern. Re-run under isolation reproduced the corrected
table; reviewer **APPROVE**. Also fixed: a `norecursedirs` pytest-collection bug (the cloned repos' own test
files were collected after a run). Gates: **166 passed, 1 skipped** (Windows-only path skip) in the venv,
ruff check + format clean. `Cargo.toml`/`src/`/Rust-`tests/`/`.claude/settings.json` untouched.

**R2 TRACK CLOSED.** This is the last R2 slice; the softened D27 exit is satisfied (real-corpus ablation over
ContextBench-Lite + the qualitative CodeRAG-Bench BM25 NDCG@10 = 0.932 reference, paper Table 3,
arXiv:2406.14497, cited but NOT reproduced). Empirical headline carried into R3: **the real corpus does
separate the BM25 vectors on NDCG (it un-masks the ordering that micro-suite Recall-saturation hid), and the
chunker A/B is directionally astchunk-favoring but too small + language-confounded to assert** — R3 takes the
full A0–A5 matrix to scale. Owner: manager (this decision + doc-sync + commit) + research-harness-engineer
(R2.7 build + run) + code-reviewer (BLOCK→APPROVE). Brief: `.claude/briefs/BRIEF-R2.7-contextbench-exit-run.md`.

### D30 — Crate renamed `codecache` → `codecache-rs` to clear the crates.io name conflict (binary stays `codecache`)  · **Adopted 2026-06-17 (M10.4 release-prep; validated green)** (plan: M10.4 — resolves human-gated blocker #1) — *spec: §10.3 (`[package] name`)*

`cargo publish --dry-run` (M10.4) reported `crate codecache@0.1.0 already exists on crates.io index`
— the **first** of M10.4's four human-gated pre-publish blockers. **Decision: rename only the
crate/package — `Cargo.toml [package] name = "codecache"` → `"codecache-rs"` — and keep the
*binary* named `codecache`.** This is the **single semantic change** (`Cargo.lock` auto-updated its
matching package-name line); `[lib] name`, `[[bin]] name`, the clap command `name`, the README
binary references, the MCP `"command"`, and the research harness's `find_codecache_binary` are all
**deliberately unchanged**.

**Rationale.**
1. **crates.io uniqueness is on `[package] name` only.** The registry enforces a unique *package*
   name; the produced *binary* name is independent. So renaming the package clears the conflict
   while the published binary stays `codecache` — the **ripgrep model** (crate/package name ≠ binary
   name). This keeps the README quickstart, the MCP config, the research harness, and the
   `assert_cmd` e2e tests (`cargo_bin("codecache")`) all unbroken.
2. **`codecache-rs` over `ast-grep-cache`.** An earlier candidate `ast-grep-cache` was **rejected**:
   it collides with the established `ast-grep-*` crate family (incl. `ast-grep-mcp`, an MCP AST-search
   server that overlaps CodeCache's headline feature), implying false affiliation / namespace
   squatting. `codecache-rs` is the conventional `-rs` disambiguator and stays brand-coherent.
3. **Consumer impact is one line.** `cargo install` resolves by *package* name, so the install command
   becomes `cargo install codecache-rs` — and it still installs a binary named `codecache`, so every
   downstream `codecache <cmd>` invocation is unchanged.

**Validation (Rust 1.85.0, 2026-06-17).** `cargo build` ✅; `cargo clippy --all-targets -- -D
warnings` ✅ clean; `cargo test` ✅ **224 passed, 0 failed** (the README's "196" is now stale — the
suite has grown; fix deferred to the release-polish pass); `cargo publish --dry-run` ✅ full verify
from the packaged tarball (234 files / 1.3 MiB compressed, exit 0). *Note:* the dry-run only ENOENTs
at its post-package `stat` when output lands on the `/mnt/c` DrvFs (WSL) mount — a filesystem
artifact, not a manifest defect (passes cleanly on a native ext4 target).

**Disposition.** Resolves M10.4 human-gated blocker **#1** (TODO.md). Blockers **#2** (real
`repository` URL), **#3** (`CARGO_REGISTRY_TOKEN` secret), **#4** (push the `v0.1.0` tag) remain
human-gated. Spec §10.3's `[package] name` excerpt and `docs/CLAUDE_CODE_SETUP.md`'s `cargo install`
command are updated to `codecache-rs` in the same change; README/CHANGELOG package-name + test-count
touch-ups are deferred to the (separate, not-yet-approved) release-polish pass. Owner: manager (this
decision + doc-sync) + main session (the validated `Cargo.toml`/`Cargo.lock` change + local commit).

### D31 — Published-crate `include` allowlist: trim the crates.io tarball to product code only (234 → 52 files)  · **Adopted 2026-06-17 (M10.4 release-prep; validated green)** (plan: M10.4 — leak-proof publish, complements the curated-public-repo plan) — *spec: §10.3 (`[package] include`)*

The M10.4 `cargo publish --dry-run` packaged **234 files** — the **entire repo**, including all of
`research/` (46 files: the paper-pending ablation harness + corpus loaders), `.claude/` (41 files:
the agent definitions, briefs, hooks, settings), and `docs/` (22 files). A **crates.io tarball is
permanent and public** (it cannot be unpublished), so publishing as-is would have **leaked** the
unreleased research track and the internal agent tooling to the world the moment `cargo publish` ran.

**Decision: add an anchored, root-relative `include = [...]` to `Cargo.toml [package]`** that ships
ONLY product code. Final set: `src/**/*.rs` + `src/**/*.scm` (the Tree-sitter queries),
`benches/**/*.rs`, `examples/**/*.rs`, `/README.md`, `/LICENSE-MIT`, `/LICENSE-APACHE`,
`/CHANGELOG.md`, `/CONTRIBUTING.md`, `/rust-toolchain.toml` (Cargo auto-adds `Cargo.toml`/`Cargo.lock`).
Result: **52 files**, verified to contain **no** `research/`, `.claude/`, `docs/`, any `CLAUDE.md`,
`project_overview.md`, `.venv/`, or `cache/`.

**Rationale.**
1. **Permanence + publicity of the registry tarball.** Unlike a git push (revertable, and the team is
   already planning a *curated* public repo that withholds `research/`/`.claude/`/`docs/` until the
   paper), a crates.io release is irreversible and immediately public. Both surfaces — the git repo
   AND the crates.io tarball — must withhold the same things; D30 fixed the package *name*, D31 fixes
   the package *contents*. This is the **publish-side complement** to the curated-public-repo plan.
2. **`include` over `exclude`.** An allowlist (`include`) is fail-safe: a newly added top-level
   directory is **excluded by default** rather than silently shipped, so future research/tooling
   additions cannot leak without an explicit manifest edit. (`exclude` is fail-open — the wrong
   default for a permanent public artifact.)

**Gotcha worth recording (the anchored-glob trap).** Cargo's `include`/`exclude` use **gitignore-style
globs**, where a **bare filename matches at ANY depth** — an unanchored `README.md` pattern slurped
every nested `README.md` across the tree (including the gitignored cloned corpus repos under
`research/`). Fixed by a **leading-`/` anchor** on each root-level pattern (`/README.md`,
`/LICENSE-MIT`, …), which pins them to the crate root. Directory-scoped patterns stay as recursive
`**` globs (`src/**/*.rs`).

**Validation (Rust 1.85.0, 2026-06-17).** `cargo package --list` = **52 files** (leak-guard clean —
manually confirmed no research/.claude/docs/CLAUDE.md/project_overview/.venv/cache entries); full
`cargo publish --dry-run` **compiled from the trimmed tarball**, exit 0. The `include` is
**packaging-only** — it does not affect the local build/test graph, so the suite stays **224**-green
from the D30 commit. Bundled with this change: the stale README status-line test count **196 → 224**.

**Disposition.** Tightens M10.4's pre-publish posture (does not itself clear a human-gated blocker —
those remain #2 real `repository` URL [now partially addressed: human set it to
`AdvancedUno/codecache`, pending confirmation it is the final remote], #3 `CARGO_REGISTRY_TOKEN`,
#4 tag push). Spec §10.3's `[package]` excerpt gains a brief `include` note in the same change.
Owner: manager (this decision + doc-sync) + main session (the validated `Cargo.toml`/`README.md`
change + local commit). Cross-references **D30** (crate rename) and the curated-public-repo plan.

---

## Deferred to v0.2+ (from project_plan §9.2)
Embeddings retrieval (D1), call-graph analysis, additional languages (Rust/Java/C++), real-time
file watching, web UI, multi-repo support, full HTTP/LSP integrations (D4).
