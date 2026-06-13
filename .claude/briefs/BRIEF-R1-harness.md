# BRIEF — R1 / eval-harness SPIKE (recommendation only)

- **Milestone:** R1 — Harness (research track)  ·  **Module(s):** NEW `harness/` (out-of-tree, Python) — NOT the Rust crate
- **Owner (manager):** principal-engineering-manager  ·  **Created:** 2026-06-12
- **Status:** SPIKE ✓ (recommendation complete) · BUILD ▢ (gated on D22 human ratification)
- **Links:** docs/ROADMAP.md "Research track" + Decision Log (PROPOSED D22) · project_overview.md §4.2 (RQ1–RQ4), §5 (benchmarks/metrics/arms), §6 (R1–R4), §7 (risks/kill) · docs/TODO.md "Research track" · tests/retrieval_quality.rs + tests/fixtures/retrieval_quality/micro_suite.json (M10.2 scorer/protocol, D21)

> **This brief is a SPIKE.** It evaluates options and records a recommendation. It builds nothing,
> adds no dependency, writes no harness/production code, touches no `Cargo.toml`. It does NOT commit
> the ~$1K R3 API spend or any paid benchmark/API access — those are downstream human gates.
> The disposition is captured as **PROPOSED D22** in `docs/ROADMAP.md`, held open exactly like D15
> was before human ratification (spike → human ratify → build).

## Goal
Decide how to build the R1 eval harness so R1 can hit its exit — **one task end-to-end in arms
A0/A1/A4 with metrics computed from logs** — at the smallest scope, reusing what M0–M10 already
shipped, and leaving R2–R4 deferred.

---

## 1. Stack: mini-SWE-agent fork vs from-scratch loop

**Recommendation: fork / vendor `mini-SWE-agent` (Python). Do NOT write a from-scratch agent loop.**

### Verification (foreground research, 2026-06-12 — flag anything unverified)
- **mini-SWE-agent** — repo `github.com/SWE-agent/mini-swe-agent` (SWE-agent org / the SWE-bench team),
  **MIT**, **actively maintained** (v2.4.1 dated 2026-06-11, ~5.1k stars). Pure **Python**, deliberately
  minimal: the agent class is **~100 lines**, linear message history, talks to LLMs via **litellm**
  (multi-provider). **Action execution is bash-only BY DESIGN** — it deliberately does *not* use the
  LLM tool-calling API and has **no MCP/tool-plugin layer**. (The "no MCP plugin" point is the research
  agent's well-grounded *inference* from the documented bash-only stance, not an explicit doc denial —
  flagged honestly.) Exact on-disk trajectory file format (JSON `.traj` vs other) was **not confirmed**;
  what is confirmed is that it keeps a complete linear message log = the actual LLM prompts.
- The SWE-agent team's own docs now position **mini-SWE-agent as the successor to full SWE-agent**
  (claimed SWE-bench parity, far simpler). Full SWE-agent (MIT, Python, heavier, YAML/ACI tool bundles)
  is therefore the *wrong* weight class for a solo R1.

### Why fork mini beats from-scratch (decisive reasons, priority order)
1. **The bash-only constraint is a *fit*, not a blocker — because CodeCache already ships the two
   surfaces an agent needs.** mini's hard stance ("the agent calls bash; no tool API, no MCP") means
   you wire an arm by putting a **command on PATH and prompting the agent to use it**. CodeCache already
   has exactly that: the `codecache query` CLI (M7) for A1/A2, `codecache_search`/`update`/`outline`
   over the MCP server (M8). A0 is *literally mini's default* (bash → grep/glob/cat/sed). So forking mini
   gives A0 for free and A1/A4 as "prompt the agent to call `codecache query …`". From-scratch would
   re-implement the loop, the LLM plumbing, retries, and turn accounting that mini already ships and the
   SWE-bench community already trusts.
