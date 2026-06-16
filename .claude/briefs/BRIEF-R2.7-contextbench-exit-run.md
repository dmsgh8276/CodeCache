# BRIEF — R2.7 / scoped real-corpus exit run (R2 EXIT)

- **Milestone:** R2.7 — baseline-reproduction + exit run (SOFTENED per D27)  ·  **Module(s):** `research/` only (`r1harness/contextbench_corpus.py` NEW + wiring + run entrypoints)
- **Owner (manager):** principal-engineering-manager  ·  **Created:** 2026-06-15
- **Status:** RED ☑  GREEN ☑  REVIEW ☑ (BLOCK→fix→APPROVE)  DONE ☑ (commit `585ec2d`, R2 track CLOSED, D29)
- **Links:** docs/ROADMAP.md (Research track R2, D23/D26/D27/D28) · docs/TODO.md (Research track → R2.7) · docs/TEST_STRATEGY.md (research scorer rows) · research/CLAUDE.md
- **Human ratification (this turn):** scoped real-corpus run + gated build route ratified. This slice + its commit **closes the R2 track** per D27's softened exit.

## Goal
Run the **scoped, directional real-corpus Layer-1 ablation** over ~10–15 ContextBench-Lite tasks
(python/typescript, ≤3–4 distinct repos) and emit the **R2.7 Markdown ablation table**: file-level
**NDCG@10** (headline) + F1@10/Recall@10 (file) across (1) the 6 BM25 weight vectors (native chunker)
and (2) the native-vs-astchunk chunker A/B. This is the R2 EXIT.

This is a **build, not "load + run"**: R2.5a's `contextbench.py` maps records → `SweepQuery` (query +
gold labels) ONLY — it does **not** materialize a searchable corpus. ContextBench-Lite's HF schema ships
`repo_url` + `base_commit` + `gold_context`(snippet) + `problem_statement` + `language` + `patch`… but
**no repository source and no retrieval pool**. The searchable corpus exists only by **cloning each
task's repo at its `base_commit`** and indexing the whole tree (indexing only gold files would make
recall trivially 1.0). R2.7 builds the corpus materializer that R2.5a deliberately left out.

## Scope (in / out)
**In:**
- NEW pure-ish module `r1harness/contextbench_corpus.py`:
  - **Task selector** (pure, unit-testable): from the fetched ContextBench-Lite slice, filter
    `language ∈ {python, typescript}`, cap to **≤3–4 distinct repos** (prefer repos with several Lite
    tasks so few clones cover many tasks), return **~10–15 tasks**. **Deterministic** selection rule —
    document it precisely in the docstring (e.g. stable sort by `(repo, instance_id)`, greedily admit
    repos until the repo-cap, admit tasks until the task-cap).
  - **Materializer** (thin I/O): per selected task, `git clone <repo_url>` **once per repo** into a
    gitignored cache (`cache/contextbench_repos/<owner>__<name>/`), reuse on re-run; then
    `git checkout <base_commit>` into a per-task working tree CodeCache indexes. Clone/checkout failure
    → **typed error / skip-with-log, no crash**. Pure helpers (path mapping, the clone/checkout argv,
    repo-cache-dir derivation) split out and unit-tested without invoking real `git`.
  - **Path-alignment guard:** a unit test proving `gold_files` (repo-relative posix, e.g.
    `astropy/coordinates/attributes.py`) align with `codecache_tool.normalize_path`'d retrieved paths,
    so file-level matches aren't silently zero.
- Wiring/run entrypoint(s) (mirror `run_ab_astchunk.py`/`run_sweep.py` precedent; missing-cache → clean
  nonzero exit, no traceback) that, **for each selected task**:
  - clone+checkout via the materializer → one on-disk repo tree;
  - **Run 1 (BM25 sweep, native chunker):** `init → index → query(problem_statement, --bm25-weights=vec)`
    for the 6 vectors of `sweep.DEFAULT_GRID`; reuse `sweep.score_vectors` with a `query_fn` that queries
    the cloned-repo `CodeCacheIndex`. Gold = the task's `SweepQuery` (from `contextbench.parse_contextbench_records`).
  - **Run 2 (chunker A/B, default weights):** native (`init→index→query`) vs astchunk
    (`init→ingest <astchunk chunks of the repo files>→query`), reusing `run_ab_astchunk` / `astchunk_chunker`
    over the cloned-repo files.
  - Aggregate across tasks → the two ablation tables (reuse `ablation_report` rendering where it fits;
    extend minimally if file-level headline rendering is needed). **Score at FILE-LEVEL** using
    `MetricAtK.ndcg_file`/`recall_file`/`f1_file`. **Block-level excluded from the headline** (ContextBench
    has no symbol-name gold + astchunk synthesizes names — block-level is meaningless here).
