"""Unit tests for the Layer-1 scorer.

These MIRROR the hand-computed metric tests in ``tests/retrieval_quality.rs``
(``metric_unit_tests``) so the Python port stays behaviourally identical to the
Rust source of truth. Each expected value is hand-derived, not regenerated.
"""

import math

from r1harness import scorer
from r1harness.scorer import (
    f1_at_k,
    macro_average,
    ndcg_at_k,
    precision_at_k,
    recall_at_k,
    score_query,
)

CLOSE = 1e-10


# ── recall_at_k ──────────────────────────────────────────────────────────────


def test_recall_k1_perfect_hit():
    assert recall_at_k(["a", "b", "c"], {"a"}, 1) == 1.0


def test_recall_k1_miss():
    assert recall_at_k(["b", "a", "c"], {"a"}, 1) == 0.0


def test_recall_partial_at_k():
    assert abs(recall_at_k(["b", "a", "d", "c", "e"], {"a", "c"}, 3) - 0.5) < CLOSE


def test_recall_full_at_k():
    assert recall_at_k(["a", "b", "c", "d"], {"a", "b"}, 3) == 1.0


def test_recall_empty_gold_is_one():
    assert recall_at_k(["a", "b"], set(), 5) == 1.0


def test_recall_k_larger_than_retrieved():
    assert abs(recall_at_k(["a", "b"], {"a", "c"}, 5) - 0.5) < CLOSE


# ── precision_at_k ─────────────────────────────────────────────────────────────


def test_precision_k1_perfect():
    assert precision_at_k(["a", "b", "c"], {"a"}, 1) == 1.0


def test_precision_k1_miss():
    assert precision_at_k(["b", "a", "c"], {"a"}, 1) == 0.0


def test_precision_at_k_partial():
    assert abs(precision_at_k(["a", "b", "c", "d"], {"a", "c"}, 3) - 2.0 / 3.0) < CLOSE


def test_precision_k_larger_than_retrieved():
    assert abs(precision_at_k(["a", "b"], {"a"}, 5) - 0.5) < CLOSE


def test_precision_empty_retrieved_is_zero():
    assert precision_at_k([], {"a"}, 5) == 0.0


# ── f1_at_k ────────────────────────────────────────────────────────────────────


def test_f1_perfect():
    assert abs(f1_at_k(["a", "b"], {"a", "b"}, 5) - 1.0) < CLOSE


def test_f1_zero_precision_zero_recall():
    assert f1_at_k(["x", "y"], {"a", "b"}, 5) == 0.0


def test_f1_known_hand_computed():
    # P@3 = 2/3, R@3 = 1.0 → F1 = (4/3)/(5/3) = 0.8
    p = 2.0 / 3.0
    r = 1.0
    expected = 2.0 * p * r / (p + r)
    assert abs(f1_at_k(["a", "b", "c", "d"], {"a", "b"}, 3) - expected) < CLOSE
    assert abs(expected - 0.8) < CLOSE


# ── ndcg_at_k ────────────────────────────────────────────────────────────────
# DCG@k = Σ_{i=1..k} rel_i / log2(i+1) (binary rel, 1-based rank); IDCG@k = ideal
# (all |gold| relevant first); NDCG@k = DCG/IDCG. Empty gold ⇒ 1.0 (matches recall).


def test_ndcg_k1_perfect_hit():
    assert ndcg_at_k(["a", "b", "c"], {"a"}, 1) == 1.0


def test_ndcg_gold_at_rank2():
    # DCG = 1/log2(3); IDCG = 1/log2(2) = 1 → NDCG = 1/log2(3)
    expected = 1.0 / math.log2(3)
    assert abs(ndcg_at_k(["b", "a", "c"], {"a"}, 10) - expected) < CLOSE


def test_ndcg_two_gold_interleaved():
    # retrieved [a, x, b], gold {a, b}: DCG = 1/log2(2) + 1/log2(4); IDCG = 1/log2(2) + 1/log2(3)
    dcg = 1.0 / math.log2(2) + 1.0 / math.log2(4)
    idcg = 1.0 / math.log2(2) + 1.0 / math.log2(3)
    assert abs(ndcg_at_k(["a", "x", "b"], {"a", "b"}, 10) - dcg / idcg) < CLOSE


