"""Layer-1 retrieval-quality scorer — Python port of the M10.2 protocol.

This re-implements, *verbatim in semantics*, the scoring protocol pinned by
``tests/retrieval_quality.rs`` and documented in
``tests/fixtures/retrieval_quality/micro_suite.json`` → ``scoring_protocol``:

- **Recall@k**    = |G ∩ R_k| / |G|        (empty gold ⇒ 1.0)
- **Precision@k** = |G ∩ R_k| / min(k, |R|)  (short lists not penalised; 0 if |R|=0)
- **F1@k**        = 2·P·R / (P + R)          (0.0 when P + R = 0)

scored at two granularities:

- **file**  — items are ``file_path`` strings; gold is ``gold_files``.
- **block** — items are ``(file_path, symbol_name)`` pairs; gold is ``gold_blocks``.

``k`` values: {1, 5, 10}. Macro-average = mean of per-query metrics at each k.

The Rust scorer (``retrieval_quality.rs``) is the source of truth; the unit
tests in ``tests/test_scorer.py`` mirror its hand-computed metric tests so the
two implementations stay byte-for-byte equivalent in behaviour (D21: "R2 swaps
in the real corpus, scorer unchanged").
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Hashable, Iterable, Sequence

K_VALUES: tuple[int, ...] = (1, 5, 10)


def recall_at_k(retrieved: Sequence[Hashable], gold: set[Hashable], k: int) -> float:
    """Fraction of gold items appearing anywhere in the top-k retrieved list.

    Empty gold is defined as recall 1.0 (nothing to miss) — matches
    ``recall_at_k`` in ``retrieval_quality.rs``.
    """
    if not gold:
        return 1.0
    top_k = retrieved[: min(len(retrieved), k)]
    hits = sum(1 for item in top_k if item in gold)
    return hits / len(gold)


def precision_at_k(retrieved: Sequence[Hashable], gold: set[Hashable], k: int) -> float:
    """Fraction of the top-k retrieved items that are gold.

    The denominator is ``min(len(retrieved), k)`` so short lists are not
    penalised; 0.0 when nothing was retrieved.
    """
    effective_k = min(len(retrieved), k)
    if effective_k == 0:
        return 0.0
    top_k = retrieved[:effective_k]
    hits = sum(1 for item in top_k if item in gold)
    return hits / effective_k


def f1_at_k(retrieved: Sequence[Hashable], gold: set[Hashable], k: int) -> float:
    """Harmonic mean of precision@k and recall@k; 0.0 when both are 0."""
    p = precision_at_k(retrieved, gold, k)
    r = recall_at_k(retrieved, gold, k)
    if p + r == 0.0:
        return 0.0
    return 2.0 * p * r / (p + r)


@dataclass(frozen=True)
class MetricAtK:
    """The six Layer-1 metrics for one query at one k value."""

    k: int
    recall_file: float
    precision_file: float
    f1_file: float
    recall_block: float
    precision_block: float
    f1_block: float


def score_query(
    retrieved_files: Sequence[str],
    retrieved_blocks: Sequence[tuple[str, str]],
    gold_files: set[str],
    gold_blocks: set[tuple[str, str]],
    k_values: Iterable[int] = K_VALUES,
) -> list[MetricAtK]:
    """Score one query's retrieved lists against its gold sets at each k.

    ``retrieved_files`` / ``retrieved_blocks`` are the ordered, best-first lists
    the agent surfaced into context (file-level deduplicated by first
    occurrence by the caller, matching ``score_corpus`` in the Rust scorer).
    """
    out: list[MetricAtK] = []
    for k in k_values:
        out.append(
            MetricAtK(
                k=k,
                recall_file=recall_at_k(retrieved_files, gold_files, k),
                precision_file=precision_at_k(retrieved_files, gold_files, k),
                f1_file=f1_at_k(retrieved_files, gold_files, k),
                recall_block=recall_at_k(retrieved_blocks, gold_blocks, k),
                precision_block=precision_at_k(retrieved_blocks, gold_blocks, k),
                f1_block=f1_at_k(retrieved_blocks, gold_blocks, k),
            )
        )
    return out


def dedup_first(items: Iterable[Hashable]) -> list[Hashable]:
    """Stable de-duplication keeping first occurrence (file-level list helper).

    Mirrors the ``fold`` dedup in ``score_corpus`` (retrieval_quality.rs): a
    file that matches several times counts once, in first-seen rank order.
    """
    seen: set[Hashable] = set()
    out: list[Hashable] = []
    for it in items:
        if it not in seen:
            seen.add(it)
            out.append(it)
    return out


def macro_average(per_query: Sequence[list[MetricAtK]], k_values: Iterable[int] = K_VALUES) -> dict[int, MetricAtK]:
    """Macro-average each metric across queries, per k.

    Returns a ``{k: MetricAtK}`` map whose fields hold the mean over queries.
    Empty input yields all-zero metrics (no queries to average).
    """
    result: dict[int, MetricAtK] = {}
    n = len(per_query)
    for k in k_values:
        if n == 0:
            result[k] = MetricAtK(k, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
            continue
        rf = pf = ff = rb = pb = fb = 0.0
        for metrics in per_query:
            m = next(mk for mk in metrics if mk.k == k)
            rf += m.recall_file
            pf += m.precision_file
            ff += m.f1_file
            rb += m.recall_block
            pb += m.precision_block
            fb += m.f1_block
        result[k] = MetricAtK(k, rf / n, pf / n, ff / n, rb / n, pb / n, fb / n)
    return result