2. **R4-artifact reproducibility & credibility.** R4's deliverable is an *open, reproducible* artifact.
   A fork of the community-standard minimal SWE agent (MIT) is instantly recognizable, auditable, and
   citable; a bespoke loop is a reviewer liability ("did your harness confound the result?"). The
   research contribution (project_overview §4.3) is the *clean ablation*, and clean means a well-understood,
   off-the-shelf host agent with **only the retrieval interface swapped**.
3. **Effort.** mini is ~100 lines of loop + litellm; we inherit trajectory logging, multi-provider LLM
   access, and turn structure. From-scratch is multi-week yak-shaving with no scientific upside.
4. **Trajectory-logging control.** mini's linear message log already records every prompt/response/turn
   — exactly the substrate Layer-2 metrics need (§5.2). We add a thin per-turn JSONL sidecar (below);
   we do **not** need to *own* the loop to log it.

### The Python-vs-Rust-core boundary (called out explicitly)
- **The harness is Python; the CodeCache core stays Rust. The boundary is a process boundary, not a
  binding.** The harness never links the Rust crate. Each arm shells out:
  - **A0:** mini's native bash → `grep`/`glob`/`rg`/`cat`/`sed` (no CodeCache at all).
  - **A1/A2:** the agent runs `codecache query "<q>" --format json --max-tokens N` (built binary on PATH),
    or drives the M8 MCP server over stdio. A2 = A1 with D3 enrichment ON (a config/index toggle, not a
    code change).
  - **A4 (one-shot top-k):** the harness itself calls `codecache query` ONCE up front and injects the
    result into the agent's initial context with no further index access (loop sees only bash/grep after).
- **Consequence:** zero FFI, zero PyO3, no Python-imports-Rust coupling, no async/sync bridge. The Rust
  crate's "zero-dependency, deterministic single binary" identity (D12/D15) is *preserved* — the binary is
  consumed as a black-box CLI/MCP server exactly as a real coding agent would consume it (which is itself
  the point: it makes the experiment ecologically valid).
- **Maintenance surface:** one Python dev-environment (mini + litellm + a scorer), entirely out-of-tree
  from the crate. It ships in **no release artifact** and touches **no runtime dep** (mirrors the D17
  "test-only, keeps Cargo.toml lean" precedent, extended to "research-only, out-of-crate").

## 2. Arm wiring into CodeCache — what EXISTS vs what R1 builds

| Arm | Interface | Already shipped (reuse) | R1 must build |
|---|---|---|---|
| **A0** | grep/glob/read only (control) | mini's native bash loop | nothing (mini default) |
| **A1** | A0 + `codecache_search` (AST+BM25, plain) | `codecache query` CLI (M7) + `codecache_search` MCP tool (M8); BM25 retriever (M6) | prompt/tool-doc telling the agent the command exists; index the task repo |
| **A2** | A1 with **D3 enrichment ON** | D3 enrichment fields are *already indexed* (M4; `parent_symbol`/`file_docstring`/`imports`/`cross_references` in FTS5) | an enrichment ON/OFF **toggle** to ablate (the index already carries the fields; A2 vs A1 is a query-time/weighting switch — confirm the cleanest toggle is the only design question) |
| **A3** | embedding tool over the **same chunks** | the chunker's chunk boundaries (M4) — same units | **out of R1 scope.** Needs an off-the-shelf code embedder + vector store as a *separate* CLI tool over CodeCache's chunks. Defer to R2/R3 (D1 = embeddings deferred to v0.2; the *research* embedding tool is allowed but is not on R1's exit path). |
| **A4** | one-shot top-k from A2's index (no loop access) | `codecache query` + the index | harness-side wiring: one query up front, inject top-k, then deny further index calls |
| **A5** | A2+A3 hybrid (RRF) | — | **out of R1 scope** (stretch; §7 "cut first"). |

**R1's exit needs only A0, A1, A4** (per ROADMAP/TODO). A2 is *trivially adjacent* (the index already
has the fields), so R1 MAY include the A2 toggle if cheap, but the exit gate is A0/A1/A4. **A3/A5 are
explicitly deferred** — they are the only arms requiring a new model/dependency, which is precisely what
the spike scope and §7 scope-discipline say to cut from R1.

