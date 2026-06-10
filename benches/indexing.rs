//! Cold-index benchmark skeleton for `Indexer::index_all`.
//!
//! Measures the wall-clock time to run a full cold index over a small synthetic Python repo
//! (50 files × 2 functions each ≈ 100 functions, roughly 500 LOC). This is informational at
//! M5 — the harness is wired but **not gated** on CI; full budget validation is deferred to M10.
//!
//! Budget reference (`docs/plans/M5-indexer.md` §Performance budgets / `project_plan.md` §5.4):
//!   - cold 10K LOC  < 5 s
//!   - cold 100K LOC < 30 s
//!   - incremental 10 files < 2 s
//!   - index size < 100 MB
//!
//! This skeleton exercises the hot path (discover → hash → parse → chunk → insert) at ~1 500 LOC
//! so the bench completes in seconds on any dev machine. Scale to 10K/100K LOC at M10.
//!
//! Run:
//!   cargo bench --bench indexing
//! Save/compare baselines:
//!   cargo bench --bench indexing -- --save-baseline before
//!   cargo bench --bench indexing -- --baseline before

use std::fs;
use std::path::Path;

use codecache::config::Config;
use codecache::indexer::Indexer;
use codecache::storage::Storage;
use codecache::types::Language;
use criterion::{criterion_group, criterion_main, Criterion};
use tempfile::TempDir;

// ───────────────────────────── fixture helpers ──────────────────────────────

/// Render one Python function body, varying by index so each function is distinct.
fn py_function(i: usize) -> String {
    format!(
        r#"def function_{i}(x, y):
    """Docstring for function_{i}."""
    result = x + y + {i}
    helper_{i}(result)
    return result

def helper_{i}(value):
    """Helper for function_{i}."""
    return value * 2
"#
    )
}

/// Write `file_count` synthetic Python files under `root`, each containing two small functions.
/// Total LOC ≈ file_count × 10 (two 5-line functions per file).
fn write_synthetic_repo(root: &Path, file_count: usize) {
    for i in 0..file_count {
        let content = py_function(i);
        let path = root.join(format!("mod_{i:04}.py"));
        fs::write(&path, content).expect("write synthetic .py file");
    }
}

/// Create a temp dir, populate it with `file_count` `.py` files, and return the dir handle so it
/// lives as long as the bench iteration.
fn setup_repo(file_count: usize) -> TempDir {
    let dir = tempfile::tempdir().expect("create temp bench repo");
    write_synthetic_repo(dir.path(), file_count);
    dir
}

// ───────────────────────────── benchmark ────────────────────────────────────

/// Cold-index bench: `index_all()` over a freshly-created DB (no prior state).
///
/// Input: 50 Python files × ~10 LOC each ≈ 500 LOC (skeleton size, not the full 10K budget).
/// Each iteration rebuilds the `Indexer` and the DB so the cache is always cold.
fn bench_cold_index(c: &mut Criterion) {
    // FILE_COUNT is kept small so the bench finishes quickly at M5. Increase at M10 to reach
    // the 10K / 100K LOC budget checkpoints.
    const FILE_COUNT: usize = 50;

    // Write the synthetic repo once outside the timing loop — fixture I/O is not under test.
    let repo = setup_repo(FILE_COUNT);
    let repo_path = repo.path().to_path_buf();

    let mut group = c.benchmark_group("cold_index");
    // Low sample count so the suite completes in a reasonable wall-clock time on a dev machine
    // while still yielding a stable median. Criterion will raise it if variance is high.
    group.sample_size(10);

    group.bench_function("index_all_50_py_files", |b| {
        b.iter(|| {
            // Fresh DB per iteration — this is the "cold" part.
            let db_dir = tempfile::tempdir().expect("create temp db dir");
            let db_path = db_dir.path().join("index.db");

            let storage = Storage::new(&db_path).expect("open storage");
            storage.init_schema().expect("init schema");

            // Python-only config; index_paths empty → discovery defaults to repo_path.
            let config = Config {
                languages: vec![Language::Python],
                ..Config::default()
            };

            let mut indexer =
                Indexer::new(config, storage, repo_path.clone()).expect("create indexer");

            // Return stats so the compiler cannot optimize the call away.
            indexer.index_all().expect("index_all must succeed")
        });
    });

    group.finish();
}

criterion_group!(benches, bench_cold_index);
criterion_main!(benches);
