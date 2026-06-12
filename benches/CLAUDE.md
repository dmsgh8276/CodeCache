# benches/ — CLAUDE.md

Criterion benchmark harnesses and the performance budgets they guard. **Owner agent:**
`performance-bench-engineer`. Budgets: [`../docs/project_plan.md`](../docs/project_plan.md) §5.4 / §11;
scenarios in [`../docs/TEST_STRATEGY.md`](../docs/TEST_STRATEGY.md).

## Purpose
Back performance claims with measurements, not intuition. Each perf-critical path gets a
criterion bench compared against its documented budget; regressions are caught before release.

## Budgets (from project_plan §11 — enforced at the milestones that own each path)
| Path | Budget | Milestone |
|---|---|---|
| Query latency | p95 < 500ms | M6 |
| Full index size | < 100MB (reference repo) | M5 |
| Incremental re-index | < 2s (single changed file) | M5 |
| Token reduction | ≥ 40% vs full-file context (5 tasks) | M10 |

## Status
M0: placeholder only (`.gitkeep`). Real benches land at **M10** (full criterion suite +
token-reduction benchmark), with latency/index benches wired earlier where their module ships
(M5 indexer, M6 retriever). Run via the `/bench` skill; CI runs them on schedule (`bench.yml`).

**M5.2: `indexing.rs` wired (cold-index skeleton — informational, not CI-gated).**
- Bench: `cold_index/index_all_50_py_files` — 50 synthetic Python files (~500 LOC), cold SQLite DB
  per iteration.
- Baseline (Windows 11, Rust 1.85, release profile, 2026-06-10): p50 ≈ 1.10 s (range 1.02–1.20 s).
- Budget (§5.4): cold 10K LOC < 5 s, 100K LOC < 30 s. The skeleton exercises ~500 LOC; full budget
  validation at these scales is deferred to M10. Linear extrapolation (1.1 s / 500 LOC ≈ 22 s /
  10K LOC) suggests the hot path needs profiling before M10 to meet the 5 s budget at 10K LOC.
- To compare across changes: `cargo bench --bench indexing -- --save-baseline <tag>` then
  `cargo bench --bench indexing -- --baseline <tag>`.

**M6.4: `query_bench.rs` wired (query-latency baseline — tracked, NOT a CI gate).**
- Bench: `query_latency/query_authenticate_user_100k_loc` — times **only** `Retriever::query` over a
  pre-seeded ~100K-LOC index (5 000 synthetic chunks ×20 LOC, seeded via `insert_chunks` **outside**
  the timed closure; on-disk temp SQLite). `query "authenticate user"`, `QueryOptions::default()`
  (4000-token budget, `max_results=20` — in-flight chunks ~10MB cap §11.3). `sample_size=100`.
- Budget (§1.3 / §11.2): **p95 < 500ms** on 100K LOC cold cache (warm breakdown target <100ms: FTS5
  <50ms, BM25 <10ms, snippet <20ms, tokens <10ms, format <10ms). **Tracked baseline only at M6** —
  exceeding it is a regression signal, not a build failure. Hard budget gate + full suite at M10.
- Percentiles: each criterion sample = one query, so median ≈ p50; **p95/p99 from raw
  `target/criterion/query_latency/.../new/sample.json`** (criterion does not print them to stdout).
- **Baseline (main-session `cargo bench --bench query_bench`, Windows 11, Rust 1.85, release, 2026-06-11):**
  **p50 = 1.02 ms, p95 = 1.17 ms (vs 500 ms target — ~425× headroom ✅), p99 = 1.22 ms** (criterion
  mean CI [0.99, 1.03] ms; 100 samples; min 0.74 / max 1.24 ms; 1 outlier). Query latency is
  dominated by FTS5 MATCH + the in-memory rank/dedup/pack; well within budget at 100K-LOC scale.
  EXPLAIN QUERY PLAN for the §6 `SEARCH` statement (carried from M1) — **deferred to M10** with the
  hard budget gate (the bench's temp DB is torn down per run; M10 captures it against a persistent
  fixture). The p95 budget is the M6 headline and is met with multiple orders of magnitude to spare.
- To compare across changes: `cargo bench --bench query_bench -- --save-baseline <tag>` then
  `cargo bench --bench query_bench -- --baseline <tag>`.

## Rules
- A bench exists for every budget claimed in `project_plan.md` §11 by the time M10 closes.
- Keep harness inputs deterministic and committed (or generated reproducibly) so numbers compare.
- Do not gate the fast inner-loop (`cargo test`) on benches; perf runs on demand / scheduled.