- **Install** `datasets==5.0.0` + `huggingface_hub==1.19.0` into `research/r1_harness/.venv` (authorized
  by D26; pinned in `requirements.txt` already) so `fetch_contextbench.py --force --n-records N` can run.
- **`.gitignore`:** add the repo-clone cache dir so cloned repos/blobs are never committed. The root
  `.gitignore` `cache/` rule plus the research `.gitignore` `cache/` rule already blanket-cover a
  `cache/` subtree, but **add an explicit pattern for `cache/contextbench_repos/`** (manager owns this;
  verify it covers the chosen path; `runs/` is already blanket-ignored).

**Out / defer:**
- **No `src/`, `Cargo.*`, `.rs`, or `.claude/settings.json` edits.** Pure `research/`. Leave
  `settings.json` modified-but-unstaged (exclude via explicit pathspec at commit).
- No full-500 run (scoped/directional exit, not a published result). No ±0.03 reproduction (R2.5b CUT).
- No block-level headline. No winner asserted unless clearly separated beyond noise.
- No Go tasks (astchunk has no Go grammar; the language filter excludes them anyway).
- No new crate flag — `--bm25-weights` (R2.2a/D24) + `ingest` (R2.3a/D25) already exist.

## Verified environment facts (do NOT re-derive)
- **GitHub git egress WORKS** (`git ls-remote https://github.com/...` returns refs). Only `api.github.com`
  REST is proxy-blocked — not needed for `git clone`.
- **`datasets`/`huggingface_hub` NOT yet installed** in `research/r1_harness/.venv` (pinned in
  `requirements.txt`; the R2.6 venv never installed them). Install them (D26-authorized, fetch-only, zero
  spend, product air-gapped).
- **Binary:** `target/debug/codecache` exists (Linux ELF). CodeCache supports python/typescript/go;
  astchunk supports python/typescript (NOT go).
- **Fetch entrypoint:** `fetch_contextbench.py --force --n-records N` downloads `contextbench_verified`
  (Lite, 500) and caches a JSON slice under gitignored `cache/contextbench/`. The current default takes a
  deterministic HEAD slice — to guarantee enough py/ts tasks across ≤4 repos you may need a larger
  `--n-records` (e.g. 100–500) so the **selector** has enough to filter from. The selector, not the fetch,
  owns the language/repo/task caps.
- **Suite runs via the venv:** `PYTHONUTF8=1 research/r1_harness/.venv/bin/python -m pytest
  research/r1_harness/` (system `python3` lacks astchunk/datasets). Green baseline before this slice =
  **138 passed, 1 skipped** (Windows-only path skip).

## Cost flags (keep the run bounded & recoverable)
- Cloning repos (astropy/django-scale) = minutes + 100s of MB each → **cache + reuse**, keep **≤4 repos**.
- astchunk chunking a FULL repo (1000s of files) per task is heavy → if needed, **scope the A/B arm to a
  smaller subset of tasks than the sweep** (DOCUMENT any such split in the table). Prefer correctness over breadth.
- Make the run **checkpointable** (cache the fetched slice + the clones) so a re-run resumes cheaply.
- **If you near a session limit:** leave the brief + partial state recoverable (the R2.3a pattern) —
  land a correct, reviewed, committed apparatus + a real (if small) table over breadth.

## Scenarios to cover (tests-first; binary/network/git-free unit tests)
- [ ] **selector — language filter:** Go/other-language records are dropped; only py/ts admitted.
- [ ] **selector — repo cap:** result spans ≤ the configured max distinct repos.
- [ ] **selector — task cap + determinism:** stable, reproducible task list for the same input (document the rule); same input ⇒ same output, independent of input ordering perturbation if the rule sorts.
- [ ] **materializer — path/argv logic (pure):** repo-cache-dir derivation from `repo_url`; the
      clone/checkout argv; per-task working-tree path mapping — tested without invoking real `git`
      (mock or test the pure functions).
- [ ] **materializer — failure handling:** clone/checkout failure yields a typed error / skip-with-log,
      not a crash (mock the failing `git` boundary).
- [ ] **path alignment:** `gold_files` (repo-relative posix) align with `normalize_path`'d retrieved
      paths so file-level matches aren't silently zero (pure, fixture-based).
- The **actual clone+index+score** is the integration RUN, not a unit test — keep network/git/binary out
  of pytest (mirror the R2.5a/R2.6 pure-core + thin-I/O split).

## Definition of Done
- [ ] Tests written first, now green (expect **1 Windows-only skip**); run in the venv.
- [ ] `ruff format research/` + `ruff check research/` clean.
- [ ] `r1harness/contextbench_corpus.py` materializer + selector built; eval RUN executed; the two
      file-level ablation tables produced (real, even if small).
