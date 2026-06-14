# CodeCache — Engineering Plan

The *how* of building CodeCache. The *what/why* lives in [`project_plan.md`](project_plan.md);
milestones in [`ROADMAP.md`](ROADMAP.md); the test scenario matrix in
[`TEST_STRATEGY.md`](TEST_STRATEGY.md); the live checklist in [`TODO.md`](TODO.md). The
strategic positioning + research track (D12–D16, R1–R4) live in
[`../project_overview.md`](../project_overview.md).

This project is built **test-first (TDD)** by a coordinated agent team. Every production line
exists to satisfy a test written before it.

---

## 1. The Agent Team

| Agent | Owns | Responsibility |
|---|---|---|
| `principal-engineering-manager` | plan, ROADMAP, TODO, all CLAUDE.md | Orchestrates the team, sequences work, writes task briefs, verifies alignment, gatekeeps "done". |
| `principal-test-engineering-lead` | `tests/`, TEST_STRATEGY | Writes failing tests first (RED) for every slice. |
| `principal-engineering-lead` | `src/` | Implements minimum idiomatic Rust to go GREEN, then refactors. |
| `code-reviewer` | the quality gate | Independent diff review; APPROVE/BLOCK before done. |
| `performance-bench-engineer` | `benches/` | Owns criterion benches + the perf budgets; guards regressions. |
| `rust-treesitter-specialist` | parser/chunker/FTS5 internals | Deep help on grammars, AST edge cases, FTS5 tuning. |
| `devops-release-engineer` | `.github/`, releases | CI parity with local gates; versioning; crates.io releases. |

Agent definitions live in `.claude/agents/`. The operating manual is `.claude/CLAUDE.md`.

---

## 2. Module Ownership & Build Order

Modules are built **bottom-up** so each slice's dependencies already exist (and are tested).
Source: module responsibility table in `project_plan.md` §3.1; layout in §10.4.

```
config  ─┐
storage ─┼─► indexer ─► (uses parser→chunker, hasher) 
hasher  ─┤
parser ─► chunker ─┘
                      retriever ─► formatter ─► cli ─► mcp_server
```

Dependency-ordered milestones (see `ROADMAP.md` for entry/exit criteria):

| Order | Module(s) | Depends on |
|---|---|---|
| M0 | project scaffolding, CI | — |
| M1 | `config`, `storage` (SQLite schema + FTS5) | scaffolding |
| M2 | `hasher` (xxHash3-128) | storage |
| M3 | `parser` (Python first) | scaffolding |
| M4 | `chunker` (AST boundaries) | parser |
| M5 | `indexer` (discovery → parse → chunk → hash → store) | storage, hasher, parser, chunker |
| M6 | `retriever` (FTS5 BM25 + token budget) | storage |
| M7 | `formatter` (TOON/JSON/text) + `cli` | retriever, indexer |
| M8 | `mcp_server` (stdio JSON-RPC) | cli/retriever |
| M9 | `parser` TypeScript + Go | parser (Python) |
| M10 | benchmarks + release | all |
| R1–R4 | research track: eval harness → offline ablations → agent-in-loop study → write-up (ROADMAP "Research track"; `project_overview.md` §5–§6) | M8 (M9 can interleave) |

---

## 3. The TDD Workflow (per slice)

Driven by the `/tdd-cycle` skill. One small slice at a time.

1. **Brief** — manager selects the next slice from `ROADMAP.md`, writes scope + the test
   scenarios (from `TEST_STRATEGY.md`).
2. **RED** — test lead writes failing unit/integration/e2e/property tests.
3. **GREEN** — engineering lead implements the minimum to pass; escalates Tree-sitter/FTS5
   depth to the specialist.
4. **REFACTOR** — clean while green; `fmt` + `clippy -D warnings` clean.
5. **PERF** — for perf-critical slices, bench engineer adds/refreshes benches vs budgets.
6. **REVIEW** — code-reviewer APPROVE/BLOCK.
7. **INTEGRATE** — manager verifies alignment, updates `TODO.md` + module `CLAUDE.md`, marks done; devops keeps CI green.

---

## 4. Definition of Done (every slice)

- [ ] Tests were written first and now pass (`cargo test`).
- [ ] `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --check` clean.
- [ ] Public API matches `project_plan.md` §3.2 (or the plan was updated first).
- [ ] In-scope Decision Log behaviors honored (see `ROADMAP.md`).
- [ ] Perf budgets respected where applicable; benches updated.
- [ ] `code-reviewer` APPROVED.
- [ ] `docs/TODO.md` and the relevant `CLAUDE.md` updated in the same change.
- [ ] CI green.

