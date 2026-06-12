# BRIEF — M6 / M6.4 — query-latency bench (skeleton)

- **Milestone:** M6 — retriever  ·  **Module(s):** `benches/query_bench.rs`
- **Owner (manager):** principal-engineering-manager  ·  **Created:** 2026-06-10
- **Status:** PERF ✔  REVIEW ✔ (APPROVE)  DONE ✓ (bench run main session 2026-06-11: p95 = 1.17 ms vs 500 ms ✅)
- **Links:** docs/ROADMAP.md#m6--retriever · docs/plans/M6-retriever.md#slice-m64--latency-bench-perf · project_plan.md §1.3 / §11.2
- **Routing:** **performance-bench-engineer** (PERF) → code-reviewer. Manager verifies budget tracking.

## Goal
Wire a criterion latency bench over a synthetic ~100K-LOC-scale seeded index; measure p50/p95/p99
(§11.2) for the full `Retriever::query` path. Track the headline budget **p95 < 500ms** (§1.3).
Carry the FTS5 EXPLAIN QUERY PLAN baseline from M1. The full budget gate is finalized at M10; M6.4
wires + tracks (skeleton), it does not yet hard-fail CI on the budget.

## Scope (in / out)
- **In:** `benches/query_bench.rs` skeleton; synthetic-index seeding helper; p50/p95/p99 reporting;
  documented current numbers vs the 500ms p95 budget; note the §11.2 warm breakdown targets.
- **Out:** full criterion suite + hard budget gate → M10; token-reduction benchmark → M10.

## Scenarios / measurements
- [ ] cold-cache p95 over 100K-LOC synthetic index < 500ms (track; record actual)
- [ ] warm breakdown sanity vs §11.2 targets (FTS5 <50ms, BM25 <10ms, snippet <20ms, tokens <10ms)
- [ ] `max_results` bounded (default 20) so in-flight chunks stay ~10MB cap (§11.3)

## Definition of Done
- [ ] Bench compiles + runs via `/bench`; p95 recorded vs budget · clippy/fmt clean
- [ ] reviewer APPROVED · docs/TODO.md Phase 6 (M6.4) + src/retriever/CLAUDE.md (perf line) updated
- [ ] M10 follow-up noted: promote to hard budget gate in the full suite

---
## PERF — performance-bench-engineer

**Deliverable:** `benches/query_bench.rs` (new) + `[[bench]] name = "query_bench", harness = false`
in `Cargo.toml`. Matches `indexing.rs` criterion style (`criterion_group!`/`criterion_main!`,
`benchmark_group`, explicit `sample_size`, fixture built outside the closure).

**Bench shape.** One criterion bench `query_latency/query_authenticate_user_100k_loc`. The timed
closure (`b.iter`) contains **only** `retriever.query("authenticate user", QueryOptions::default())`
— i.e. the full M6 path: preprocess → FTS5 `MATCH` → BM25 rank → stable tie-break → `file_filter`
(None) → dedup → greedy token-budget pack. Inputs/output `black_box`'d so nothing is elided.

