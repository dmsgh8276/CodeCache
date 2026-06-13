"""Materialise a micro-suite corpus into a real on-disk repository.

mini-SWE-agent drives an agent that runs real ``bash`` commands (grep/cat) and,
in arm A1, our ``codecache`` binary — both need actual files on disk. The R1
corpus is loaded from the *same* gold fixture the Rust scorer uses
(``tests/fixtures/retrieval_quality/micro_suite.json``) so Layer-1 gold labels
line up exactly (D21: one gold source, two scorers).

A file may own several chunks (e.g. ``authenticate.py`` holds both
``authenticate_user`` and ``verify_password``); their ``chunk_text`` is
concatenated in fixture order to reconstruct the file.
"""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

# tests/fixtures/retrieval_quality/micro_suite.json relative to the repo root.
DEFAULT_MICRO_SUITE = (
    Path(__file__).resolve().parents[3]
    / "tests"
    / "fixtures"
    / "retrieval_quality"
    / "micro_suite.json"
)


@dataclass(frozen=True)
class Corpus:
    """One micro-suite corpus: its id and the ordered chunk records."""

    id: str
    chunks: list[dict]

    @property
    def files(self) -> list[str]:
        """Distinct file paths in first-seen order."""
        seen: list[str] = []
        for c in self.chunks:
            if c["file_path"] not in seen:
                seen.append(c["file_path"])
        return seen


def load_corpus(corpus_id: str, micro_suite_path: Path = DEFAULT_MICRO_SUITE) -> Corpus:
    """Load a single corpus by id from the micro-suite fixture."""
    data = json.loads(Path(micro_suite_path).read_text(encoding="utf-8"))
    for c in data["corpora"]:
        if c["id"] == corpus_id:
            return Corpus(id=c["id"], chunks=list(c["chunks"]))
    available = [c["id"] for c in data["corpora"]]
    raise KeyError(f"corpus {corpus_id!r} not found; available: {available}")


def materialize(corpus: Corpus, dest_dir: Path) -> list[Path]:
    """Write the corpus to ``dest_dir`` as a real source tree.

    Chunks sharing a ``file_path`` are concatenated in fixture order. Returns
    the list of written file paths (absolute), in first-seen order.
    """
    dest = Path(dest_dir)
    by_file: dict[str, list[str]] = {}
    for c in corpus.chunks:
        by_file.setdefault(c["file_path"], []).append(c["chunk_text"])

    written: list[Path] = []
    for rel in corpus.files:  # preserve first-seen order, deterministic
        target = dest / rel
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text("".join(by_file[rel]), encoding="utf-8")
        written.append(target)
    return written
