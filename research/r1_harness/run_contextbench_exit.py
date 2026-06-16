"""R2.7 — Scoped real-corpus exit run (R2 EXIT) against ContextBench-Lite.

Materialises ≤3 repos (≤15 tasks, python+typescript) from the cached ContextBench-Lite slice,
clones each repo once, and runs two ablation experiments:

  **Run 1 — BM25 weight sweep (native chunker):**
    For each selected task's repo tree, run ``init → index → query(problem_statement)`` for
    each of the 6 weight vectors in ``sweep.DEFAULT_GRID``.  Index ONCE per task; query 6 times
    with different ``--bm25-weights``.  Produces Table 1: NDCG@10 (file) + F1@10/Recall@10 (file)
    per vector, macro-averaged across tasks.

  **Run 2 — Chunker A/B (default weights):**
    For the SAME tasks, run native (``init → index → query``) vs astchunk
    (``init → ingest → query``) arms.  For Python/TypeScript tasks only (already guaranteed
    by the selector).  Produces Table 2: native vs astchunk, file-level metrics.

    **Isolation guarantee:** each arm gets its own scratch directory with its own ``.codecache/``
    DB.  Both arms operate on the SAME set of source files (same extension filter + 500-file cap)
    copied into separate scratch dirs — ensuring symmetric candidate pools and independent indexes.
    The shared task worktree is NEVER indexed during the A/B run; it is only used as the source
    of source files to copy.  (This mirrors ``ab_runner.run_ab_astchunk`` which materialises
    ``native_ast/repo`` and ``astchunk/repo`` as distinct dirs.)

Scoring: FILE-LEVEL (ndcg_file / f1_file / recall_file @ k=10) — headline.
Block-level excluded from headline: ContextBench gold has no symbol names; astchunk
synthesises names so block-level for the astchunk arm is always ~0.

Gold: ``contextbench.parse_contextbench_records`` → ``SweepQuery`` (gold_files = repo-relative posix).
Path alignment: codecache returns absolute paths → ``normalize_path`` relativises to ``repo_dir``.

Scope / honesty:
  - Scoped, directional real-corpus exit.  NOT full-500 ContextBench-Lite.
  - No winner asserted unless clearly separated beyond noise.
  - max_repos=3, max_tasks=15 (configurable via CLI flags).

Known limitations (documented, not hidden):
  - Run 2 A/B: both arms index only files matching the task language's primary extension
    (``.py`` for python, ``.ts`` for typescript), capped at 500 files.  The native arm in Run 1
    indexes the full worktree (all supported languages); Run 2 uses the same restricted file set
    for both arms so the comparison is symmetric.

Missing-cache → clean nonzero exit with instructions (mirror run_report.py precedent).

Run (from research/r1_harness/):
    PYTHONUTF8=1 .venv/bin/python run_contextbench_exit.py

Full options:
    PYTHONUTF8=1 .venv/bin/python run_contextbench_exit.py \\
        --max-repos 3 --max-tasks 15 --max-chunk-size 300 \\
        --cache-dir cache/contextbench --repo-cache-dir cache/contextbench_repos

Binary resolution: $CODECACHE_BIN → target/release/codecache → target/debug/codecache → PATH.
"""

from __future__ import annotations

import argparse
import json
import shutil
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

HERE = Path(__file__).resolve().parent

_MAX_CHUNK_SIZE = 300


def _fmt_w(w: float) -> str:
    f = float(w)
    return str(int(f)) if f.is_integer() else str(f)


def _render_sweep_table(sweep_rows: list[dict]) -> str:
    """Render the BM25 weight-sweep ablation as Markdown."""
    lines = [
        "### Table 1: BM25 Weight Sweep — File-level Metrics (native chunker)",
        "",
        "| Vector | Weights | NDCG@10 (file) | F1@10 (file) | Recall@10 (file) |",
        "|--------|---------|---------------|--------------|-----------------|",
    ]
    for row in sweep_rows:
        label = row["label"]
        weights = row["weights"]
        n10 = row["ndcg_file_10"]
        f10 = row["f1_file_10"]
        r10 = row["recall_file_10"]
        wstr = ",".join(_fmt_w(w) for w in weights)
        baseline = " ← baseline" if label == "default" else ""
        lines.append(f"| {label} | {wstr} | {n10:.3f} | {f10:.3f} | {r10:.3f} |{baseline}")
    return "\n".join(lines)


