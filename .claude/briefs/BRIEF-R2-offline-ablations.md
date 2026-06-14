# BRIEF — R2 / offline-ablations SPIKE (plan + recommendation only)

- **Milestone:** R2 — Offline ablations (research track)  ·  **Module(s):** `research/r1_harness/` extended (or a sibling `research/r2_ablations/`) — Python, NOT the Rust crate
- **Owner (manager):** principal-engineering-manager  ·  **Created:** 2026-06-14
- **Status:** SPIKE ✓ (plan complete) · BUILD ▢ (ungated slices may start on ratification of PROPOSED D23; corpus/license/dep slices gated)
- **Links:** project_overview.md §5.1 (benchmarks), §5.2 (3 metric layers — **NDCG@10 / CodeRAG-Bench is R2's new Layer-1 metric**), §5.3 (arms + **chunking × ranking × enrichment** ablations = R2's substance), §6 (R2 row: *reproduce published BM25 within tolerance; pick top configs*), §7 (scope discipline) · docs/ROADMAP.md "Research track" + Decision Log (D16/D21/D22; PROPOSED **D23** below) · docs/TODO.md "Research track" R2 line · research/CLAUDE.md (out-of-crate / process-boundary / one gold source / no paid spend without a gate) · `.claude/briefs/BRIEF-R1-harness.md` §4 (data licenses/offline status) + §5 (ownership) · tests/retrieval_quality.rs + tests/fixtures/retrieval_quality/micro_suite.json (M10.2 protocol/gold, D21) · research/r1_harness/r1harness/{scorer,corpus,trajectory}.py (R1 reuse surface)

> **This brief is a SPIKE / PLAN.** It decides the smallest R2, stages the work GATED vs UNGATED,
> and records the disposition as **PROPOSED D23** (held open exactly like D15/D22 were before human
> ratification: spike → human ratify → build). It **builds nothing, adds no dependency, writes no
> harness/production code, touches no `Cargo.toml`, downloads no data, commits nothing.** The ~$1K
> R3 API spend and any paid access stay **R3 gates — explicitly NOT in R2** (R2 is offline Layer-1
> scoring; no agent-in-loop, no LLM spend). Scope discipline (§7) is front and center throughout: the
> deliverable is *a few clean ablation tables on a named slice*, not a full matrix.

## Goal
Decide how to run R2 so it hits its exit — **reproduce a published BM25 baseline within a stated
tolerance on a named corpus slice, and select the top retrieval config(s) per a stated criterion** —
at the smallest scope, reusing the R1 scorer/corpus/trajectory machinery and the M10.2 protocol, and
leaving RQ4 / SWE-ContextBench / SWE-bench-Verified / A3 (embedding) / A5 (hybrid) deferred to R3+.

---

## 1. Smallest R2 that hits the exit

