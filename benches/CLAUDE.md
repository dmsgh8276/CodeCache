# benches/ — CLAUDE.md

Criterion benchmark harnesses and the performance budgets they guard. **Owner agent:**
`performance-bench-engineer`. Budgets: [`../docs/project_plan.md`](../docs/project_plan.md) §5.4 / §11;
scenarios in [`../docs/TEST_STRATEGY.md`](../docs/TEST_STRATEGY.md).

## Purpose
Back performance claims with measurements, not intuition. Each perf-critical path gets a
criterion bench compared against its documented budget; regressions are caught before release.

## Budgets (from project_plan §1.3 / §5.4 / §11 — enforced at the milestones that own each path)
| Path | Budget | Milestone | Measured (M10.1, Win11/Rust 1.85/release) | Status |
|---|---|---|---|---|
| Query latency | p95 < 500ms (100K LOC) | M6 / M10 | p95 = 0.51 ms (M6.4: 1.17 ms) | PASS (≫ headroom) |
| Full index size | < 100MB (100K-LOC synthetic) | M5 / M10 | 12.32 MB (12,922,880 B) | PASS (hard assert) |
| Incremental re-index | < 2s (10-file change) | M5 / M10 | p50 = 190 ms | PASS (~10×) |
| Cold index 10K LOC | < 5s | M10 | Win11 p50 = 6.04 s; **D20 batching → WSL2/Linux p50 = 1.37 s** | **MISS-on-Win11 pending; PASS on Linux (D20)** |
| Cold index 100K LOC | < 30s | M10 | p50 = 13.54 s | PASS (>2×) |
| Hash 1K files | < 500ms | M10 | 459 ms total (compute_file_hash) | PASS (hard assert) |
| Retrieval quality (L1) | recorded vs gold (no hard gate) | M10 (D16) | M10.2 | — |

> The retired "token reduction ≥40% vs file dumping on 5 tasks" budget is **gone** (D16): file
> dumping is a strawman baseline in 2026; the grep-baseline comparison is agent-in-loop at research
> track R3, not a v0.1 release gate. M10.2 records Layer-1 retrieval quality only.

## Status
Real benches landed at **M10.1** (full criterion suite). Latency/index benches were wired earlier
where their module shipped (M5 indexer, M6 retriever). Run via the `/bench` skill; CI runs them on
schedule (`bench.yml`, M10.3).

### M10.1 — full systems-budget suite (2026-06-12)
Benches: `indexing.rs` (`cold_index/{500_loc,10k_loc,100k_loc}`, `incremental/10_files`,
`index_size/100k_loc_db_bytes`), `query_bench.rs` (M6.4, re-confirmed), `hashing_bench.rs`
(`hash_1k_files/{compute_file_hash,compute_content_hash}_per_file` + one-shot 1K-file hard assert).
Fixture I/O is outside every timed closure; inputs are deterministic. Numbers in the budget table
above; p95/p99 read from `target/criterion/.../new/sample.json` (criterion prints only the median).
**Assertion policy:** hard asserts only where the metric is stable and met (index size; hash 1K);
all machine-variable timings are tracked-not-asserted and trend-guarded by `bench.yml` (M10.3).

**Cold-index 10K-LOC MISS (6.04 s vs < 5 s) — Decision Log D20.** Tracked, NOT a v0.1 release
blocker; deferred to a v0.1.x test-first optimization slice. Root cause: per-file SQLite transaction
commits (`Storage::insert_chunks` opens+commits a transaction per file → ~200 fsyncs for a 200-file
index) plus per-iteration cold-DB creation on Windows. The harder 100K < 30 s target passes with
>2× margin (non-monotonic vs budget confirms fixed per-iteration overhead, not algorithmic blowup).
**Follow-up (v0.1.x):** batch inserts across files into one transaction (preserving D2 per-file
isolation), then re-measure 10K cold index.