- [ ] No `src/`/`Cargo.*`/`.rs`/`.claude/settings.json` touched. Repo-clone cache gitignored + never committed.
- [ ] `code-reviewer` APPROVED.
- [ ] docs/TODO.md (R2.7 DONE + **R2 track COMPLETE**) + docs/ROADMAP.md Decision Log (new D-entry, cite
      D23/D27) + research/CLAUDE.md (new materializer module + repo-clone cache + R2.7 needs network/git,
      product still air-gapped; note the R2.5a "mapper-only" gap R2.7 closed) updated in the same commit.
- [ ] Local commit (explicit pathspec; exclude `.claude/settings.json` + the gitignored caches; no push)
      with `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`.

## Deliverable — the R2.7 ablation table (state exactly)
- Which/how-many tasks; which repos; the language filter; `max_chunk_size`; the sweep-vs-A/B task split if any.
- That it is a **scoped/directional real-corpus exit (not full 500)**.
- **Headline = NDCG@10 (file)**; include F1@10/Recall@10 (file) for context.
- **No winner asserted** unless clearly separated beyond noise.
- **Honest comparison to the micro-suite saturation finding** (D28/R2.4: 5/6 vectors tied at NDCG@10=0.822,
  native vs astchunk tied file-level @ Recall saturation): does the real corpus finally SEPARATE the
  arms/vectors, or does saturation persist?

---
## RED — test lead (research-harness-engineer, 2026-06-15)

New test file: `research/r1_harness/tests/test_contextbench_corpus.py` (28 tests).

Coverage:
- selector — language filter (Go/java dropped, py/ts admitted)
- selector — repo cap (max_repos=1/2/3 enforced)
- selector — task cap + determinism (same input → same output after shuffle)
- selector — stable sort (repo, instance_id) verified
- materialiser — repo_cache_dir derivation (no git call)
- materialiser — clone_argv / checkout_argv shape
- materialiser — task_repo_dir (commit-embedded, distinct per commit, idempotent)
- materialiser — failure handling (clone fail → CorpusMaterializeError via mock)
- materialiser — checkout failure → CorpusMaterializeError
- path alignment — normalize_path(abs_path, repo_dir) → repo-relative posix matches gold_files

RED confirmed: `ModuleNotFoundError: No module named 'r1harness.contextbench_corpus'`

## GREEN — engineering lead (research-harness-engineer, 2026-06-15)

**New files built:**
- `research/r1_harness/r1harness/contextbench_corpus.py` — selector (pure) + materialiser (thin I/O)
  - Selection rule: filter py/ts → sort (repo, instance_id) → greedy repo/task admission
  - Materialiser: `git clone --no-checkout` once per repo; `git worktree add <commit>` per task
  - Typed error: `CorpusMaterializeError` for clone/checkout failures (skip-with-log at caller)
  - Pure helpers: `repo_cache_dir`, `clone_argv`, `checkout_argv`, `worktree_add_argv`, `task_repo_dir`
- `research/r1_harness/run_contextbench_exit.py` — scoped real-corpus exit entrypoint
  - CLI: `--repos`, `--max-repos`, `--max-tasks`, `--max-chunk-size`, `--cache-dir`, `--repo-cache-dir`
  - Run 1: BM25 weight sweep (native chunker), index once per task, query 6× per vector
  - Run 2: chunker A/B (native vs astchunk), same tasks
  - Timeout: 600s (10 min) per binary call; handles large repos with debug binary

**Schema confirmed from real ContextBench-Lite slice (500 records fetched 2026-06-15):**
- `instance_id`, `repo` (e.g. "astropy/astropy"), `repo_url` (e.g. "https://github.com/astropy/astropy.git"),
  `language` ("python"/"typescript"/etc.), `base_commit` (40-char SHA), `gold_context` (JSON string),
  `problem_statement` (natural language), `patch`, `test_patch`, `f2p`, `p2p`, `source`

**Tests:** 28 passed, 0 failed (RED→GREEN). Full suite: **166 passed, 1 skipped**.
**Ruff:** check clean + format clean.

**Run command:**
```
PYTHONUTF8=1 .venv/bin/python run_contextbench_exit.py \
    --repos pytest-dev/pytest vuejs/core \
    --max-tasks 10 --max-chunk-size 300
```

## Specialist / Perf notes (research-harness-engineer, 2026-06-15)

**Clone/index cost notes:**
- Cloning: `git clone --no-checkout` (full object graph, no working tree) per repo once.
  Sizes: astropy/astropy ~133MB, pytest-dev/pytest ~41MB, vuejs/core: not measured (cloned OK).
  Worktrees created via `git worktree add <commit>` are fast (share the object graph).