**Synthetic scale — decision.** Indexing real source to 100K LOC per iteration would dominate the
closure with M5 parse/chunk cost (not M6's budget) and take minutes to set up. So we seed `Storage`
**directly** via `insert_chunks` with `CHUNK_COUNT = 5_000` synthetic chunks × `LOC_PER_CHUNK = 20`
LOC of body each ≈ **100K LOC** of indexed material — the same FTS5 inverted-index size the query
traverses — all in `seed_storage()`, **outside** the timed region. On-disk SQLite (temp dir, not
`:memory:`) so SQLite page-cache behavior is realistic. ~1/20 of chunks carry the query terms
("authenticate"/"user"/…) so there is a real but bounded match set; `max_results = 20` (the §3.2.3
default) caps in-flight chunks (~10MB, §11.3). Inserts batched at 500/transaction.

**Percentiles.** The timed closure runs exactly **one** query, so each criterion sample IS one
query's latency. Criterion prints mean / **median (≈ p50)** / std-dev / MAD and writes raw per-sample
data to `target/criterion/query_latency/<bench>/new/{sample.json,estimates.json}`. **p95/p99 are not
printed to stdout** — they are derived from the raw samples (sort per-iteration times, take 95th/99th
percentile). The `/bench` skill / main session captures them after the run. `sample_size = 100`.

**p95 < 500ms baseline — recorded where.** The 500ms target (§1.3 / §11.2) is documented in the bench
module doc-comment and in `benches/CLAUDE.md`; the **actual** measured p50/p95/p99 are recorded by the
main session after `cargo bench --bench query_bench`. **This is a TRACKED BASELINE, not a hard CI
gate** — exceeding 500ms is a regression signal to investigate; the hard budget gate (assert / CI
fail) + full suite land at **M10**. The bench contains **no** latency assertion (honest: the manager
subagent cannot measure, so no fabricated `assert!`).

**FTS5 EXPLAIN QUERY PLAN baseline (§11.2, carried from M1).** Noted in the bench doc-comment as a
main-session follow-up: run the §6 `SEARCH` SQL under `EXPLAIN QUERY PLAN` against the seeded DB and
record the plan in `benches/CLAUDE.md` next to the latency baseline. Informational; not asserted here.

**Files changed (this hand-off):**
- `benches/query_bench.rs` — NEW (the bench).
- `Cargo.toml` — added `[[bench]] name = "query_bench", harness = false`.
- `docs/TODO.md` — M6.4 marked `[x]` with "bench numbers + gates pending main-session run".
- `src/retriever/CLAUDE.md` — Perf section + Status updated.
- `benches/CLAUDE.md` — M6.4 row + baseline placeholder (pending main-session numbers).

**PLACEHOLDERS awaiting the main-session `cargo bench` run (NOT measured by the manager subagent):**
actual p50/p95/p99 latency; the p95-vs-500ms verdict; clippy/fmt-clean confirmation on the bench;
the EXPLAIN QUERY PLAN text. Do **not** treat any latency number as real until the main session runs it.

## REVIEW — code reviewer

**Verdict: APPROVE** (subject to the main-session compile/bench run — reviewer cannot run cargo either).

Honesty checks (the focus of this slice):
- **Timed section scoped correctly.** The `b.iter` closure contains only `retriever.query(...)` plus
  the required per-call `options.clone()` (`query` takes `QueryOptions` by value, so this is real call
  cost, not measurement noise) and `black_box` wrapping. No seeding, no DB open, no chunk-building
  inside the closure. ✔
- **Fixture outside the closure.** `seed_storage()` (open DB, schema, 5 000-chunk insert) runs once
  before `bench_function`; the `TempDir` is bound to `_dir` so it outlives the iterations. ✔
- **No misleading assertions.** There is **no** `assert!(p95 < 500ms)` and **no** fabricated latency
  number anywhere — the budget is documented as a tracked baseline; the `.expect("query must succeed")`
  only guards correctness (a failed query would be a real bug), not latency. ✔
- **Scale documented + justified.** The module doc-comment explains why we seed `Storage` directly
  instead of indexing real source, and states the ~100K-LOC arithmetic. ✔
- **Percentile derivation documented.** p50≈median, p95/p99 from raw `sample.json`. ✔
- **Not a CI gate.** Explicitly stated in the doc-comment, `benches/CLAUDE.md`, and `docs/TODO.md`;
  hard gate deferred to M10. ✔

API-correctness checks (against confirmed signatures): `Storage::new/init_schema/insert_chunks`,
`Retriever::new`, `Retrieve::query` (trait imported — required for the method to resolve),
`QueryOptions::default` + `Clone`, all 14 `Chunk` fields, `SymbolType::Function`, `Language::Python`,
`criterion::black_box` — all match. `[[bench]] harness = false` mirrors `indexing`. No
`unwrap()` on reachable production paths (bench `expect`s are fixture/setup, acceptable in a bench).

**Gate to main session:** compile-check (`cargo clippy --all-targets -- -D warnings` covers benches),
`cargo bench --bench query_bench`, fill the p50/p95/p99 + EXPLAIN QUERY PLAN placeholders, then commit.

## OUTCOME — manager

**Status: PERF ✔  REVIEW ✔ (APPROVE)  DONE ✓ — bench run by the main session, M6 complete.**

Bench wired and reviewed; no production `src/` logic changed (this slice adds a criterion bench only).
M6.4 is the final M6 slice — with the main-session bench run, **M6 (retriever) is complete**.

**Main-session run (Rust 1.85, release, Windows 11, 2026-06-11):**
1. `cargo clippy --all-targets -- -D warnings` clean; `cargo fmt --all -- --check` clean.
2. `cargo bench --bench query_bench` over the seeded ~100K-LOC index (100 samples): criterion mean CI
   [0.99, 1.03] ms; from `sample.json` → **p50 = 1.02 ms, p95 = 1.17 ms, p99 = 1.22 ms** (min 0.74 /
   max 1.24). **p95 is ~425× under the 500 ms budget ✅** — tracked baseline, hard gate remains M10.
3. EXPLAIN QUERY PLAN for the §6 `SEARCH` SQL — **deferred to M10** (the bench DB is a per-run
   tempfile; capture against a persistent fixture with the hard budget gate). Recorded as an M10 item.
4. `_TODO_` placeholders in `benches/CLAUDE.md` filled with the measured numbers; verdict recorded.
5. Committed by the main session — M6.4 done.
