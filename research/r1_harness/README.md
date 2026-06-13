# CodeCache R1 — eval harness

Research track **R1** (ROADMAP **D22**, ratified 2026-06-13). The smallest harness that
runs **one gold-labeled task end-to-end in arms A0/A1/A4** and computes the Layer-1 / Layer-2
metrics *from the trajectory logs* — the controlled, same-agent comparison of retrieval
*interfaces* that the repositioning (D12) and `project_overview.md` §4–§5 are built on.

This tree is **research-only and out-of-crate**: it ships in no release artifact, adds no Rust
dependency, and does not touch `Cargo.toml`. The harness is **Python**; the CodeCache core stays
**Rust**; they meet at a **process boundary** — the harness shells out to the built `codecache`
binary (no FFI/PyO3, no async bridge). This preserves the zero-dependency single-binary identity
(D12/D15).

## Arms (R1 scope)
| Arm | Retrieval interface | `codecache` in loop | One-shot inject |
|---|---|---|---|
| **A0** | grep/glob/read only (control) | no | no |
| **A1** | A0 + `codecache query` as a tool | yes | no |
| **A4** | one-shot top-k from the index, no loop access | no | yes |

A2 (D3-enrichment toggle), A3 (embedding tool), A5 (hybrid RRF) are **deferred to R2/R3** (D22).

## Layout
```
research/r1_harness/
├── r1harness/
│   ├── scorer.py          # Layer-1: Python port of the M10.2 protocol (Recall/Precision/F1 @k, file+block)
│   ├── trajectory.py      # JSONL turn-log schema + Layer-2 (tokens/turns-to-coverage)
│   ├── corpus.py          # materialise a micro-suite corpus to a real on-disk repo
│   ├── codecache_tool.py  # adapter: shell out to the codecache binary, parse §6.4.2 JSON
│   └── arms.py            # A0/A1/A4 + Task definitions
├── tasks/auth_q1.json     # the R1 single task (gold mirrors the M10.2 fixture verbatim)
├── tests/                 # pytest: scorer (mirrors retrieval_quality.rs), trajectory, corpus
├── requirements.txt       # mini-swe-agent==2.4.1 (runner only) + pytest
└── pyproject.toml
```

The Layer-1 scorer is a **Python port of the M10.2 protocol** pinned by
`tests/retrieval_quality.rs`; `tests/test_scorer.py` mirrors that file's hand-computed metric
tests so the two stay behaviourally identical. The R1 task's gold is loaded from the *same*
`tests/fixtures/retrieval_quality/micro_suite.json` the Rust scorer uses — one gold source, two
scorers (D21: "R2 swaps in the real corpus, scorer unchanged").

## Running the offline tests (no agent, no API, no network)
```bash
cd research/r1_harness
python -m pytest            # scorer + trajectory + corpus unit tests
```
The `codecache_tool` adapter is exercised against the **built binary** (`cargo build --release`
first, or set `$CODECACHE_BIN`).

## Status
- **Done (verified offline):** scorer (+ tests), trajectory schema + Layer-2 extraction (+ tests),
  corpus materialisation (+ tests), the `codecache` tool adapter, A0/A1/A4 + task definitions.
- **Next:** `runner.py` — build mini-SWE-agent's loop with the per-arm tool surface and write a
  trajectory; validate end-to-end **offline** via mini's `DeterministicModel` (no API).
- **Gated:** a live-model run needs a model-backend decision (a free/local model via litellm, or a
  paid key). The ~$1K **R3** API spend is a separate downstream gate — **not** authorised by R1.

## Scope discipline (`project_overview.md` §7)
R1 builds the outcome-agnostic *apparatus* only. **No arm-winner claim is made here** — which
interface wins is **R3**. A rigorous null result is itself a publishable outcome (§4.3).
