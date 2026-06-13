"""Trajectory log schema + Layer-1/Layer-2 metric extraction.

A *trajectory* is the per-turn record of one agent run on one task in one arm.
mini-SWE-agent already keeps a linear message history; this module defines the
thin JSONL sidecar we add so retrieval metrics are computable *from the logs*
(R1 exit criterion) without re-running the agent.

Wire format: one JSON object per line (JSONL).
- Line 0 is a **meta** record: ``{"record": "meta", arm, task_id, model, ...}``.
- Lines 1..N are **turn** records (:class:`TurnRecord`): the action the agent
  took, the per-turn token usage, and which gold-comparable items (files /
  ``(file, symbol)`` blocks) that action surfaced into the agent's context.

Layer-1 (retrieval quality) consumes the cumulative surfaced sets via
:func:`surfaced_lists`; Layer-2 (token & turn economy, project_overview §5.2)
consumes cumulative tokens and turns via :func:`tokens_to_coverage` /
:func:`turns_to_coverage`.
"""

from __future__ import annotations

import json
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Iterator


@dataclass
class TurnRecord:
    """One agent turn.

    ``files_surfaced`` / ``blocks_surfaced`` are the gold-comparable items this
    turn brought into the agent's context — e.g. files ``cat``/``grep``-ed, or
    the ``(file, symbol)`` pairs returned by ``codecache_search``. They are what
    the Layer-1 scorer measures against gold, in turn order.
    """

    turn: int
    action: str
    action_kind: str  # "bash" | "codecache_query" | "oneshot_inject" | "submit" | "other"
    observation: str
    prompt_tokens: int = 0
    completion_tokens: int = 0
    files_surfaced: list[str] = field(default_factory=list)
    blocks_surfaced: list[tuple[str, str]] = field(default_factory=list)
    wall_ms: float = 0.0
    record: str = "turn"


@dataclass
class TrajectoryMeta:
    """Run-level header: enough to reproduce the run and label every figure."""

    arm: str
    task_id: str
    model: str
    temperature: float
    corpus_id: str
    query: str
    record: str = "meta"


class TrajectoryLogger:
    """Append-only JSONL writer for one (arm, task) run."""

    def __init__(self, path: Path, meta: TrajectoryMeta) -> None:
        self.path = Path(path)
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self._turn = 0
        with self.path.open("w", encoding="utf-8") as fh:
            fh.write(json.dumps(asdict(meta), ensure_ascii=False) + "\n")

    def log_turn(
        self,
        action: str,
        action_kind: str,
        observation: str,
        *,
        prompt_tokens: int = 0,
        completion_tokens: int = 0,
        files_surfaced: list[str] | None = None,
        blocks_surfaced: list[tuple[str, str]] | None = None,
        wall_ms: float = 0.0,
    ) -> TurnRecord:
        self._turn += 1
        rec = TurnRecord(
            turn=self._turn,
            action=action,
            action_kind=action_kind,
            observation=observation,
            prompt_tokens=prompt_tokens,
            completion_tokens=completion_tokens,
            files_surfaced=list(files_surfaced or []),
            blocks_surfaced=[tuple(b) for b in (blocks_surfaced or [])],
            wall_ms=wall_ms,
        )
        d = asdict(rec)
        # JSON has no tuple type — store blocks as 2-element lists; read_trajectory restores tuples.
        d["blocks_surfaced"] = [list(b) for b in rec.blocks_surfaced]
        with self.path.open("a", encoding="utf-8") as fh:
            fh.write(json.dumps(d, ensure_ascii=False) + "\n")
        return rec


def read_trajectory(path: Path) -> tuple[TrajectoryMeta, list[TurnRecord]]:
    """Load a JSONL trajectory back into its meta header + ordered turns."""
    meta: TrajectoryMeta | None = None
    turns: list[TurnRecord] = []
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line:
            continue
        obj = json.loads(line)
        if obj.get("record") == "meta":
            obj.pop("record", None)
            meta = TrajectoryMeta(**obj, record="meta")
        else:
            obj.pop("record", None)
            obj["blocks_surfaced"] = [tuple(b) for b in obj.get("blocks_surfaced", [])]
            turns.append(TurnRecord(**obj, record="turn"))
    if meta is None:
        raise ValueError(f"trajectory {path} has no meta record on line 0")
    turns.sort(key=lambda t: t.turn)
    return meta, turns


def surfaced_lists(turns: list[TurnRecord]) -> tuple[list[str], list[tuple[str, str]]]:
    """Cumulative ordered (files, blocks) surfaced across the whole trajectory.

    Concatenates per-turn surfaced items in turn order — the ordered "retrieved"
    lists the Layer-1 scorer ranks. File-level dedup is left to the scorer
    (:func:`r1harness.scorer.dedup_first`) so block-level order is preserved.
    """
    files: list[str] = []
    blocks: list[tuple[str, str]] = []
    for t in turns:
        files.extend(t.files_surfaced)
        blocks.extend(t.blocks_surfaced)
    return files, blocks


def _coverage_reached(blocks_so_far: set[tuple[str, str]], gold_blocks: set[tuple[str, str]], threshold: float) -> bool:
    if not gold_blocks:
        return True
    recall = len(blocks_so_far & gold_blocks) / len(gold_blocks)
    return recall >= threshold


def turns_to_coverage(
    turns: list[TurnRecord],
    gold_blocks: set[tuple[str, str]],
    threshold: float = 1.0,
) -> int | None:
    """1-based turn index at which cumulative block recall first ≥ threshold.

    ``None`` if the gold set is never covered. This is the Layer-2
    *turns-to-coverage* metric (project_overview §5.2).
    """
    seen: set[tuple[str, str]] = set()
    for t in turns:
        seen.update(t.blocks_surfaced)
        if _coverage_reached(seen, gold_blocks, threshold):
            return t.turn
    return None


def tokens_to_coverage(
    turns: list[TurnRecord],
    gold_blocks: set[tuple[str, str]],
    threshold: float = 1.0,
) -> int | None:
    """Cumulative prompt+completion tokens consumed until coverage is first reached.

    ``None`` if coverage is never reached. Layer-2 *tokens-to-correct-context*
    (the headline metric the field argues about with anecdotes — §5.2).
    """
    seen: set[tuple[str, str]] = set()
    cumulative = 0
    for t in turns:
        cumulative += t.prompt_tokens + t.completion_tokens
        seen.update(t.blocks_surfaced)
        if _coverage_reached(seen, gold_blocks, threshold):
            return cumulative
    return None


def total_tokens(turns: list[TurnRecord]) -> int:
    """Total prompt+completion tokens over the whole trajectory."""
    return sum(t.prompt_tokens + t.completion_tokens for t in turns)


def iter_jsonl(path: Path) -> Iterator[dict]:
    """Yield each JSONL record as a dict (utility for ad-hoc inspection)."""
    for line in Path(path).read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            yield json.loads(line)
