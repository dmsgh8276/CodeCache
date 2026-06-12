//! Query-latency benchmark for `Retriever::query` (M6.4).
//!
//! Measures the wall-clock latency of the **full retrieval path** —
//! preprocess → FTS5 `MATCH` → BM25 rank → stable tie-break → dedup → token-budget pack — over a
//! synthetic, pre-seeded index at ~100K-LOC scale (`project_plan.md` §11.2). The headline budget is
//! **p95 < 500ms on 100K LOC, cold cache** (`project_plan.md` §1.3 / §11.2).
//!
//! ## What is (and is not) timed
//! Only `Retriever::query` runs inside the criterion closure. The index is seeded **once, outside**
//! the timed region (see [`seed_storage`]): building the synthetic chunks, opening SQLite, and
//! `insert_chunks` are fixture cost, not query cost, so they must not be measured. This isolates the
//! retriever's hot path exactly as §11.2 breaks it down (FTS5 < 50ms, BM25 < 10ms, snippet < 20ms,
//! tokens < 10ms; warm total < 100ms, cold adds +10–20ms SSD disk I/O).
//!
//! ## Synthetic scale — decision
//! §11.2's budget is stated for a **100K-LOC monorepo**. Building that by *indexing* real source per
//! iteration would (a) dominate the closure with parse/chunk cost that is M5's budget, not M6's, and
//! (b) make the bench take minutes to set up. Instead we seed `Storage` **directly** with
//! [`CHUNK_COUNT`] synthetic chunks via `insert_chunks`, each ~[`LOC_PER_CHUNK`] LOC of body text, so
//! `CHUNK_COUNT * LOC_PER_CHUNK ≈ 100K LOC` of indexed material — the same FTS5 inverted-index size
//! the query traverses, without paying M5's indexing cost here. The chunks carry distinct
//! `symbol_name`s and bodies so the FTS5 term dictionary is realistically large and BM25 has real
//! work to rank. This is the documented, M6-appropriate way to reach the §11.2 scale; the *cold full
//! index* at this scale is M5's `indexing.rs` bench (skeleton) / M10's full suite.
//!
//! ## Percentiles (p50 / p95 / p99)
//! The timed closure runs **exactly one** `query`, so each criterion sample IS one query's latency.
//! Criterion reports mean / **median (≈ p50)** / std-dev / MAD and writes the raw per-sample data to
//! `target/criterion/query_latency/<bench>/new/{sample.json,estimates.json}`. True **p95/p99** are
//! derived from those raw samples (sort the per-iteration times, take the 95th/99th percentile). The
//! `/bench` skill / main session captures these after running; criterion does not print p95/p99 to
//! stdout directly, so they are read from `sample.json` (or computed by the skill).
//!
//! ## Budget tracking — NOT a hard gate (yet)
//! This slice **records** p95 against the 500ms target as a tracked baseline; it does **not** fail the
//! build if exceeded. Exceeding 500ms here is a **regression signal** to investigate, not a CI gate.
//! The hard budget gate (assertion / CI fail) is finalized in **M10** with the full criterion suite
//! and the token-reduction benchmark. See `docs/plans/M10-benchmarks-release.md`.
//!
//! ## FTS5 query-plan baseline (carried from M1, §11.2)
//! `EXPLAIN QUERY PLAN` for the §6 `SEARCH` statement was flagged at M1 as "capture at gate
//! execution". The main session can capture it alongside this bench (run the `SEARCH` SQL under
//! `EXPLAIN QUERY PLAN` against the seeded DB) and record it in `benches/CLAUDE.md` next to the
//! latency baseline. It is informational here; not asserted.
//!
//! Run:
//!   cargo bench --bench query_bench
//! Save / compare baselines:
//!   cargo bench --bench query_bench -- --save-baseline before
//!   cargo bench --bench query_bench -- --baseline before

use std::path::PathBuf;

use codecache::retriever::{QueryOptions, Retrieve, Retriever};
use codecache::storage::Storage;
use codecache::types::{Chunk, Language, SymbolType};
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

// ───────────────────────────── scale knobs ──────────────────────────────────

/// Number of synthetic chunks seeded into the index. With [`LOC_PER_CHUNK`] LOC of body each, this
/// targets ~100K LOC of indexed material (§11.2's reference scale) without paying M5's indexing cost.
const CHUNK_COUNT: usize = 5_000;

/// Approximate lines of body text per synthetic chunk. `CHUNK_COUNT * LOC_PER_CHUNK ≈ 100K LOC`.
const LOC_PER_CHUNK: usize = 20;

// ───────────────────────────── fixture helpers ──────────────────────────────