def test_ndcg_perfect_when_gold_ranked_first():
    # both gold at ranks 1,2 → DCG == IDCG → NDCG = 1.0
    assert abs(ndcg_at_k(["a", "b", "c"], {"a", "b"}, 10) - 1.0) < CLOSE


def test_ndcg_empty_gold_is_one():
    assert ndcg_at_k(["a", "b"], set(), 5) == 1.0


def test_ndcg_empty_retrieved_is_zero():
    assert ndcg_at_k([], {"a"}, 5) == 0.0


def test_ndcg_gold_beyond_k_is_zero():
    # 'a' is at rank 6; at k=5 it is outside the top-k → NDCG = 0.0
    assert ndcg_at_k(["b", "c", "d", "e", "f", "a"], {"a"}, 5) == 0.0


def test_ndcg_gold_at_rank6_within_k10():
    # 'a' at rank 6 → DCG = 1/log2(7); IDCG = 1 → NDCG = 1/log2(7)
    expected = 1.0 / math.log2(7)
    assert abs(ndcg_at_k(["b", "c", "d", "e", "f", "a"], {"a"}, 10) - expected) < CLOSE


# ── dedup_first ──────────────────────────────────────────────────────────────


def test_dedup_first_keeps_order():
    assert scorer.dedup_first(["a", "b", "a", "c", "b"]) == ["a", "b", "c"]


# ── score_query + macro_average (integration of the metric fns) ────────────────


def test_score_query_file_and_block():
    retrieved_files = ["src/auth/authenticate.py", "src/auth/session.py"]
    retrieved_blocks = [
        ("src/auth/authenticate.py", "authenticate_user"),
        ("src/auth/session.py", "generate_session_token"),
    ]
    gold_files = {"src/auth/authenticate.py"}
    gold_blocks = {("src/auth/authenticate.py", "authenticate_user")}
    metrics = score_query(retrieved_files, retrieved_blocks, gold_files, gold_blocks, k_values=[1, 5, 10])
    at1 = next(m for m in metrics if m.k == 1)
    # gold file is rank 1 → recall@1 file = 1.0; precision@1 file = 1/1 = 1.0
    assert at1.recall_file == 1.0
    assert at1.precision_file == 1.0
    # block gold is rank 1 → recall@1 block = 1.0
    assert at1.recall_block == 1.0


def test_macro_average_means_across_queries():
    # Query 1: recall_file@1 = 1.0 ; Query 2: recall_file@1 = 0.0 → mean 0.5
    q1 = score_query(["a"], [("a", "f")], {"a"}, {("a", "f")}, k_values=[1])
    q2 = score_query(["b"], [("b", "g")], {"a"}, {("a", "f")}, k_values=[1])
    avg = macro_average([q1, q2], k_values=[1])
    assert math.isclose(avg[1].recall_file, 0.5)


def test_macro_average_empty_is_zero():
    avg = macro_average([], k_values=[1, 5, 10])
    assert avg[10].recall_file == 0.0
    assert avg[10].f1_block == 0.0


def test_score_query_carries_ndcg():
    metrics = score_query(["a", "b"], [("a", "f")], {"a"}, {("a", "f")}, k_values=[1, 10])
    at1 = next(m for m in metrics if m.k == 1)
    # gold file + block both at rank 1 → NDCG@1 = 1.0 at both granularities
    assert at1.ndcg_file == 1.0
    assert at1.ndcg_block == 1.0


def test_macro_average_includes_ndcg():
    # Query 1: ndcg_file@1 = 1.0 (gold at rank 1); Query 2: ndcg_file@1 = 0.0 (miss) → mean 0.5
    q1 = score_query(["a"], [("a", "f")], {"a"}, {("a", "f")}, k_values=[1])
    q2 = score_query(["b"], [("b", "g")], {"a"}, {("a", "f")}, k_values=[1])
    avg = macro_average([q1, q2], k_values=[1])
    assert math.isclose(avg[1].ndcg_file, 0.5)