---

## 5. Quality Gates (automated)

Local hooks (`.claude/settings.json`, scripts in `.claude/hooks/`) enforce gates so nothing
red is left behind; CI (`devops-release-engineer`) mirrors them exactly:

| Gate | Local hook | CI |
|---|---|---|
| Format | `PostToolUse` runs `cargo fmt` on every `.rs` edit | `cargo fmt --check` |
| Lint | `Stop`/`SubagentStop` runs `cargo clippy --all-targets -- -D warnings` | same |
| Tests | `Stop`/`SubagentStop` runs `cargo test` | `cargo test --all` |
| Perf | `/bench` skill on demand | scheduled `bench.yml` |

Hooks no-op cleanly before `Cargo.toml` exists, and honor `stop_hook_active` to avoid loops.

**Research track (out-of-crate).** `research/` is Python, not Rust — the four cargo gates above do not
apply there. Its gates are **`ruff` + `pytest`**, run by the `research-harness-engineer` (ROADMAP **D23**)
as part of its tests-first workflow (no hook wires them — they are run in the agent's loop, against the
short-path venv). The research tree ships in no release artifact and touches no `Cargo.toml`
(see `research/CLAUDE.md`).

**Toolchain (local == CI).** The pinned channel in `rust-toolchain.toml` is the **single source
of truth** — currently **1.85.0** (our MSRV; mirrored by `Cargo.toml` `rust-version = "1.85"`).
CI honors that file, so gates run on the same compiler locally and in CI. Bump the channel only
deliberately, in lockstep with `Cargo.toml`'s `rust-version` and `.github/CLAUDE.md`; record the
reason in the ROADMAP Decision Log (see **D10** for the 1.82 → 1.85 rationale).

---

## 5a. Harness Engineering

The team's *operating environment* is engineered deliberately, not just its prompts:

- **Context priming** — a `SessionStart` hook (`.claude/hooks/prime-context.ps1`) injects the
  current milestone + next `docs/TODO.md` items at the start of every session, so agents begin
  aligned without manual lookup.
- **Durable hand-off briefs** — one brief per slice in `.claude/briefs/` is the shared
  blackboard; manager → test-lead → engineering-lead → reviewer each append their section so
  hand-offs survive across cold-starting subagents. Template + protocol in `.claude/briefs/`.
- **Tool/permission scoping** — each agent gets least-privilege tools; `settings.json`
  pre-allows safe `cargo` + read-only `git` so autonomous runs aren't prompt-blocked.
- **Single-orchestrator topology** — only `principal-engineering-manager` holds the `Agent`
  tool; it fans work out to specialists, keeping coordination centralized and auditable.
- **Observability** — `/standup` reports milestone status, gate health, open briefs, blockers.
- **Memory** — durable team decisions/preferences live in project memory (recalled each session).

---

## 6. Engineering Standards

- **Errors**: `Result` + `?`; no reachable `unwrap()/expect()/panic!`; typed errors / `anyhow`.
- **Dependencies**: keep `Cargo.toml` to the §10.3 set; new deps need manager sign-off.
- **Toolchain/MSRV**: `rust-toolchain.toml` channel is authoritative (1.85.0; ROADMAP D10);
  keep `Cargo.toml` `rust-version` in sync. A committed `Cargo.lock` makes resolution reproducible.
- **Hot paths** (parse, hash, FTS5 query): avoid needless allocations/clones; back perf claims
  with criterion runs, not intuition.
- **Determinism**: stable ordering and tie-breaks; reproducible indexing.
- **Robustness**: indexing never hard-fails on malformed source (Decision Log #2).
- **Docs**: code change ⇒ update the local `CLAUDE.md` + `TODO.md` in the same change.

---

## 7. Repository Map (target, per project_plan §10.4)

```
src/{cli,indexer,parser,chunker,hasher,storage,retriever,formatter,mcp_server,config}/
tests/        integration + e2e + property tests, fixtures/
benches/      criterion harnesses
docs/         project_plan, ENGINEERING_PLAN, ROADMAP, TEST_STRATEGY, TODO, assets/
.claude/      agents/, skills/, hooks/, settings.json, CLAUDE.md
.github/      CI/release workflows
```
Every directory above gets a `CLAUDE.md` as it is created (manager-enforced).