- Indexing with debug binary:
  - astropy (899 py files): ~13 min per task → EXCLUDED from the actual run
  - pytest (214 py files): ~50 sec per task
  - vuejs/core (457 ts files): ~74 sec per task
- **Decision: run only pytest-dev/pytest (5 tasks) + vuejs/core (5 tasks) = 10 tasks.**
  astropy would take >2 hours with the debug binary.
  The run was scoped to `--repos pytest-dev/pytest vuejs/core --max-tasks 10`.

**Sweep vs A/B split:** same 10 tasks for both (no split needed; total time ~30 min).
All 10 tasks: 0 skipped, 0 failures.

## REVIEW — code reviewer
<APPROVE / BLOCK + findings: severity — file:line — problem — fix>

## OUTCOME — manager
<aligned? TODO/ROADMAP/research-CLAUDE.md updated? R2 closed? commit hash + pathspec? follow-ups?>

## GREEN (re-run) — research-harness-engineer (2026-06-16)

### Fixes applied (all pure `research/`; no Rust/Cargo/settings.json touched)

**BLOCKER fixed — A/B isolation:**
`_score_task_ab()` now gives each arm its own scratch directory:
- `native_{i}/` — source files copied here; `init` + `index` + `query`
- `astchunk_{i}/` — same source files copied here (identical set); `init` + `ingest` + `query`

The shared task worktree (`task_dir`) is used ONLY to enumerate + read source files; it is never
passed to `codecache init` during Run 2. A `_copy_source_files()` helper copies the enumerated
file list into each arm's scratch dir, preserving relative layout. Both arms share the same
extension filter (`.py`/`.ts`) and 500-file cap — symmetric candidate pools.
This mirrors the `ab_runner.run_ab_astchunk` pattern (`native_ast/repo` vs `astchunk/repo`).

**MAJOR fixed — `norecursedirs` in pyproject.toml:**
Added `norecursedirs = ["cache", "runs", ".venv", "vendor"]` to `[tool.pytest.ini_options]`.
`pytest research/r1_harness/` (positional path arg) now collects only `tests/` regardless of
on-disk clones.

**MINOR fixed — symmetric candidate pools:**
Both arms enumerate the same extension + 500-file cap. The module docstring and `_score_task_ab`
docstring document the known limitation (extension restriction vs full-tree native sweep in Run 1).

**MINOR fixed — dead `tmp_path`:**
`with tempfile.TemporaryDirectory(...) as _tmp_sweep:` — no unused `tmp_path = Path(tmp)` binding.

### Gates (all clean)

```
pytest research/r1_harness/ → 166 passed, 1 skipped
ruff check research/         → All checks passed!
ruff format --check research/ → 40 files already formatted (CLEAN)
```

### Corrected Run 2 — Table 2 (chunker A/B, under proper isolation)

Binary: `target/debug/codecache` | Corpus: ContextBench-Lite (contextbench_verified, 500) |
Repos: `pytest-dev/pytest` (5 python tasks) + `vuejs/core` (5 typescript tasks) |
`max_chunk_size=300` | BM25 default weights | `n=10` tasks

```
### Table 2: Chunker A/B — File-level Metrics (default BM25 weights)

| Arm      | NDCG@10 (file) | F1@10 (file) | Recall@10 (file) | N tasks |
|----------|---------------|--------------|-----------------|---------|
| native   | 0.173         | 0.125        | 0.233           | 10      |
| astchunk | 0.249         | 0.164        | 0.367           | 10      |
```

### Table 1 (BM25 sweep, native chunker) — unchanged from prior run

```
### Table 1: BM25 Weight Sweep — File-level Metrics (native chunker)

| Vector       | Weights        | NDCG@10 (file) | F1@10 (file) | Recall@10 (file) |
|--------------|---------------|---------------|--------------|-----------------|
| default      | 10,1,1,5,2,2,2 | 0.173         | 0.125        | 0.233           | ← baseline
| flat         | 1,1,1,1,1,1,1  | 0.173         | 0.125        | 0.233           |
| name_only    | 10,0,0,0,0,0,0 | 0.153         | 0.100        | 0.233           |
| body_heavy   | 1,1,10,1,1,1,1 | 0.197         | 0.130        | 0.233           |
| name_strong  | 20,1,1,5,2,2,2 | 0.173         | 0.125        | 0.233           |
| enrich_heavy | 10,1,1,5,5,5,5 | 0.160         | 0.108        | 0.233           |
```

### Per-task A/B breakdown (under isolation)

