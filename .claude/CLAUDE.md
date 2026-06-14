# .claude/ — CLAUDE.md (team operating manual)

How the CodeCache agent team, skills, and hooks fit together. **Owner:** `principal-engineering-manager`.

## Layout
```
.claude/
├── agents/      7 agent definitions (one .md each)
├── skills/      tdd-cycle/, new-module/, bench/, standup/  (each a SKILL.md)
├── hooks/       prime-context.ps1, fmt-on-edit.ps1, check-on-stop.ps1  (PowerShell)
├── briefs/      durable per-slice hand-off briefs (TEMPLATE.md + CLAUDE.md)
├── settings.json  permissions allowlist + hook wiring
└── CLAUDE.md    this file
```

## The agents (`agents/`)
| Agent | Model | Role |
|---|---|---|
| principal-engineering-manager | opus | Orchestrator/PM; owns plan, ROADMAP, TODO, all CLAUDE.md; gatekeeps "done". |
| principal-test-engineering-lead | opus | Writes failing tests first (RED). |
| principal-engineering-lead | opus | Implements minimum Rust to green; refactors. |
| code-reviewer | opus | Independent APPROVE/BLOCK gate. |
| performance-bench-engineer | sonnet | Criterion benches + perf budgets. |
| rust-treesitter-specialist | opus | Tree-sitter grammars/queries + FTS5 tuning. |
| devops-release-engineer | sonnet | CI parity + releases. |
| research-harness-engineer | sonnet | Python research harness (`research/`, R2+); ruff + pytest; process-boundary to the binary. |

**Delegation:** start non-trivial work with the manager; it writes a brief and routes to the
test lead → engineering lead → (specialist/perf) → reviewer → back to manager.

## The skills (`skills/`)
- `/tdd-cycle <slice>` — drive one red→green→refactor→review cycle.
- `/new-module <name>` — scaffold a module (`src/<m>/mod.rs` + tests + `CLAUDE.md`), tests-first.
- `/bench [name]` — run criterion benches and compare to budgets.
- `/standup` — manager status: milestone, in-progress/next, gate health, open briefs, blockers.

## Harness engineering (how the team operates)
- **Durable briefs** (`briefs/`) — one per slice; the shared blackboard each agent appends to,
  so manager→test→impl→review hand-offs survive cold-starting subagents.
- **Single orchestrator** — only `principal-engineering-manager` has the `Agent` tool; it fans
  out to specialists. Keeps coordination centralized.
- **Permissions allowlist** (`settings.json`) — safe `cargo` + read-only `git` pre-approved.

## The hooks (`settings.json` + `hooks/`)
- **SessionStart** → `prime-context.ps1`: injects the current milestone + next `docs/TODO.md`
  items so each session/subagent starts aligned. No-ops before `docs/TODO.md` exists.
- **PostToolUse** (Edit/Write/MultiEdit) → `fmt-on-edit.ps1`: runs `cargo fmt` when a `.rs`
  file is touched. Non-blocking; no-ops before `Cargo.toml` exists.
- **Stop / SubagentStop** → `check-on-stop.ps1`: runs `cargo clippy -D warnings` then
  `cargo test`; on failure exits 2 to surface output back so red lint/tests aren't left behind.
  Honors `stop_hook_active` (no loops); no-ops before scaffolding.

Scripts use `$env:CLAUDE_PROJECT_DIR` and run under PowerShell `RemoteSigned` (no policy
override). If hooks don't fire, confirm the shell expands `%CLAUDE_PROJECT_DIR%` and that
`powershell` is on PATH.

## Conventions
- Keep agent/skill descriptions trigger-rich so delegation is automatic.
- When hooks change, update CI (`devops-release-engineer`) to keep local/CI gates identical.
- Any change here ⇒ reflect it in `docs/ENGINEERING_PLAN.md` §5 (quality gates) if behavior changes.