/// Render one synthetic chunk body of ~[`LOC_PER_CHUNK`] lines, varying by `i` so each chunk has a
/// distinct symbol name and a realistic spread of terms (so FTS5's term dictionary is large and BM25
/// has real ranking work). A subset of chunks deliberately includes the benchmarked query terms
/// ("authenticate"/"user"/"validate") so the query returns a non-trivial result set to rank + pack.
fn synthetic_body(i: usize) -> String {
    // Sprinkle the query terms into ~1 in 20 chunks so there is a real (but bounded) match set.
    let domain = if i % 20 == 0 {
        "authenticate user validate credentials session token"
    } else {
        "compute transform aggregate serialize render dispatch"
    };
    let mut body = format!("def symbol_{i}(arg_{i}):\n    \"\"\"{domain} for {i}.\"\"\"\n");
    for line in 0..LOC_PER_CHUNK {
        body.push_str(&format!("    step_{line} = {domain} + {i}\n"));
    }
    body
}

/// Build one synthetic [`Chunk`] with a distinct span and body. Spans are disjoint across `i` (each
/// chunk occupies its own 10_000-byte window) so dedup keeps them all — the query path still pays the
/// dedup + pack cost over the full result set.
fn synthetic_chunk(i: usize) -> Chunk {
    let body = synthetic_body(i);
    let start = i * 10_000;
    let end = start + body.len();
    Chunk {
        symbol_name: format!("symbol_{i}"),
        symbol_type: SymbolType::Function,
        file_path: PathBuf::from(format!("src/mod_{:05}.py", i / 50)),
        start_byte: start,
        end_byte: end,
        start_line: 1,
        end_line: LOC_PER_CHUNK + 2,
        chunk_text: body,
        language: Language::Python,
        parent_symbol: None,
        file_docstring: None,
        imports: Vec::new(),
        cross_references: Vec::new(),
        is_heuristic: false,
    }
}

/// Seed a fresh on-disk SQLite DB with [`CHUNK_COUNT`] synthetic chunks. Returns the temp dir (keep
/// it alive for the bench's duration) and the ready-to-query [`Storage`]. All of this is fixture
/// cost — it runs **outside** the timed closure.
///
/// Chunks are inserted in batches so a single giant transaction does not blow memory; `insert_chunks`
/// is itself one transaction per call (§ storage). On-disk (not in-memory) so the bench reflects the
/// real SQLite page-cache behavior §11.2 budgets against.
fn seed_storage() -> (TempDir, Storage) {
    let dir = tempfile::tempdir().expect("create temp bench db dir");
    let db_path = dir.path().join("index.db");
    let storage = Storage::new(&db_path).expect("open storage");
    storage.init_schema().expect("init schema");

    const BATCH: usize = 500;
    let mut batch: Vec<Chunk> = Vec::with_capacity(BATCH);
    for i in 0..CHUNK_COUNT {
        batch.push(synthetic_chunk(i));
        if batch.len() == BATCH {
            storage.insert_chunks(&batch).expect("seed insert_chunks");
            batch.clear();
        }
    }
    if !batch.is_empty() {
        storage
            .insert_chunks(&batch)
            .expect("seed insert_chunks tail");
    }

    (dir, storage)
}

// ───────────────────────────── benchmark ────────────────────────────────────

/// Query-latency bench: time **only** `Retriever::query` over the pre-seeded ~100K-LOC index.
///
/// The query "authenticate user" exercises the full path: preprocessing (tokenize/stopword/escape) →
/// FTS5 `MATCH` over the large inverted index → BM25 rank → stable tie-break → dedup → greedy
/// token-budget pack (default 4000 tokens, 20 results). `max_results = 20` keeps in-flight chunks
/// bounded (~10MB cap, §11.3) and matches the §3.2.3 default the CLI/MCP will use.
fn bench_query_latency(c: &mut Criterion) {
    // Seed once, outside the timed region — fixture cost, not query cost.
    let (_dir, storage) = seed_storage();
    let retriever = Retriever::new(storage);

    // §3.2.3 defaults: 4000-token budget, 20 results, no file filter — the real CLI/MCP shape.
    let options = QueryOptions::default();

    let mut group = c.benchmark_group("query_latency");
    // Higher sample count than the index bench: a single query is fast, so we can afford many samples
    // for a stable p50/p95/p99 distribution. Criterion will adapt if variance is high.
    group.sample_size(100);

    group.bench_function("query_authenticate_user_100k_loc", |b| {
        b.iter(|| {
            // Only this call is timed. `black_box` the inputs/outputs so the optimizer cannot hoist
            // or elide the query.
            let result = retriever
                .query(black_box("authenticate user"), black_box(options.clone()))
                .expect("query must succeed");
            black_box(result)
        });
    });

    group.finish();
}

criterion_group!(benches, bench_query_latency);
criterion_main!(benches);