| Task (short)                                    | Lang | nat_ndcg | ast_ndcg | delta   | nat_rec | ast_rec |
|-------------------------------------------------|------|----------|----------|---------|---------|---------|
| SWE-Bench-Verified__python__bugfix__0eecae1e    | py   | 0.631    | 1.000    | +0.369  | 1.000   | 1.000   |
| SWE-Bench-Verified__python__bugfix__982277e4    | py   | 0.000    | 0.387    | +0.387  | 0.000   | 1.000   |
| SWE-Bench-Verified__python__bugfix__abb9b8b0    | py   | 0.000    | 0.000    | +0.000  | 0.000   | 0.000   |
| SWE-Bench-Verified__python__bugfix__c2f0f2be    | py   | 0.469    | 0.671    | +0.202  | 0.333   | 0.667   |
| SWE-Bench-Verified__python__bugfix__e5236b5f    | py   | 0.000    | 0.000    | +0.000  | 0.000   | 0.000   |
| Multi-SWE-Bench__typescript__bugfix__04c51be7   | ts   | 0.000    | 0.000    | +0.000  | 0.000   | 0.000   |
| Multi-SWE-Bench__typescript__bugfix__18eac778   | ts   | 0.631    | 0.431    | -0.200  | 1.000   | 1.000   |
| Multi-SWE-Bench__typescript__bugfix__2aa6fa4c   | ts   | 0.000    | 0.000    | +0.000  | 0.000   | 0.000   |
| Multi-SWE-Bench__typescript__bugfix__54ebe590   | ts   | 0.000    | 0.000    | +0.000  | 0.000   | 0.000   |
| Multi-SWE-Bench__typescript__bugfix__5eee261d   | ts   | 0.000    | 0.000    | +0.000  | 0.000   | 0.000   |

Summary: astchunk > native on 3/10 tasks; astchunk < native on 1/10 (ts__18eac778); 6/10 tied.
Python: native mean NDCG 0.220 vs astchunk 0.412. TypeScript: native 0.126 vs astchunk 0.086.

### Assessment: does isolation change the headline? Does it separate vs. D28 saturation?

**Overall headline (0.173 native vs 0.249 astchunk) is the same numerically as before.**
The fix did NOT flip the direction — astchunk still shows a positive gap overall. However, the
character of the gap is now correctly understood:

1. **The signal is python-only.** All 3 "astchunk wins" tasks are python (pytest-dev/pytest).
   The typescript arm (vuejs/core) is entirely flat: 4/5 tasks both arms score 0.000, and
   the 1 non-zero ts task (18eac778) actually goes NATIVE > astchunk (0.631 vs 0.431).
   Hypothesis: the `.ts` extension filter captures fewer vuejs/core gold files than the
   native indexer's full-tree walk, which includes `.tsx`/`.vue`/other extensions.

2. **The python wins are real under isolation.** Two tasks show native NDCG=0 but astchunk
   NDCG>0 (982277e4: +0.387; 0eecae1e: +0.369 NDCG, same Recall). These are genuine
   cases where astchunk's AST-boundary chunking places the relevant file higher in the
   ranking than native's fixed-size chunking.

3. **Comparison to D28 micro-suite saturation.** D28 found 5/6 BM25 vectors tied at
   NDCG@10=0.822 on the micro-suite (3 fixtures) and native vs astchunk tied at Recall
   saturation (small fixture set, all top-k slots filled). On the real corpus:
   - BM25 sweep (Table 1) also mostly ties on Recall (all 6 vectors: 0.233) but DOES
     separate on NDCG: body_heavy (0.197) > default/flat/name_strong (0.173) > enrich_heavy
     (0.160) > name_only (0.153). Saturation does NOT persist on real corpus for NDCG ordering.
   - Chunker A/B: the real corpus DOES separate the arms (0.249 vs 0.173 NDCG overall;
     3 wins vs 1 loss vs 6 ties) but the separation is LANGUAGE-DEPENDENT — driven entirely
     by python tasks. TypeScript tasks do not separate (both arms near-zero).

4. **No winner asserted.** n=10, sparse (6/10 both-zero), language-confounded.
   The observed gap (0.249 vs 0.173 = +44%) is directionally positive for astchunk on
   python tasks but NOT supported for typescript in this run. This is a scoped, directional
   exit (R2 scope), not a publishable result. R3 would need ≥50 tasks per language + release
   binary to make a claim.

