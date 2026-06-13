"""R1 eval harness for CodeCache (research track — ROADMAP D22).

A controlled, same-agent comparison of retrieval *interfaces* (ROADMAP D12,
project_overview.md §4.2) — arms A0 (grep-only), A1 (+ ``codecache_search``),
A4 (one-shot top-k injection) — built by forking mini-SWE-agent and shelling
out to the Rust ``codecache`` binary over a process boundary.

This package is **research-only and out-of-crate**: it ships in no release
artifact, adds no Rust dependency, and does not touch ``Cargo.toml`` (extends
the D17 "test-only, keep the runtime lean" precedent to "research-only").

Layer-1 scoring (:mod:`r1harness.scorer`) is a Python port of the M10.2 scoring
protocol (``tests/retrieval_quality.rs``); the protocol is the contract, so R2
can swap in the real ContextBench corpus with the scorer unchanged (D21).
"""

__all__ = ["scorer", "trajectory", "corpus", "codecache_tool", "arms"]