**Already-shipped CodeCache surface each in-scope arm reuses (concrete):**
- Retriever (M6): deterministic BM25 ranking, token-budget packing (`--max-tokens` honored exactly).
- Formatter/CLI (M7): `codecache init|index|query` with `--format json|toon|text`, agent-first ordering (D13).
- MCP server (M8): `codecache serve` stdio + `codecache_search`/`codecache_update`/`codecache_outline`,
  self-healing search (D14). An MCP-driving arm is possible but **bash+CLI is the simpler A1 wiring for
  mini** (bash-only host) — prefer the CLI path for R1, keep MCP for later arms.
- Index size/latency are already within budget (M10: p95 ≈ 0.51 ms, index 12.3 MB), so indexing the R1
  task repo is cheap.

## 3. Trajectory logging + scorer (reuse M10.2; do NOT reinvent)

**Per-turn log (Layer-1 + Layer-2 substrate) — append a JSONL sidecar to mini's message log:**
- turn index; arm id; task id; the action (bash command issued, incl. any `codecache query …`);
- **tokens** prompt+completion this turn and cumulative (Layer-2: *tokens-to-correct-context*,
  *tokens-per-resolved-task*, *tool turns to coverage*);
- **files/symbols surfaced into context this turn** (parsed from the action's output) — this is the set
  scored against gold for *coverage* (Layer-1 Recall@k/Precision@k/F1 at **file** and **block** granularity);
- wall-clock for *time-to-first-relevant-result* (Layer-3, §5.2);
- final outcome (resolved/not) for *tokens-per-resolved-task*.

**Scorer: reuse the M10.2 protocol verbatim.** `tests/retrieval_quality.rs` already defines the exact
metric math (Recall@k / Precision@k / F1@k at file + block granularity, macro-averaged, k∈{1,5,10}) and
`tests/fixtures/retrieval_quality/micro_suite.json` defines the gold-context schema
(`gold_files` + `gold_blocks` = `{file_path, symbol_name}`). **The protocol is the contract**, not the
Rust code. R1's scorer (Python) re-implements the *same formulas* over the *same gold schema*, scoring the
trajectory's "context coverage set" instead of a single retriever call. D21 already promised "R2 swaps in
the real corpus with the scorer unchanged" — R1 honors that by matching the protocol exactly. (Keeping the
canonical metric math in Rust *and* re-using it from Python is fine: the formulas are five lines; the value
is the shared, unit-tested *definition*, which the Rust tests pin.)

## 4. Data acquisition — availability, license, offline-vendorability

**Material update to D21.** D21 (M10, 2026-06-12) recorded that *"the real ContextBench corpus requires
network/LLM access and is not vendorable offline."* The R1 spike re-checked this and the finding has
**changed**: ContextBench (arXiv:2602.05892, **EuniAI/ContextBench**, **Apache-2.0**) ships its
**human-annotated gold contexts as static pre-annotated files on HuggingFace (parquet)** and evaluates
locally; the gold contexts do **not** require LLM construction. *(Caveat, stated honestly: generating new
agent trajectories to score still needs an agent+LLM — that is Layer-2, the R3 spend — but the gold-context
DATA is offline-downloadable. The "offline gold contexts" point is the research agent's well-grounded
reading of the repo docs; verify against the repo README before treating as absolute.)* This means the
D21 blocker is likely **resolved for the data**, and R2 can vendor a real slice. **Proposed: supersede the
data half of D21 in the PROPOSED D22 entry**, keeping D21's scorer/proxy disposition intact.