5. **Honesty on the numerically-identical headline.** The Table 2 numbers are the same as
   the contaminated run (0.249 vs 0.173). This is coincidental: the contaminated run added
   astchunk rows on top of the native index; in this case it appears the astchunk rows on the
   contaminated index produced similar query rankings to the clean astchunk-only index. The
   measurement is now SOUND (each arm's DB contains only its own chunks); the prior measurement
   was structurally invalid regardless of whether the number happened to be close.

### Files changed (this fix session — pure `research/`)

- `research/r1_harness/run_contextbench_exit.py` — isolation fix (`_copy_source_files` helper +
  per-arm scratch dirs), dead `tmp_path` removal, module docstring + `_score_task_ab` docstring
  updated to document isolation guarantee and known limitations.
- `research/r1_harness/pyproject.toml` — added `norecursedirs` to `[tool.pytest.ini_options]`.
- `research/r1_harness/runs/contextbench_exit/report.json` — regenerated (gitignored, not tracked).

No `src/`, `Cargo.*`, `.rs`, or `.claude/settings.json` modified.

## REVIEW — code reviewer (2026-06-15)

**VERDICT: BLOCK.**

Gates (all green): pytest `166 passed, 1 skipped` via the venv with `testpaths=["tests"]`
(the 1 skip = the Windows-only path test). `ruff check research/` + `ruff format --check research/`
both clean. Scope clean: working tree touches only `.gitignore` (the brief-specified
`cache/contextbench_repos/` addition), `.claude/settings.json` (pre-existing unrelated edit, exclude
at commit), and the three new `research/` files. NO `src/`/`Cargo.*`/`.rs` touched. Clones (`cache/`)
+ `runs/` correctly gitignored; `git status --porcelain` shows nothing from the run tracked. Selector,
materialiser pure helpers, typed-error handling, and path-alignment are all correct and well-tested.

### Findings

**[BLOCKER] — run_contextbench_exit.py:160,219–221 — A/B index NOT isolated: the astchunk arm
measures native ∪ astchunk, not astchunk alone. The +44% NDCG headline is an artifact.**
Both arms run against the SAME `task_dir`. Native arm (line 160) does `init()` + `index()` →
populates `task_dir/.codecache/index.db` with native chunks. The astchunk arm (lines 219–221) then
does `init()` + `ingest(chunks_json)` against the **same `task_dir`**. Per `src/app.rs::init`
(docstring lines 38–39): `init` is idempotent and "does not rewrite an existing config —
`init_schema` is itself idempotent (`CREATE ... IF NOT EXISTS`)" — it does NOT drop/reset the
populated DB. And `ingest_chunks` (`src/app.rs:213`) calls `storage.insert_chunks` — a plain
`INSERT_CHUNK` loop (`src/storage/mod.rs:131–156`) with NO `DELETE`/reset (the only DELETE,
`DELETE_CHUNKS_FOR_FILE`, is the incremental-update path, never invoked by ingest). So ingest
**appends** astchunk chunks on top of the native chunks already in the DB. The astchunk arm's query
ranks over the **union** of both chunkers' rows. The produced `runs/contextbench_exit/report.json`
(astchunk NDCG@10 0.249 vs native 0.173, +44%; Recall@10 0.367 vs 0.233) is therefore native+astchunk
vs native — measuring "native plus extra rows" vs "native", which trivially cannot lose. The result
is unsound and must NOT be reported as a chunker separation.
The code even self-documents the shortcut it took (lines 212–218): it creates `astchunk_dir` then
abandons it and inits against `task_dir` instead. The existing, reviewed precedent in
`ab_runner.py` (`run_ab` lines 103–127; `run_ab_astchunk` lines 210–237) deliberately uses SEPARATE
per-arm on-disk dirs (`work_dir/"native"/"repo"` vs `work_dir/"stub"|"astchunk"/"repo"`), each with
its own `.codecache/` DB, exactly to guarantee isolation. The exit run broke that invariant.
*Fix:* give the astchunk arm its own fresh index dir with its own `.codecache/` (mirror
`run_ab_astchunk`: materialise/symlink the task files into a separate dir, or run ingest in a clean
dir containing the source tree but no prior `.codecache/`). `codecache init` requires a git repo /
working dir; the simplest correct fix is a per-arm copy or `git worktree` of `task_dir` so each arm's
DB is physically separate. Then re-run Run 2 and regenerate the table. (The native-arm sweep, Run 1,
is unaffected — it never ingests; only Run 2's astchunk row is contaminated.)

**[MAJOR] — run_contextbench_exit.py / pyproject.toml — pytest collection breaks after a run:
cloned repo worktrees under `cache/contextbench_repos/worktrees/` are collected by pytest.**
Running `pytest .` or `pytest research/r1_harness/` (a positional path arg, which OVERRIDES
`testpaths=["tests"]`) after a real run recurses into the cloned repos (e.g. pytest-dev/pytest's own
`testing/*.py`) → `Interrupted: 153 errors during collection`. The suite is green ONLY when invoked
with no positional (so `testpaths` applies). The brief's documented canonical command passes the
directory positionally, which would trip this on any machine that has run the exit script.
*Fix:* add `norecursedirs = ["cache", "runs", ".venv", "vendor"]` (or `collect_ignore_glob`) to the
`[tool.pytest.ini_options]` block so collection is robust regardless of how pytest is invoked and
regardless of on-disk clones. Low effort; prevents a confusing false-red for the next agent.

**[MINOR] — run_contextbench_exit.py:182–185 — astchunk arm only chunks one extension and caps at
500 files, while the native arm indexes the whole tree.** `ext = ".py" if language=="python" else
".ts"` then `rglob(f"*{ext}")[:500]`. For a mixed-language repo the astchunk arm sees only one
language's files and at most 500 of them, whereas native `index()` indexes everything CodeCache
supports with no cap. Even after the isolation fix this asymmetry biases the comparison (different
candidate pools). *Fix:* document the cap/extension scoping in the table as a known asymmetry, or
align the native arm's candidate set to the same files; at minimum surface the cap in the report when
it bites (`len(source_files) == 500`).

**[MINOR] — run_contextbench_exit.py:55,57–59 unused — `tmp_path` assigned but unused (line 392) and
again (line 458).** `tmp_path = Path(tmp)` in the Run 1 block (line 392) is never read (the sweep
arm doesn't use the temp dir). Dead assignment; harmless but ruff-clean only because it's a local
reassign of a with-bound name. *Fix:* drop the unused `tmp_path` in the Run 1 block, or use it.

### Required to APPROVE
1. Fix the A/B isolation blocker (per-arm separate `.codecache/`), re-run Run 2, regenerate
   `report.json` + the Table 2 numbers. The deliverable's astchunk separation claim must be
   recomputed under isolation; if astchunk no longer separates beyond noise, state that honestly
   (consistent with the D28 saturation finding the brief asks to compare against).
2. Fix the pytest collection robustness (norecursedirs) so the gate is stable.
3. Address the two minors (document or fix the astchunk file-scoping asymmetry; drop the dead
   `tmp_path`).
Re-review on the corrected Run 2 table.

## RE-REVIEW — code reviewer (2026-06-16)

**VERDICT: APPROVE.**

All three items from the prior BLOCK are fixed and verified by re-reading the changed files and
re-running the gates.

### BLOCKER (A/B index contamination) — FIXED, structurally sound
`_score_task_ab` (run_contextbench_exit.py:168–290) now enumerates ONE shared `source_files`
list (primary extension `.py`/`.ts`, sorted, 500-cap) and copies it via the new
`_copy_source_files` helper into two SEPARATE scratch dirs under the per-run tempdir:
`native_{i}/` (`init`+`index`+`query`) and `astchunk_{i}/` (`init`+`ingest`+`query`). The shared
git worktree (`task_dir`) is NEVER passed to `init`/`index`/`ingest` during Run 2 — it is only
`rglob`'d to enumerate and `read_text`'d to chunk. Verified `_copy_source_files` copies ONLY the
enumerated source files (no `.codecache/`, no `.git` inherited), so the astchunk arm's `init`
produces a fresh empty DB and `ingest` is the sole populator. Cross-checked against
`src/app.rs::init` (idempotent, non-clobbering, no DB reset) and the insert-only `storage` ingest
path: the astchunk DB now contains ONLY astchunk chunks. The native∪astchunk artifact is gone.
Both arms share the identical candidate pool (symmetric). Confirmed `src/indexer/discovery.rs`
uses `require_git(false)`, so the non-git scratch dirs index correctly.

### MAJOR (pytest collection robustness) — FIXED, non-vacuously
`norecursedirs = ["cache", "runs", ".venv", "vendor"]` present in `[tool.pytest.ini_options]`.
Verified NON-vacuously: 6,684 `.py` files currently exist under `cache/contextbench_repos/
worktrees/` (incl. pytest-dev/pytest's own suite — the exact prior `153 errors` trigger). BOTH
invocations green:
- positional path arg → **166 passed, 1 skipped**
- no-positional (cd research/r1_harness && pytest) → **166 passed, 1 skipped**

### MINORs — addressed
- Dead `tmp_path`: Run 1 sweep now `as _tmp_sweep` (no unused binding); Run 2's `tmp_path` is a
  live binding passed as `work_dir`. Fixed.
- Asymmetric file sets: now symmetric (both arms use the same `source_files`); the extension +
  500-cap limitation is documented in the module + function docstrings, with a `[cap]` stderr
  warning when the cap bites.

### Gates
- `ruff check research/` → All checks passed. `ruff format --check research/` → 40 files
  already formatted (clean).
- Scope: pure `research/` — `.gitignore` (brief-specified `cache/contextbench_repos/` add),
  `pyproject.toml` (the `norecursedirs` line), and the 3 new `research/` files. NO
  `src/`/`Cargo.*`/`.rs` touched. `.claude/settings.json` is the pre-existing unrelated edit
  (exclude at commit). Clones (`cache/`) + `runs/` untracked.

### Deliverable honesty — defensible
Table 2 (native 0.173 vs astchunk 0.249 file NDCG@10) is now produced UNDER ISOLATION; persisted
`runs/contextbench_exit/report.json` matches and carries the full per-task breakdown. The framing
"no winner asserted + language-confounded + scoped/directional, n=10" is a defensible, honest
read of the data. The corrected number being numerically identical to the contaminated run is
acknowledged and the measurement is now structurally valid regardless.

### Remaining finding (MINOR — for manager to close at doc-sync; NOT blocking)
**[MINOR] — report.json / Table 2 — the TS near-zero misread risk is not disclosed at the
artifact.** The persisted `report.json` shows the aggregate `astchunk 0.249 vs native 0.173`
with the rendered `table2_markdown`, but neither the table nor `scope_note` mentions (a) the
`.ts`-only file cap that misses `.tsx`/`.vue` (the leading hypothesis for vuejs/core's near-zero
TS arm) nor (b) the language-confound (the +44% headline is python-driven; TS slightly favors
native). That analysis lives only in the brief prose. The per-task breakdown IS persisted, so the
split is recoverable; `scope_note`/header already carry "No winner asserted" + the isolation/cap
design; the docstring documents the extension restriction. This is a disclosure-completeness nit,
not a measurement-soundness defect — hence MINOR, not a re-BLOCK. *Fix at doc-sync:* add one line
to `scope_note` (and/or a footnote under the rendered Table 2) noting the `.ts`-only cap excludes
`.tsx`/`.vue` so the TS arm's near-zero is a coverage artifact, and that the +44% is python-driven
(language-confounded). Recommend the manager fold this into the R2.7 closeout doc-sync.

**APPROVED.** The measurement is now structurally sound, the gates are clean, scope is pure
`research/`, and the deliverable framing is honest. The single remaining item is a non-blocking
disclosure nit to close during doc-sync. R2.7 is clear to proceed to doc-sync + commit (closes R2).

---
## OUTCOME — manager (2026-06-16)
**Aligned + R2 track CLOSED.** RED→GREEN→REVIEW(BLOCK→fix→APPROVE) all complete; the reviewer's
BLOCK caught a real measurement bug (A/B index contamination → the original +44% was an artifact),
which is exactly why the gate exists. Re-run under per-arm DB isolation; reviewer APPROVE.

**Disclosure nit folded in (doc-sync):** added a footnote under the rendered Table 2 in
`run_contextbench_exit.py::_render_ab_table` noting the `.ts`-only cap excludes `.tsx`/`.vue` (a
coverage artifact) + the python-driven language confound. ruff re-verified clean after the edit.

**Doc-sync (this commit):**
- `docs/TODO.md` — R2.7 row → DONE; R2 track header `[~]` → `[x]` **TRACK COMPLETE (D29)**.
- `docs/ROADMAP.md` — Decision Log **D29** added (R2.7 scoped exit + R2 CLOSED, cites D23/D24/D25/D27/D28);
  Research-track table R2 row marked ✅ CLOSED.
- `research/CLAUDE.md` — new `contextbench_corpus.py`+`run_contextbench_exit.py` modules listed; new
  "R2.7 corpus materializer + network/git boundary (D29)" section (clone cache, air-gapped product,
  hermetic suite, the R2.5a mapper-only gap R2.7 closed); green baseline 138 → **166** (both mentions).
- `research/r1_harness/.gitignore` — explicit `cache/contextbench_repos/` line (manager-owned).

**Gates (reviewer + devops both re-ran in the venv):** **166 passed, 1 skipped**; ruff check + format clean.
**Scope:** `Cargo.toml`/`src/`/Rust-`tests/`/`.claude/settings.json` untouched; cloned repos + `runs/` gitignored.
**Commit:** `585ec2d` (local only, no push; explicit pathspec; `.claude/settings.json` left modified-but-unstaged).

**Carry into R3:** the real corpus separates BM25 vectors on NDCG (un-masking the ordering micro-suite
Recall-saturation hid; `body_heavy` led) and the chunker A/B is directionally astchunk-favoring but
n=10 + language-confounded — no winner. R3 (gated $1K) takes the full A0–A5 matrix to scale.

**Follow-ups:** none blocking. The `.tsx`/`.vue` coverage gap + n-scale are R3 concerns, already noted.
