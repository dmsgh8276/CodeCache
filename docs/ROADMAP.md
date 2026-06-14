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
  **NOT yet met (the release gate itself):** `v0.1.0` is **not tagged/published** — publish is human-gated and
  staged behind 4 pre-publish steps (crates.io `codecache` name conflict, real repository URL,
  `CARGO_REGISTRY_TOKEN`, tag push). Reviewer APPROVED all four slices (M10.1/M10.2/M10.4 one BLOCK→fix→APPROVE
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

---

## Deferred to v0.2+ (from project_plan §9.2)
Embeddings retrieval (D1), call-graph analysis, additional languages (Rust/Java/C++), real-time
file watching, web UI, multi-repo support, full HTTP/LSP integrations (D4).
