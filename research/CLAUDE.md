# research/ — CLAUDE.md

Research track (R1–R4) artifacts. **Owner:** `research-harness-engineer` agent for **R2+** (ROADMAP
**D23**, adopted 2026-06-14 — sonnet; scope `research/`; gates **ruff + pytest**; process-boundary to the
binary). The main session drove **R1** (D22). The `principal-engineering-manager` stays gatekeeper
(scope/DoD/doc-sync) and the `code-reviewer` is the independent APPROVE/BLOCK gate.

## What lives here
- `r1_harness/` — the R1 eval harness: a fork of mini-SWE-agent (Python) that runs the
  same-agent retrieval-interface ablation (arms A0/A1/A4) against the built `codecache` binary and
  scores Layer-1/Layer-2 metrics from trajectory logs. See `r1_harness/README.md`.

## Rules (different from the Rust crate)
- **Out-of-crate, research-only.** Nothing here is a Rust dependency, ships in a release artifact,
  or touches `Cargo.toml`. The four Rust gates (fmt/clippy/test/build) do not apply; this is Python.
- **Process boundary only.** The harness talks to CodeCache by shelling out to the `codecache`
  binary — no FFI/PyO3. Preserves the zero-dependency single-binary identity (D12/D15).
- **One gold source.** Layer-1 gold contexts come from `tests/fixtures/retrieval_quality/`
  (shared with the Rust M10.2 scorer); the Python scorer ports the M10.2 protocol verbatim (D21).
- **No paid spend without a gate.** R1 runs offline (deterministic/local model). The ~$1K R3 API
  spend and any paid benchmark/API access are separate downstream human gates.
- **Scope discipline (`../project_overview.md` §7):** R1 builds outcome-agnostic apparatus; arm
  winners are an R3 determination, not R1.

## Update rule
Code change here ⇒ update `docs/TODO.md` (research-track section) in the same change, mirroring the
crate's golden rule.