**Follow-up RESOLVED 2026-06-17 (D20 batching).** `Storage::write_in_transaction` runs the whole
index run's per-file writes inside one outer transaction (savepoint per file; D2 preserved). Measured
on **this WSL2/Linux machine** (Rust 1.85, release, before/after via a `git worktree` at HEAD):
10K cold-index p50 **5.84 s → 1.37 s (−76.5%)**, p95 6.18 s → 1.57 s — both well under < 5 s here.
The unbatched 5.84 s on Linux closely tracks the Win11 6.04 s, confirming the bottleneck was
commit/fsync COUNT (≈200 per-file commits → 1), not parse/tree-sitter/FTS5 work — so the speedup is
platform-independent. **Windows CI remains the authoritative budget gate:** absolute timings are not
comparable across machines, so the table stays "MISS-on-Win11 pending" until a Windows run confirms
< 5 s there; a Windows pass is strongly expected given the mechanism. (Per the M10 assertion policy,
machine-variable cold-index timings are trend-tracked, not hard-asserted in CI.)

### M10.1 — FTS5 query-plan baseline (EXPLAIN QUERY PLAN, persistent DB) — carried from M1/M6.4
M6.4 deferred this because its bench DB was a per-run tempfile. Captured at M10.1 against a
**persistent** on-disk DB seeded via the public `Storage` API (5_000 chunks ≈ 100K LOC) by
`examples/explain_query_plan.rs` (`cargo run --release --example explain_query_plan`). The §6
`SEARCH` SQL (`src/storage/queries.rs::SEARCH`) under `MATCH "authenticate OR user" LIMIT 20`:
```
QUERY PLAN
|--SCAN symbols VIRTUAL TABLE INDEX 0:M13
`--USE TEMP B-TREE FOR ORDER BY
```
Read (specialist): the FTS5 inverted index **is** used (`VIRTUAL TABLE INDEX 0:M13` = the MATCH
path, not a full scan); the contentful FTS5 table (D11) returns the UNINDEXED columns
(`file_path`/`start_byte`/`end_byte`/`start_line`/`end_line`/`language`) with **no second lookup**
(zero-extra-lookup, as D7/D11 intended); `USE TEMP B-TREE FOR ORDER BY` is the expected bounded
sort on the computed `bm25()` value (sorts only the match set, not the corpus), with `rowid ASC` the
deterministic tie-break. No concerns — textbook FTS5 MATCH + bm25-rank plan. Re-confirmed
independently via the `sqlite3` 3.41.2 shell against the same file. If `SEARCH` ever changes,
re-capture this and update `examples/explain_query_plan.rs`'s verbatim copy in the same change.

### M10.2 — Layer-1 retrieval-quality scoring (D16; not a criterion bench — lives in `tests/`)
The retrieval-quality scorer is `tests/retrieval_quality.rs` (deterministic, offline, no LLM spend),
scoring `Retriever` output against the committed gold-context fixture
`tests/fixtures/retrieval_quality/micro_suite.json` (single source of truth, loaded via serde_json).
Metrics: Recall@k / Precision@k / F1@k at **file** and **block(function)** granularity (metric math
unit-tested test-first — 14 unit tests; the integration test seeds Storage via the public API).
**Offline micro-suite proxy** of 15 queries (3 corpora × 5) — the real ContextBench corpus is not
vendorable offline; same protocol, R2 swaps in the real corpus (**Decision Log D21**). No hard gate
at M10 (D16: "recorded vs gold"). **Measured @k=10 macro (2026-06-12):** keyword (N=13) file
Recall=1.000 / F1≈0.51, block Recall=1.000 / F1≈0.49; semantic (N=2, e.g. "error handling") file
Recall@10=0.000 — expected BM25-only semantic gap (D1 informational, the v0.2 hybrid rationale; not a
gate). The scoring method is documented verbatim in the `tests/retrieval_quality.rs` module doc for
R2/R3 reuse. Run: `cargo test --test retrieval_quality -- --nocapture` (prints the per-corpus tables).

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