| Source | Status (verified 2026-06-12) | Use in R1 | Offline / license |
|---|---|---|---|
| **ContextBench / Lite** | repo `EuniAI/ContextBench`, **Apache-2.0**; HF parquet; gold contexts pre-annotated; 500-instance verified subset | Layer-1 gold-context scoring; pick the **single smallest task** for R1 exit | **Offline-downloadable** (data); license OK |
| **CodeRAG-Bench (RepoEval slice)** | repo `code-rag-bench/code-rag-bench`; HF corpora; **RepoEval function-level slice included**; ships BM25 + dense baselines; **license UNVERIFIED** | R2 baseline reproduction (sanity vs published BM25) | offline-capable; **license not confirmed — verify before vendoring** |
| **RepoEval / RepoCoder** | `microsoft/CodeT/RepoCoder`, **MIT**; data shipped as in-repo zips; line/API/function completion over real Python repos | R2 alternative/comparison slice | **offline (MIT)**; caveat: function-level slice reportedly leans on a private lib — verify |
| **SWE-bench Verified** | `princeton-nlp/SWE-bench_Verified`, HF parquet; 500 human-validated instances (repo+base_commit+gold patch+test_patch+FAIL_TO_PASS); **license UNVERIFIED** | R3 downstream Pass@1 only (top arms) — **not R1** | offline; **license not confirmed** |
| **CodeCache micro-suite (M10.2)** | committed: `tests/fixtures/retrieval_quality/micro_suite.json` (15 queries, hand-verified) | **R1's bootstrap gold set** — already in-tree, zero acquisition | offline, in-repo |
| **astchunk (cAST)** | PyPI `astchunk` v0.1.0, **MIT**, Python | R2 chunking-ablation baseline only | offline (MIT) |

**Smallest task set for R1's exit:** **one task.** R1 does NOT need a corpus. The committed M10.2
micro-suite (or a single ContextBench-Lite instance) provides one gold-labeled task; R1 runs that single
task end-to-end through A0, A1, A4 and computes Layer-1 + Layer-2 metrics from the logs. Corpus expansion
is **R2's** job, not R1's.

**Blockers / honest caveats:** (1) CodeRAG-Bench and SWE-bench Verified **licenses are unverified** —
confirm before vendoring either into the artifact. (2) ContextBench's "offline gold contexts" is a
strong-but-inferred reading — confirm against the repo README. (3) None of these block R1's single-task
exit, which can run entirely on the in-tree micro-suite.

## 5. Ownership — research track is "new ownership"

**Recommendation: the main session (you) drives R1 directly; do NOT spin up a new persistent agent for R1.**
Reasons in priority order:
1. **The Rust specialist agents structurally do not fit a Python harness.** test-lead/eng-lead/specialist
   are scoped to the Rust crate + its TDD gates (clippy/fmt/cargo test). A Python harness has none of those
   hooks; routing it through them adds friction with no benefit.
2. **R1 is a thin, exploratory, single-author slice** (fork mini, add a JSONL logger + a Python scorer,
   run one task). It is the kind of glue work a single driver does fastest without hand-off overhead.
3. **The manager (this role) stays the gatekeeper** for scope/DoD/doc-sync and owns the PROPOSED D22 +
   ROADMAP/TODO updates — unchanged.
4. **If R1 grows** (R2/R3 corpus + matrix + ~$1K spend), *then* introduce a dedicated **`research-harness-engineer`**
   agent definition (Python-aware, its own quality gates: `ruff`/`pytest`) — but that is a **D22-deferred
   item**, not an R1 prerequisite. Creating the agent now would be premature scaffolding.

## 6. Scope discipline (§7) — the SMALLEST R1 + its exit test

**Smallest R1 (in scope):**
1. Vendor/fork mini-SWE-agent (MIT) into an out-of-tree `harness/` dir (NOT the crate; gitignored or a
   sibling — manager to decide placement at build time; keep it out of `src/`/`Cargo.toml`).