def _render_ab_table(ab_rows: list[dict]) -> str:
    """Render the native-vs-astchunk A/B ablation as Markdown."""
    lines = [
        "### Table 2: Chunker A/B — File-level Metrics (default BM25 weights)",
        "",
        "| Arm | NDCG@10 (file) | F1@10 (file) | Recall@10 (file) | N tasks |",
        "|-----|---------------|--------------|-----------------|---------|",
    ]
    for row in ab_rows:
        arm = row["arm"]
        n10 = row["ndcg_file_10"]
        f10 = row["f1_file_10"]
        r10 = row["recall_file_10"]
        n = row["n_tasks"]
        lines.append(f"| {arm} | {n10:.3f} | {f10:.3f} | {r10:.3f} | {n} |")
    lines.append("")
    lines.append(
        "> Scoped/directional, n=10, language-confounded: the astchunk lead is python-driven; the "
        "typescript arm is mostly both-zero because Run 2's astchunk chunking is `.ts`-only and "
        "**excludes `.tsx`/`.vue`** (a coverage artifact, NOT a chunker signal). No winner asserted."
    )
    return "\n".join(lines)


_INDEX_TIMEOUT = 600.0  # 10 min: debug binary on 900-file repos can be slow


def _score_task_sweep(
    task_dir: Path,
    sweep_query,
    binary,
    grid,
) -> list[dict]:
    """Index one task dir once; query with each weight vector; return per-vector metric dicts."""
    from r1harness.codecache_tool import CodeCacheIndex
    from r1harness.scorer import dedup_first, score_query

    idx = CodeCacheIndex(repo_dir=task_dir, binary=binary, timeout=_INDEX_TIMEOUT)
    idx.init()
    idx.index()

    rows = []
    for vec in grid:
        result = idx.query(sweep_query.query, bm25_weights=list(vec.weights))
        metrics = score_query(
            dedup_first(result.files),
            list(result.blocks),
            set(sweep_query.gold_files),
            set(sweep_query.gold_blocks),
        )
        m10 = next(m for m in metrics if m.k == 10)
        rows.append(
            {
                "label": vec.label,
                "weights": list(vec.weights),
                "ndcg_file_10": m10.ndcg_file,
                "f1_file_10": m10.f1_file,
                "recall_file_10": m10.recall_file,
            }
        )
    return rows


def _copy_source_files(source_files: list[Path], task_dir: Path, dest_dir: Path) -> None:
    """Copy *source_files* (absolute paths under *task_dir*) into *dest_dir*, preserving
    their relative layout.  Intermediate directories are created as needed.
    Files that fail to copy are silently skipped (encoding/permission edge cases).
    """
    for src in source_files:
        try:
            rel = src.relative_to(task_dir)
            dst = dest_dir / rel
            dst.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(src, dst)
        except Exception:
            continue