R2 is the **offline-ablation** milestone: it scores *retrieval configurations* against gold contexts
with **no agent and no LLM** (that is R3's agent-in-loop study). Per project_overview §5.3, the
substance is the **chunking × ranking × enrichment** cube measured at Layer-1. The smallest R2 keeps
the cube tiny but real, and pins it to a *published* number so "reproduce within tolerance" is checkable.

### The exit, made concrete (analogous to R1's single-task exit)

> **R2 is done when:** on a **named public corpus slice with published BM25 retrieval numbers**, the
> CodeCache retriever (driven offline through the harness, scored by the R1 Layer-1 scorer extended
> with **NDCG@10**) **reproduces the published BM25 baseline within a stated tolerance**, AND an
> ablation table over **{chunking} × {per-column BM25 weights} × {D3 enrichment ON/OFF}** is produced
> with a **stated top-config selection criterion**, naming the winning config(s). All offline,
> deterministic, reproducible from a clean checkout; **no LLM, no agent loop, no paid spend.**

Two parameters must be stated (proposed defaults, ratify/adjust at build):

- **Named corpus slice + published baseline.** **Primary target: the CodeRAG-Bench RepoEval
  (function-level) slice**, because §5.1 chose it precisely as the "sanity-check vs published
  BM25/dense numbers; comparability with cAST" benchmark and it *ships BM25 + dense baselines in-repo*
  (so the published number travels with the data). **Fallback if its license blocks vendoring:**
  ContextBench-Lite (Apache-2.0, parquet, gold pre-annotated — the cleanly-licensed option from the R1
  §4 research), reproducing *its own* reported BM25 line. The exit names whichever slice clears the
  license gate (§4 gate #1); the harness is corpus-agnostic so the choice is data, not code.
- **Tolerance + selection criterion (proposed).** *Reproduction tolerance:* the headline retrieval
  metric (NDCG@10, or Recall@k if the published table leads with it) lands within **± 0.03 absolute**
  (≈ a few points) of the published BM25 number — a deliberately loose band, because our tokenizer /
  chunk boundaries / FTS5 BM25 constants differ from the reference implementation and the goal is
  *"we're in the same ballpark, our harness isn't confounded,"* not bit-exactness. State the gap
  honestly either way (a documented out-of-band result is still a valid R2 outcome — it bounds the
  measurement, §7 null-result discipline). *Top-config criterion:* pick the config(s) whose Layer-1
  headline metric is **highest and separated from the next config by more than the run-to-run noise
  floor**; ties are reported as ties (no false winner). These promoted configs are the inputs R3 runs
  agent-in-loop — R2 *selects*, R3 *adjudicates*.

### The ablation cube — kept minimal (§7)

| Axis | R2 (smallest) | Deferred |
|---|---|---|
| **Chunking** | CodeCache's own function-level AST chunks (M4, baseline) **vs** cAST/astchunk split-merge (the published cAST baseline) | fixed-32-line windows (§5.3's third option) → only if the first two separate and time allows |
| **Ranking** | a **small per-column BM25 weight sweep** over the 7 indexed FTS5 columns (D3/D11: `symbol_name`, `parent_symbol`, `imports`, `cross_references`, `chunk_text`, `file_docstring`, …) — e.g. the current default vs 2–3 hand-reasoned alternatives (symbol-heavy, body-heavy) | a full grid / learned weights → R3+ |
| **Enrichment** | **D3 ON vs OFF** (the cleanest RQ2 probe — the index already carries the fields; OFF = exclude the enrichment columns from the MATCH/weighting) | per-field ablation → only if ON/OFF separates |

That is **2 chunkings × ~3 weight settings × 2 enrichment = ~12 offline scoring runs** on one slice —
small, deterministic, finishes in minutes, and directly answers RQ2 ("how much recall does zero-cost
enrichment buy?") at Layer-1.

### Explicitly CUT from R2 (§7 — "cut stretch first")

- **RQ4** (freshness/staleness penalty), **SWE-ContextBench** (context-reuse), **SWE-bench Verified**
  (downstream Pass@1) → **R3+** (the last is the R1 §4 finding's explicit "R3, not R2").
- **Arm A3** (embedding tool — needs an off-the-shelf code embedder + vector store; D1 defers
  embeddings) and **Arm A5** (hybrid RRF) → **R3** (these are the only configs needing a model/new dep;
  R2 is zero-model by construction).
- **Any agent-in-loop run / Layer-2 token economy / bootstrap CIs / the ~$1K spend** → **R3** (R2 is
  Layer-1 offline only).
- **line-level granularity** (ContextBench offers file/block/line) → R2 keeps **file + block** (the
  M10.2 protocol granularities the scorer already pins); adding NDCG@10 is the only metric expansion.

---

## 2. Slice decomposition — staged GATED vs UNGATED

The decisive scheduling insight: **the math and the harness scaffolding need no external data and no
human decision — they can start immediately**; only the *external corpus*, its *license*, the
*astchunk dependency*, and the *"which published number"* choice are gated. Start every ungated slice
first, against synthetic/in-tree gold, so R2 has a fully-tested, corpus-agnostic engine *before* any
gate is touched — then dropping in the real slice is a data swap, exactly as D21 promised.

### UNGATED — can start immediately on PROPOSED-D23 ratification (no external data, no new dep)

- **R2.1 — NDCG@10 scorer extension (pure math, TDD-able).** Add `dcg`/`ndcg_at_k` to
  `research/r1_harness/r1harness/scorer.py` beside the existing Recall/Precision/F1, with hand-computed
  unit tests in `tests/test_scorer.py` (the R1 pattern: tests mirror a known table). Graded relevance
  defaults to **binary** (gold vs not — what our gold schema carries today), with the ideal-DCG
  denominator computed from the gold-set size; document that graded relevance is a later refinement.
  Net-new but self-contained; no corpus, no network, no dep. **This is the first slice.**
- **R2.2 — per-column BM25 weight-sweep harness (over CodeCache's own retriever).** A thin driver that,
  for each weight setting, configures the retriever and scores it against an in-tree corpus. **Key
  finding from reading the code:** the BM25 per-column weights live in the **Rust crate** (the `bm25()`
  weight list, D11/D3 follow-up) — *not* in the Python harness — so the sweep is a **CodeCache-config /
  CLI-flag question, not a retriever-code change.** Smallest honest path: drive the sweep through the
  **existing `codecache query` process boundary** (R1's `codecache_tool.py`), varying weights via
  whatever surface the binary already exposes; **if no such surface exists, the sweep that needs a new
  CLI flag / config key to expose weights is itself a tiny GATED production slice** (a real `Cargo.toml`
  -free code change in the crate, manager-gated, test-first) — flagged honestly here as a possible fork,
  not assumed away. R2.2 scaffolding (the sweep loop + scoring + table emit) is ungated and built first
  against the default weights; wiring it to *vary* weights may surface the gated sub-slice.
- **R2.3 — chunker-swap seam (scaffolding only, against in-tree corpus).** Introduce a seam in the
  harness that scores **a corpus chunked two ways**: CodeCache's native chunks (today's path) and a
  pluggable "external chunker" slot. Build and test the seam with a **stub second chunker** (e.g. a
  trivial fixed-window splitter implemented in-harness, zero dep) so the A/B plumbing + table emit is
  proven *before* astchunk is introduced. Reuses `corpus.py`'s materialiser; net-new is the
  swap-point + the per-chunking scoring loop.
- **R2.4 — ablation-table reporter (extends R1 `report.py`).** A pure, deterministic emitter that takes
  the {chunking × weights × enrichment} scoring results and renders the comparison table(s) +
  machine-readable JSON, with the top-config selection criterion (§1) applied. Mini-free, corpus-free
  to test (synthetic inputs). Net-new but trivially TDD-able.

**Ungated order:** **R2.1 → R2.2 → R2.3 → R2.4.** After these four, R2 has a fully unit-tested,
deterministic ablation engine that runs end-to-end on the **in-tree micro-suite** (3 corpora × 5
queries) and emits a real (if small-N) ablation table — *with zero external data and zero new
dependency*. This is the de-risking milestone: prove the engine, then feed it the real slice.

### GATED — blocked on a specific human decision (do not start until the named gate clears)

- **R2.5 — vendor the external corpus slice (gated on §4 #1 license + §4 #2 network/HF).** Add a
  **corpus loader for the external slice** (CodeRAG-Bench RepoEval *or* ContextBench-Lite parquet) that
  maps the slice's gold annotations into our gold schema (`gold_files` + `gold_blocks={file_path,
  symbol_name}`) so the **scorer stays unchanged** (D21). Blocked because it requires (a) a confirmed
  license to vendor (R1 §4: CodeRAG-Bench license **UNVERIFIED**; ContextBench Apache-2.0 OK) and (b)
  permission to download benchmark data over the network/HF.
- **R2.6 — astchunk (cAST) baseline chunker (gated on §4 #3 dependency approval).** Replace R2.3's stub
  chunker with the real **astchunk** PyPI package (R1 §4: **MIT**, offline) as the published cAST
  baseline. Blocked on manager sign-off to add a **research-only Python dependency** (the precedent is
  the D17 "test/research-only, keeps `Cargo.toml` lean" basis — astchunk touches no crate, no release
  artifact — but it is still a new dep and gets an explicit gate).
- **R2.7 — baseline-reproduction script + the exit run (gated on R2.5, and on §4 #4 "which published
  number").** The script that runs the named slice through the retriever, computes NDCG@10/Recall, and
  asserts the published-baseline tolerance (§1), then runs the full ~12-cell ablation and promotes top
  configs. Blocked because it can't exist until the corpus (R2.5) and the target number (§4 #4) are
  fixed. **This is the slice that satisfies the R2 exit.**

**Why this staging matters (§7):** if any gate stalls (e.g. the human declines the network download, or
no benchmark license clears), **R2.1–R2.4 still ship value** — a tested NDCG@10 scorer + a working
ablation engine over the in-tree micro-suite, which is a legitimate (smaller) deliverable and keeps R2
from being all-or-nothing. The gated slices add *external comparability*, not *the apparatus*.

---

## 3. What R2 reuses from R1 vs newly builds

**Reuse (do NOT reinvent — these are R1's tested, committed surface):**
- **The Layer-1 scorer + protocol** — `research/r1_harness/r1harness/scorer.py` (Recall@k/Precision@k/
  F1@k at file+block, k∈{1,5,10}, macro-average, the `dedup_first` first-seen rule), pinned behaviorally
  to `tests/retrieval_quality.rs` (the M10.2 contract, D21). R2 **extends** it with NDCG@10; it does
  **not** rewrite the existing metrics.
- **The gold schema** — `gold_files` + `gold_blocks={file_path, symbol_name}` from
  `tests/fixtures/retrieval_quality/micro_suite.json` (the single source of truth, D21). The external
  corpus loader (R2.5) maps *into* this schema so the scorer is unchanged.
- **`corpus.py`** — the micro-suite → on-disk-repo materialiser (`load_corpus`/`materialize`). R2's
  chunker-swap seam and its in-tree ablation runs build directly on it.
- **`codecache_tool.py`** — the process-boundary adapter (shell out to the built binary, parse §6.4.2
  JSON, relativise paths to gold). The weight-sweep + reproduction runs drive the retriever through it
  (no FFI — research/CLAUDE.md process-boundary rule preserved).
- **`trajectory.py` / `report.py`** — the Layer-2 schema and the pure-scoring reporter. R2 is Layer-1
  only, so trajectory is largely dormant, but `report.py`'s pure-emit pattern is the template for the
  R2 ablation reporter (R2.4).
- **The M10.2 contract** — the Rust unit tests keep the metric *definitions* honest; the Python side
  ports, never diverges.

**Newly build (R2's net-new surface):**
- **NDCG@10** (graded-relevance Layer-1 metric — R2.1; the only metric expansion).
- **A corpus loader for the external slice** that maps published-benchmark gold → our gold schema
  (R2.5).
- **A chunker-swap seam** (R2.3) + the **astchunk baseline** behind it (R2.6).
- **The per-column BM25 weight-sweep driver** (R2.2) — and, *if weights aren't already exposable by the
  binary*, a small gated CLI/config surface in the crate to expose them.
- **The baseline-reproduction + ablation-table runner** (R2.4 reporter + R2.7 exit script).

---

## 4. Human gates (numbered — needing main-session → user ratification)

Flagged honestly; **unverified items are called out, not asserted** (mirrors R1 §4 discipline).

1. **License verification — CodeRAG-Bench / RepoEval slice.** R1 §4 recorded the CodeRAG-Bench license
   as **UNVERIFIED** and RepoEval/RepoCoder (`microsoft/CodeT/RepoCoder`) as **MIT** but with a caveat
   that the function-level slice "reportedly leans on a private lib — verify." **Confirm the license
   before vendoring either** into the artifact. If neither clears, **fall back to ContextBench-Lite
   (Apache-2.0** — the one license R1 confirmed as clean, though its "offline gold contexts" reading is
   itself a grounded inference to re-confirm against the repo README). *Gate blocks R2.5/R2.7.*
2. **Network / HuggingFace download permission for benchmark data.** The product is **air-gapped by
   design** (D12), but the **research harness downloading a benchmark corpus is a separate question** —
   flag it explicitly: R2 needs a one-time network/HF fetch of the chosen slice's parquet/zip. This is
   *research-harness* network access (out-of-crate, ships in no release), not a change to the product's
   offline guarantee — but it is the user's call to authorize the download. *Gate blocks R2.5/R2.7.*
3. **The astchunk (cAST) research dependency.** Adding `astchunk` (PyPI, **MIT**, offline per R1 §4) as
   a **research-only Python dep**. It touches no `Cargo.toml`, no crate, no release artifact (D17
   "test/research-only, keep the runtime lean" precedent extended to research) — but a new dependency
   still gets explicit manager/human sign-off. *Gate blocks R2.6 (R2.3's stub chunker stands in until
   cleared).*
4. **The scope-cut confirmation + "which published baseline."** Ratify (a) that R2 **cuts** RQ4 /
   SWE-ContextBench / SWE-bench-Verified / A3 / A5 / Layer-2 / the $1K spend to R3+ (§1), and (b) the
   **named corpus slice + the specific published BM25 number** R2 reproduces and the **tolerance**
   (§1 proposes CodeRAG-Bench RepoEval, ± 0.03 absolute, top-config = highest-and-separated). *Gate
   shapes the exit; R2.7 can't finalize without it.*

**Possible fifth gate, surfaced honestly (not yet a decision):** if the per-column BM25 weights are
**not** already variable through the built binary's CLI/config, the weight sweep (R2.2) needs a **tiny
production change in the Rust crate** to expose them (a CLI flag / config key, test-first, `Cargo.toml`
untouched). That is a real crate change and would route through the normal TDD team + manager gate —
flagged here so it isn't discovered mid-build. *Affects how far R2.2 can go ungated.*

**NOT in R2 (stay R3 gates — restated for emphasis):** the **~$1K R3 API spend**, any **paid API /
paid benchmark access**, the R3 model choice, and any agent-in-loop run. R2 is zero-spend offline
Layer-1 scoring.

---

## 5. Ownership recommendation — introduce `research-harness-engineer` NOW

**Recommendation: introduce a dedicated `research-harness-engineer` agent for R2 (and forward to R3),
rather than continuing main-session-drives as R1 did.** This is the firm call the R1 brief deferred:
R1 §5 said main-session-drives R1 and to add the agent **"if R1 grows (R2/R3 corpus + matrix)"** — and
**R2 *is* that growth point** (D22 named exactly this trigger). Reasons, priority order:

1. **R2 crosses the size/complexity threshold R1 explicitly set.** R1 was thin single-author glue (fork
   mini, add a logger + scorer, run one task). R2 is a **multi-slice ablation program** — NDCG math, a
   sweep harness, a chunker seam, an external-corpus loader, a reproduction script, and ~12 scored
   configurations — with its own TDD inner loop. That is precisely the "if it grows, give it an owner"
   condition.
2. **R2 has a *real, distinct* quality-gate stack the Rust agents don't fit.** The Rust team gates on
   `fmt`/`clippy -D warnings`/`cargo test` (research/CLAUDE.md: those four gates **do not apply** here).
   R2's gates are **`ruff` + `pytest`** over `research/`. A dedicated agent makes those gates
   first-class and routable, instead of ad-hoc in the main session.
3. **R3 is coming and is heavier** (full A0–A5 matrix, 30–100 tasks, the $1K spend, CIs). Standing the
   agent up at R2 means R3 inherits an established Python-aware owner instead of re-deriving one under
   cost pressure. The R1 brief already pre-authorized this exact agent by name as the R2/R3 growth path.
4. **The manager stays gatekeeper** for scope/DoD/doc-sync and owns the PROPOSED D23 + ROADMAP/TODO
   updates — unchanged. The new agent *executes within* R2 slices; it does not own the plan.

**Sketch of the agent definition** (manager to scaffold a `.claude/agents/research-harness-engineer.md`
at build, only after D23 ratification — do not create it in this spike):
- **Model:** sonnet (Python glue + ablation runs; opus reserved for the Rust specialists). 
- **Scope:** `research/` **only** — never the Rust crate, never `Cargo.toml`, never a release artifact.
- **Quality gates:** `ruff` (lint/format) + `pytest` (tests-first, mirroring the crate's TDD discipline:
  failing test → green → refactor), run over `research/`. Honors the research/CLAUDE.md rules
  (out-of-crate, process boundary to the binary, **one gold source**, **no paid spend without a gate**).
- **Tools:** Read/Grep/Glob/Edit/Write/Bash (Python execution + pytest/ruff). No `Agent` tool (single
  orchestrator stays the manager).
- **Description (trigger-rich):** "Python research-harness engineer for CodeCache's R2–R4 ablation/eval
  track. Use for NDCG/Layer-1 scorer work, BM25 weight sweeps, chunker ablations, corpus loaders,
  baseline reproduction — all under `research/`, gated by ruff + pytest, process-boundary to the built
  binary. Never touches the Rust crate or `Cargo.toml`."
- **Doc consequence:** adding it updates `.claude/CLAUDE.md` (the agent table) + `research/CLAUDE.md`
  (ownership line currently says "main session for R1; dedicated agent if R2/R3 grow") + ENGINEERING_PLAN
  §5 if gate behavior is referenced — manager does this in the same change.

*(If the user prefers to keep main-session-drives for R2's ungated slices and only stand the agent up at
R3, that is a defensible lighter option — but my firm recommendation is to introduce it at R2, because
R2 is the named trigger and R3 will be cheaper to run with the owner already in place.)*

---

## 6. Proposed Decision Log entry + TODO expansion (for the main session to apply)

### Proposed `docs/ROADMAP.md` Decision Log entry — **D23 (PROPOSED — pending human ratification)**

> Append after D22. Held open exactly like D15/D22 were before ratification (spike → human ratify →
> build). Do **not** mark Adopted until the human ratifies.

```
### D23 — R2 offline ablations: smallest exit = reproduce one published BM25 baseline + pick top configs; introduce `research-harness-engineer`  · **PROPOSED — pending human ratification** (plan: research track R2) — *overview §5–§7; spike: `.claude/briefs/BRIEF-R2-offline-ablations.md`*

> **Spike → human ratify → build (the D15/D22 pattern).** The R2-entry spike is complete; the
> disposition below is PROPOSED and **not yet ratified**. Ungated slices (NDCG@10 scorer, sweep/chunker
> scaffolding over the in-tree micro-suite) may start on ratification; the corpus/license/astchunk/CLI
> slices are individually gated (below). The ~$1K R3 API spend and any paid benchmark/API access remain
> separate downstream R3 gates — **not** authorized here (R2 is zero-spend offline Layer-1 scoring).

The research track (overview §5.3) needs the chunking × ranking × enrichment ablation R2 owns. R2's exit
is narrow and offline: **reproduce a published BM25 baseline within tolerance on a named corpus slice,
and select the top retrieval config(s)** — no agent, no LLM, no Layer-2.

**Smallest R2 + exit.** Run the CodeCache retriever offline through the (R1) harness, scored by the R1
Layer-1 scorer extended with **NDCG@10** (the CodeRAG-Bench protocol metric R2 adds), over a **~12-cell
ablation**: {CodeCache AST chunks vs cAST/astchunk} × {~3 per-column BM25 weight settings} × {D3
enrichment ON/OFF}. **Exit:** on a named public slice with a published BM25 number, the retriever
reproduces that number within a stated tolerance (proposed: **CodeRAG-Bench RepoEval function-level
slice; ± 0.03 absolute** on the headline metric; fallback **ContextBench-Lite**, Apache-2.0), AND the
ablation table is emitted with a stated top-config criterion (proposed: **highest-and-separated beyond
the noise floor**; ties reported as ties) naming the promoted config(s) — which are R3's agent-in-loop
inputs. Deterministic, reproducible from a clean checkout, **no LLM/agent/paid spend.**

**Reuse vs new.** Reuse the R1 scorer/protocol (Recall/Precision/F1 @k file+block — M10.2 contract,
D21), the gold schema, `corpus.py`, `codecache_tool.py` (process boundary), `report.py`'s pure-emit
pattern. New: **NDCG@10**; an **external-corpus loader** mapping published gold → our gold schema (so
the scorer is unchanged, honoring D21); a **chunker-swap seam** + the **astchunk** baseline; a
**per-column BM25 weight-sweep driver**; the **baseline-reproduction + ablation runner**.

**Staging — ungated first.** UNGATED (start on ratification, no external data/dep, TDD vs in-tree gold):
(R2.1) NDCG@10 scorer extension; (R2.2) BM25 weight-sweep scaffolding over CodeCache's own retriever;
(R2.3) chunker-swap seam with a stub chunker; (R2.4) ablation-table reporter. GATED (each on a specific
human decision): (R2.5) vendor the external slice — license + network/HF; (R2.6) the astchunk dep;
(R2.7) the baseline-reproduction exit run — depends on R2.5 + the chosen published number. If a gate
stalls, R2.1–R2.4 still ship the tested apparatus over the micro-suite (R2 is not all-or-nothing).

**Note (BM25 weights live in the crate).** The per-column `bm25()` weights are in the **Rust crate**
(D11/D3), not the Python harness; the sweep drives the binary via the process boundary. If the weights
are not already varied through the binary's CLI/config, exposing them is a **small test-first crate
change** (no `Cargo.toml` dep change) routed through the normal team + manager gate — flagged so it
isn't a build-time surprise.

**Cuts (§7, to R3+).** RQ4 (freshness), SWE-ContextBench, SWE-bench Verified, arm A3 (embedding tool —
D1 defers embeddings), arm A5 (hybrid RRF), all Layer-2 token economy + bootstrap CIs + the ~$1K spend,
and line-level granularity. R2 keeps file+block granularity + adds only NDCG@10.

**Ownership — introduce `research-harness-engineer` now.** R1 (D22) had the main session drive and
pre-authorized this agent "if R2/R3 grow"; **R2 is that growth point.** Stand up a dedicated
**`research-harness-engineer`** (model: sonnet; scope: `research/` only; gates: **ruff + pytest**;
process-boundary to the binary; honors research/CLAUDE.md — never touches the crate/`Cargo.toml`). The
manager stays gatekeeper for scope/DoD/doc-sync. (Lighter alternative: defer the agent to R3 — but R2 is
the named trigger and R3 is cheaper with the owner already in place.)

**Human gates (NOT in this proposal):** ratify this D23; verify the CodeRAG-Bench/RepoEval **license**
(fall back to ContextBench-Lite Apache-2.0 if unclear); authorize the **research-harness network/HF
download** of benchmark data (separate from the product's air-gapped guarantee); approve the **astchunk**
research dep; confirm the **scope cuts + the named published baseline/tolerance**. The **~$1K R3 spend**
and any paid access stay R3 gates. Owner: manager (proposal + spike) → research-harness-engineer (R2
build, on ratification).
```

### Proposed `docs/TODO.md` "Research track" — R2 expansion text

> Replace the current single R2 line with the expansion below (mirrors the R1 nested-checklist style).

```
- [ ] **R2 offline ablations** (PROPOSED D23 — pending human ratification; on ratify → research-harness-engineer):
      chunking × ranking × enrichment, Layer-1 only, zero LLM/agent/paid spend. Exit = reproduce a
      published BM25 baseline within tolerance on a named slice + pick top configs. Brief:
      `.claude/briefs/BRIEF-R2-offline-ablations.md`.
      - [ ] **Ownership/agent:** stand up `.claude/agents/research-harness-engineer.md` (sonnet; scope
            `research/`; gates ruff + pytest; process-boundary to the binary) + update `.claude/CLAUDE.md`
            agent table + `research/CLAUDE.md` ownership line. → manager
      - [ ] **R2.1 (UNGATED) NDCG@10 scorer extension** — add `ndcg_at_k` to `r1harness/scorer.py` +
            hand-computed `tests/test_scorer.py` cases (binary relevance; ideal-DCG from gold size).
            No corpus, no dep. → research-harness-engineer
      - [ ] **R2.2 (UNGATED) BM25 weight-sweep scaffolding** over CodeCache's retriever via the
            `codecache_tool.py` process boundary; default weights first. (Weights live in the crate —
            if not CLI/config-exposable, a tiny test-first crate flag is a gated sub-slice.) → research-harness-engineer (+ eng-lead if the crate flag is needed)
      - [ ] **R2.3 (UNGATED) chunker-swap seam** with an in-harness stub chunker (proves the A/B
            plumbing before astchunk). Reuses `corpus.py`. → research-harness-engineer
      - [ ] **R2.4 (UNGATED) ablation-table reporter** — pure deterministic emit of {chunking × weights ×
            enrichment} results + top-config selection (extends `report.py`'s pattern). → research-harness-engineer
      - [ ] **R2.5 (GATED: license #1 + network/HF #2) external-corpus loader** — map CodeRAG-Bench
            RepoEval (or ContextBench-Lite) gold → our `gold_files`/`gold_blocks` schema (scorer
            unchanged, D21). → research-harness-engineer
      - [ ] **R2.6 (GATED: astchunk dep #3) cAST baseline chunker** — replace the R2.3 stub with the
            astchunk PyPI package (MIT, research-only). → research-harness-engineer
      - [ ] **R2.7 (GATED: R2.5 + named-baseline #4) baseline-reproduction + exit run** — reproduce the
            published BM25 number within tolerance + run the full ~12-cell ablation + promote top
            configs (R3 inputs). Satisfies the R2 exit. → research-harness-engineer + manager (exit verify)
```

---

## Downstream HUMAN GATES (out of scope for this spike — numbered list)
1. **Ratify PROPOSED D23** (this disposition) — mirrors the D15/D22 spike→ratify protocol. Until
   ratified, no R2 slice starts.
2. **CodeRAG-Bench / RepoEval license** — verify before vendoring; fall back to ContextBench-Lite
   (Apache-2.0) if unclear (gate #1).
3. **Research-harness network/HF download** of the benchmark slice — authorize the one-time fetch;
   distinct from the product's air-gapped guarantee (gate #2).
4. **astchunk research dependency** — approve the research-only Python dep (gate #3).
5. **Scope cuts + named published baseline/tolerance** — confirm the §1 cuts and the exit target (gate
   #4).
6. **(Conditional) tiny crate flag to expose BM25 weights** — if not already exposable, routes through
   the normal TDD team + manager gate (possible fifth gate, surfaced honestly).
7. **NOT authorized here:** the **~$1K R3 API spend**, any **paid API/benchmark access**, the R3 model
   choice, any **agent-in-loop run** — all stay **R3** gates.

## Deliverable paths (for the main session to commit; manager has no Bash/git)
- This brief: `C:\Users\ehlee\workspace\projects\CodeCache\.claude\briefs\BRIEF-R2-offline-ablations.md`
- PROPOSED D23 entry appended to: `C:\Users\ehlee\workspace\projects\CodeCache\docs\ROADMAP.md` (marked **PROPOSED — pending human ratification**, held open like D15/D22 were)
- R2 expansion applied to: `C:\Users\ehlee\workspace\projects\CodeCache\docs\TODO.md` ("Research track" section)

---
## OUTCOME — manager
Spike/plan complete in one synchronous pass (no background agents — `SendMessage`-resume does not exist
in this harness, and R1 already proved background agents orphan; foreground file reads only). **Smallest
R2 = reproduce one published BM25 baseline within ± 0.03 on a named slice (CodeRAG-Bench RepoEval, or
ContextBench-Lite fallback) + a ~12-cell {chunking × BM25-weights × D3-enrichment} Layer-1 ablation with
a stated top-config criterion — zero LLM/agent/paid spend.** Staged **UNGATED first** (R2.1 NDCG@10
scorer → R2.2 weight-sweep scaffolding → R2.3 chunker seam → R2.4 reporter, all against the in-tree
micro-suite) then **GATED** (R2.5 corpus/license/network → R2.6 astchunk dep → R2.7 exit run). **Reuses**
the R1 scorer/protocol/gold-schema + `corpus.py` + `codecache_tool.py` + `report.py` pattern (M10.2
contract, D21); **newly builds** NDCG@10, an external-corpus loader, a chunker-swap seam, the BM25
weight-sweep driver, and the reproduction runner. **Firm ownership call: introduce
`research-harness-engineer` now** (sonnet; `research/` scope; ruff + pytest; process boundary) — R2 is
the D22-named growth trigger. Five+ human gates listed (D23 ratify; CodeRAG-Bench license w/
ContextBench-Lite fallback; research-harness network/HF download; astchunk dep; scope-cut + named
baseline; conditional crate-flag for BM25 weights). **No code/deps/`Cargo.toml`/data/commits touched.**
Awaiting human ratification of **PROPOSED D23** before any build. Task #19 marked completed.