2. Add a per-turn **JSONL trajectory logger** (§3 fields) around mini's loop.
3. Wire **three arms**: A0 (mini default), A1 (`codecache query` on PATH + tool-doc prompt), A4 (one-shot
   top-k injection, then index access denied).
4. Port the **M10.2 scorer protocol** to Python (same formulas, same gold schema), scoring the trajectory
   coverage set.
5. Run **one** gold-labeled task (in-tree micro-suite instance) through A0/A1/A4; emit Layer-1
   (Recall@k/Precision@k/F1 file+block) + Layer-2 (tokens-to-coverage, tokens-per-task, turns) from logs.

**Concrete exit test (R1 done when):**
> For ONE task, the harness produces three trajectory logs (A0, A1, A4) and a metrics report computed
> *from those logs* showing, per arm: Layer-1 Recall@k/Precision@k/F1 at file + block granularity (vs the
> task's gold context) and Layer-2 cumulative tokens + tool-turns-to-coverage. The run is **deterministic
> in wiring** (fixed model/temp/prompt across arms) and **reproducible from a clean checkout** given an API
> key. No claim is made about which arm wins — that is R3.

**Explicitly deferred (do NOT build in R1):**
- **A2** beyond a cheap toggle, **A3** (embedding tool — needs a model; D1 defers embeddings), **A5** (hybrid RRF). → R2/R3.
- Corpus scale-up, NDCG@10, published-BM25 reproduction. → **R2**.
- Full A0–A5 matrix, 30–50→100 tasks, budget/scale sweeps, bootstrap CIs, **the ~$1K API spend**. → **R3 (human-gated)**.
- Preprint/artifact/workshop. → **R4**.
- A dedicated `research-harness-engineer` agent. → only if R2/R3 warrant it (D22-deferred).

**Null-result / kill-criterion handling (§7):** R1 builds the *measurement apparatus*; it asserts nothing
about outcomes. A rigorous null result (index does not beat grep) is itself the publishable §4.3
contribution. The product kill criterion ("index can't beat grep even for a cheap model on a 1M-LOC repo at
a 2K budget") is an **R3** determination, not R1 — R1 only proves the apparatus can measure it. The harness
must therefore be outcome-agnostic (log everything, decide nothing).

---

## Downstream HUMAN GATES (out of scope for this spike)
1. **Ratify PROPOSED D22** (this disposition) — mirrors the D15 spike→ratify protocol. Until ratified, R1 is not started.
2. **~$1K R3 API spend** — the agent-in-loop study's real cost line item. NOT committed here.
3. **Any paid API / paid benchmark access** — out of scope; confirm CodeRAG-Bench + SWE-bench Verified **licenses** before vendoring.
4. **Model choice** for the harness (litellm-routable) — a deliberate human/cost decision at R3, not R1.

## Deliverable paths (for the main session to commit; manager has no Bash/git)
- This brief: `C:\Users\ehlee\workspace\projects\CodeCache\.claude\briefs\BRIEF-R1-harness.md`
- PROPOSED D22 entry appended to: `C:\Users\ehlee\workspace\projects\CodeCache\docs\ROADMAP.md` (marked **PROPOSED — pending human ratification**, held open like D15 was)

---
## OUTCOME — manager
Spike complete in one synchronous pass (no background agents; foreground research used to verify
mini-SWE-agent + benchmarks on 2026-06-12). Recommendation: **fork mini-SWE-agent**, **main session drives**,
**smallest R1 = one task through A0/A1/A4 with metrics from logs**, reusing the M6 retriever / M7 CLI / M8
MCP surface and the M10.2 scorer protocol. Data finding **updates D21**: ContextBench gold contexts now appear
**offline-downloadable (Apache-2.0)**. No code/deps/Cargo.toml touched. Task #18 marked completed. Awaiting
human ratification of PROPOSED D22 before any build.