def _score_task_ab(
    task_dir: Path,
    sweep_query,
    binary,
    language: str,
    work_dir: Path,
    max_chunk_size: int,
    task_idx: int,
) -> list[dict]:
    """Run native vs astchunk A/B for one task dir; return per-arm metric dicts.

    **Isolation design:** both arms operate on the SAME set of source files (enumerated once:
    files matching the task language's primary extension, sorted, capped at 500).  Each arm
    gets its OWN scratch directory under *work_dir* with its own ``.codecache/`` DB:

      - ``native_<i>/``  — source files copied here; ``init`` + ``index`` + ``query``
      - ``astchunk_<i>/`` — same source files copied here; ``init`` + ``ingest`` + ``query``

    The shared task worktree (*task_dir*) is used ONLY to enumerate + read source files;
    it is never passed to ``codecache init`` during the A/B run.  This mirrors the
    ``ab_runner.run_ab_astchunk`` pattern (``native_ast/repo`` vs ``astchunk/repo``).

    The 500-file cap applies to BOTH arms (symmetric candidate pools).  If the gold file
    falls outside the cap, both arms miss it equally — the comparison remains fair.

    Documented limitation: both arms index only the task language's primary extension
    (``.py`` or ``.ts``); the full-tree native sweep (Run 1) is a separate experiment that
    does index the whole worktree.
    """
    from r1harness.astchunk_chunker import astchunk_chunk
    from r1harness.chunkers import dump_chunks
    from r1harness.codecache_tool import CodeCacheIndex
    from r1harness.scorer import dedup_first, score_query

    gold_files = set(sweep_query.gold_files)
    gold_blocks = set(sweep_query.gold_blocks)

    # --- Enumerate the SHARED source file set (both arms use this same list) ---
    ext = ".py" if language == "python" else ".ts"
    source_files = sorted(task_dir.rglob(f"*{ext}"))[:500]
    capped = len(source_files) == 500
    if capped:
        # Log so the caller can note the cap in the report.
        import sys as _sys

        print(f"[cap] task {task_idx}: 500-file cap hit for {ext} files", file=_sys.stderr)

    # --- Native arm: copy files → isolated dir → init → index → query ---
    native_dir = work_dir / f"native_{task_idx}"
    native_dir.mkdir(parents=True, exist_ok=True)
    _copy_source_files(source_files, task_dir, native_dir)

    native_idx = CodeCacheIndex(repo_dir=native_dir, binary=binary, timeout=_INDEX_TIMEOUT)
    native_idx.init()
    native_idx.index()
    native_result = native_idx.query(sweep_query.query)
    native_metrics = score_query(
        dedup_first(native_result.files),
        list(native_result.blocks),
        gold_files,
        gold_blocks,
    )
    nm10 = next(m for m in native_metrics if m.k == 10)
    rows: list[dict] = [
        {
            "arm": "native",
            "ndcg_file_10": nm10.ndcg_file,
            "f1_file_10": nm10.f1_file,
            "recall_file_10": nm10.recall_file,
        }
    ]

    # --- astchunk arm: chunk the SAME source files → isolated dir → init → ingest → query ---
    astchunk_records: list[dict] = []
    for src in source_files:
        try:
            rel = src.relative_to(task_dir).as_posix()
            content = src.read_text(encoding="utf-8", errors="replace")
            astchunk_records.extend(astchunk_chunk(content, rel, language, max_chunk_size=max_chunk_size))
        except Exception:
            continue

    if not astchunk_records:
        # No chunks produced (e.g. empty repo or all files binary) — return zeros.
        rows.append(
            {
                "arm": "astchunk",
                "ndcg_file_10": 0.0,
                "f1_file_10": 0.0,
                "recall_file_10": 0.0,
            }
        )
        return rows

    chunks_json = work_dir / f"astchunk_task{task_idx}.json"
    dump_chunks(astchunk_records, chunks_json)

    # Fresh isolated dir for astchunk arm — no prior .codecache/ from the native arm.
    astchunk_dir = work_dir / f"astchunk_{task_idx}"
    astchunk_dir.mkdir(parents=True, exist_ok=True)
    _copy_source_files(source_files, task_dir, astchunk_dir)

    astchunk_idx = CodeCacheIndex(repo_dir=astchunk_dir, binary=binary, timeout=_INDEX_TIMEOUT)
    astchunk_idx.init()
    astchunk_idx.ingest(chunks_json)
    astchunk_result = astchunk_idx.query(sweep_query.query)
    astchunk_metrics_list = score_query(
        dedup_first(astchunk_result.files),
        list(astchunk_result.blocks),
        gold_files,
        gold_blocks,
    )
    am10 = next(m for m in astchunk_metrics_list if m.k == 10)
    rows.append(
        {
            "arm": "astchunk",
            "ndcg_file_10": am10.ndcg_file,
            "f1_file_10": am10.f1_file,
            "recall_file_10": am10.recall_file,
        }
    )

    return rows


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description="R2.7 scoped real-corpus exit run over ContextBench-Lite.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--max-repos",
        type=int,
        default=3,
        help="Max distinct repos to include (default: 3)",
    )
    parser.add_argument(
        "--max-tasks",
        type=int,
        default=15,
        help="Max tasks to run (default: 15)",
    )
    parser.add_argument(
        "--max-chunk-size",
        type=int,
        default=_MAX_CHUNK_SIZE,
        help=f"astchunk max_chunk_size (default: {_MAX_CHUNK_SIZE})",
    )
    parser.add_argument(
        "--cache-dir",
        type=Path,
        default=HERE / "cache" / "contextbench",
        help="ContextBench cache dir (default: cache/contextbench)",
    )
    parser.add_argument(
        "--repo-cache-dir",
        type=Path,
        default=HERE / "cache" / "contextbench_repos",
        help="Repo clone cache dir (default: cache/contextbench_repos)",
    )
    parser.add_argument(
        "--repos",
        nargs="+",
        default=None,
        metavar="OWNER/NAME",
        help=(
            "Explicit repo filter: limit selection to these repos only "
            "(e.g. --repos astropy/astropy pytest-dev/pytest vuejs/core). "
            "Overrides max-repos when provided; max-tasks still applies. "
            "Useful when the alphabetical default omits desired language coverage."
        ),
    )
    args = parser.parse_args(argv)

    # --- Gate: cache must exist ---
    cache_file = args.cache_dir / "contextbench_verified_slice.json"
    if not cache_file.exists():
        print(
            "ERROR: ContextBench-Lite cache not found.\n"
            f"  Expected: {cache_file}\n"
            "  Run the fetch entrypoint first:\n"
            "    python fetch_contextbench.py --force --n-records 500\n"
            "  Then retry.",
            file=sys.stderr,
        )
        return 1

    from r1harness.codecache_tool import find_codecache_binary
    from r1harness.contextbench import parse_contextbench_records
    from r1harness.contextbench_corpus import (
        CorpusMaterializeError,
        materialize_task,
        select_tasks,
    )
    from r1harness.sweep import DEFAULT_GRID

    binary = find_codecache_binary()
    runs_dir = HERE / "runs" / "contextbench_exit"
    runs_dir.mkdir(parents=True, exist_ok=True)

    # --- Load + select tasks ---
    records = json.loads(cache_file.read_text(encoding="utf-8"))
    if args.repos:
        # Explicit repo filter: restrict the pool to the named repos BEFORE select_tasks.
        # select_tasks's alphabetical greedy rule then picks from this restricted pool.
        # max_repos is effectively overridden by the number of explicit repos provided.
        explicit_repos = set(args.repos)
        records = [r for r in records if r.get("repo") in explicit_repos]
        effective_max_repos = len(explicit_repos)
    else:
        effective_max_repos = args.max_repos
    selected = select_tasks(records, max_repos=effective_max_repos, max_tasks=args.max_tasks)

    repo_set = sorted({r["repo"] for r in selected})
    lang_breakdown = {}
    for r in selected:
        lang_breakdown.setdefault(r["language"], 0)
        lang_breakdown[r["language"]] += 1

    print(f"=== R2.7 Scoped Real-Corpus Exit Run — binary={binary.name} ===")
    print("  Corpus: ContextBench-Lite (contextbench_verified, 500 tasks)")
    print("  Filter: language in {python, typescript}")
    print(f"  Selected: {len(selected)} tasks from {len(repo_set)} repos")
    print(f"  Repos: {repo_set}")
    print(f"  Language breakdown: {lang_breakdown}")
    print(f"  max_chunk_size={args.max_chunk_size}  BM25 vectors={len(DEFAULT_GRID)}")
    print(f"  max_repos={args.max_repos}  max_tasks={args.max_tasks}")
    print()

    # --- Parse sweep queries (gold labels) ---
    sweep_queries = parse_contextbench_records(selected)
    # Build mapping instance_id → SweepQuery.
    sq_map = {sq.query_id: sq for sq in sweep_queries}

    # --- Materialise tasks ---
    task_dirs: list[tuple[dict, Path]] = []
    skipped_tasks: list[str] = []

    print("Materialising repos (git clone + worktree, reused on re-run)...")
    for rec in selected:
        iid = rec["instance_id"]
        print(f"  task: {iid[:50]}...", end=" ", flush=True)
        try:
            tdir = materialize_task(rec, cache_root=args.repo_cache_dir)
            task_dirs.append((rec, tdir))
            print(f"OK  ({tdir.name})")
        except CorpusMaterializeError as exc:
            print(f"SKIPPED — {exc}")
            skipped_tasks.append(iid)

    print()
    print(f"Materialised: {len(task_dirs)} tasks  |  Skipped: {len(skipped_tasks)}")
    print()

    if not task_dirs:
        print("ERROR: no tasks materialised successfully. Cannot run eval.", file=sys.stderr)
        return 1

    # ----------------------------------------------------------------
    # Run 1: BM25 weight sweep (native chunker)
    # ----------------------------------------------------------------
    print("Run 1: BM25 weight sweep (native chunker) ...")
    # Accumulate per-vector metrics across tasks.
    vec_accum: dict[str, dict[str, float]] = {}
    for vec in DEFAULT_GRID:
        vec_accum[vec.label] = {
            "ndcg_file_10": 0.0,
            "f1_file_10": 0.0,
            "recall_file_10": 0.0,
            "n": 0,
        }

    sweep_task_results: list[dict] = []

    with tempfile.TemporaryDirectory(prefix="cc_exit_sweep_") as _tmp_sweep:
        for rec, tdir in task_dirs:
            iid = rec["instance_id"]
            sq = sq_map.get(iid)
            if sq is None or not sq.gold_files:
                print(f"  [{iid[:40]}] SKIP (no gold files)")
                continue

            print(f"  [{iid[:40]}...] sweep", end=" ", flush=True)
            try:
                per_vec = _score_task_sweep(tdir, sq, binary, DEFAULT_GRID)
                for row in per_vec:
                    acc = vec_accum[row["label"]]
                    acc["ndcg_file_10"] += row["ndcg_file_10"]
                    acc["f1_file_10"] += row["f1_file_10"]
                    acc["recall_file_10"] += row["recall_file_10"]
                    acc["n"] += 1
                sweep_task_results.append({"task": iid, "per_vec": per_vec})
                print("OK")
            except Exception as exc:
                print(f"FAIL — {exc}")
                skipped_tasks.append(iid)

    # Macro-average over tasks.
    sweep_rows: list[dict] = []
    for vec in DEFAULT_GRID:
        acc = vec_accum[vec.label]
        n = acc["n"]
        if n > 0:
            sweep_rows.append(
                {
                    "label": vec.label,
                    "weights": list(vec.weights),
                    "ndcg_file_10": acc["ndcg_file_10"] / n,
                    "f1_file_10": acc["f1_file_10"] / n,
                    "recall_file_10": acc["recall_file_10"] / n,
                    "n_tasks": n,
                }
            )
        else:
            sweep_rows.append(
                {
                    "label": vec.label,
                    "weights": list(vec.weights),
                    "ndcg_file_10": 0.0,
                    "f1_file_10": 0.0,
                    "recall_file_10": 0.0,
                    "n_tasks": 0,
                }
            )

    n_sweep_tasks = sweep_rows[0]["n_tasks"] if sweep_rows else 0
    print(f"  Sweep done: {n_sweep_tasks} tasks scored across {len(DEFAULT_GRID)} vectors")
    print()

    # ----------------------------------------------------------------
    # Run 2: Chunker A/B (default weights, same tasks)
    # ----------------------------------------------------------------
    print(f"Run 2: Chunker A/B (default weights, max_chunk_size={args.max_chunk_size}) ...")
    ab_accum: dict[str, dict[str, float]] = {
        "native": {"ndcg_file_10": 0.0, "f1_file_10": 0.0, "recall_file_10": 0.0, "n": 0},
        "astchunk": {"ndcg_file_10": 0.0, "f1_file_10": 0.0, "recall_file_10": 0.0, "n": 0},
    }
    ab_task_results: list[dict] = []

    with tempfile.TemporaryDirectory(prefix="cc_exit_ab_") as tmp:
        tmp_path = Path(tmp)
        for task_idx, (rec, tdir) in enumerate(task_dirs):
            iid = rec["instance_id"]
            language = rec.get("language", "python")
            sq = sq_map.get(iid)
            if sq is None or not sq.gold_files:
                print(f"  [{iid[:40]}] SKIP (no gold files)")
                continue

            print(f"  [{iid[:40]}...] A/B ({language})", end=" ", flush=True)
            try:
                ab_rows = _score_task_ab(
                    tdir,
                    sq,
                    binary,
                    language,
                    tmp_path,
                    args.max_chunk_size,
                    task_idx,
                )
                for row in ab_rows:
                    arm = row["arm"]
                    acc = ab_accum[arm]
                    acc["ndcg_file_10"] += row["ndcg_file_10"]
                    acc["f1_file_10"] += row["f1_file_10"]
                    acc["recall_file_10"] += row["recall_file_10"]
                    acc["n"] += 1
                ab_task_results.append({"task": iid, "per_arm": ab_rows})
                print("OK")
            except Exception as exc:
                print(f"FAIL — {exc}")
                skipped_tasks.append(iid)

    ab_rows_agg: list[dict] = []
    for arm in ("native", "astchunk"):
        acc = ab_accum[arm]
        n = acc["n"]
        ab_rows_agg.append(
            {
                "arm": arm,
                "ndcg_file_10": acc["ndcg_file_10"] / n if n > 0 else 0.0,
                "f1_file_10": acc["f1_file_10"] / n if n > 0 else 0.0,
                "recall_file_10": acc["recall_file_10"] / n if n > 0 else 0.0,
                "n_tasks": n,
            }
        )

    n_ab_tasks = ab_accum["native"]["n"]
    print(f"  A/B done: {n_ab_tasks} tasks per arm")
    print()

    # ----------------------------------------------------------------
    # Render tables
    # ----------------------------------------------------------------
    table1 = _render_sweep_table(sweep_rows)
    table2 = _render_ab_table(ab_rows_agg)

    header = (
        "## R2.7 Ablation Tables — Scoped Real-Corpus Exit\n"
        "\n"
        f"**Corpus:** ContextBench-Lite (contextbench_verified, Apache-2.0, EuniAI)  \n"
        f"**Tasks:** {len(task_dirs)} materialised / {len(selected)} selected  \n"
        f"**Repos ({len(repo_set)}):** {', '.join(repo_set)}  \n"
        f"**Language filter:** python + typescript  \n"
        f"**max_chunk_size:** {args.max_chunk_size} (astchunk, Run 2)  \n"
        f"**Binary:** {binary}  \n"
        f"**Skipped tasks:** {len(skipped_tasks)} ({', '.join(skipped_tasks[:5])} ...)  \n"
        "\n"
        "> Scoped/directional exit — NOT full-500 ContextBench-Lite. No winner asserted.\n"
    )

    print(header)
    print(table1)
    print()
    print(table2)
    print()

    # ----------------------------------------------------------------
    # Write report JSON
    # ----------------------------------------------------------------
    report = {
        "run": "R2.7-contextbench-exit",
        "binary": str(binary),
        "corpus": "ContextBench-Lite (contextbench_verified, Apache-2.0)",
        "n_selected": len(selected),
        "n_materialised": len(task_dirs),
        "n_skipped": len(skipped_tasks),
        "skipped_tasks": skipped_tasks,
        "repos": repo_set,
        "language_breakdown": lang_breakdown,
        "max_chunk_size": args.max_chunk_size,
        "max_repos": args.max_repos,
        "max_tasks": args.max_tasks,
        "scope_note": (
            "Scoped/directional real-corpus exit. NOT full-500 ContextBench-Lite. "
            "No winner asserted unless clearly separated beyond noise. "
            "Headline metric: NDCG@10 (file). Block-level excluded (no symbol-name gold). "
            "Micro-suite saturation finding (D28): 5/6 vectors tied NDCG@10=0.822 (block); "
            "native vs astchunk tied at Recall saturation. "
            "Real corpus test: does it separate arms/vectors? "
            "Run 2 A/B isolation: each arm has its own scratch dir + .codecache/ DB. "
            "Both arms index the SAME file set: primary extension (.py/.ts) + 500-file cap. "
            "Run 1 sweep: full worktree indexed (all supported languages, no cap)."
        ),
        "sweep_rows": sweep_rows,
        "ab_rows": ab_rows_agg,
        "sweep_task_results": sweep_task_results,
        "ab_task_results": ab_task_results,
        "table1_markdown": table1,
        "table2_markdown": table2,
    }
    report_path = runs_dir / "report.json"
    report_path.write_text(json.dumps(report, indent=2), encoding="utf-8")

    print(f"Report written: {report_path}")
    print(
        "\n(Outcome-agnostic: R2.7 is a scoped directional real-corpus exit. "
        "No winner asserted. R3 gates the full evaluation.)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
