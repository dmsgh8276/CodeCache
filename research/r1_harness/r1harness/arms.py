"""Experiment arms — the retrieval *interface* swapped under a fixed agent.

R1 wires only the three arms its exit criterion names (A0/A1/A4); A2/A3/A5
(project_overview §5.3) are explicitly deferred to R2/R3 (D22). Each arm is a
pure declaration consumed by ``runner.py`` to configure mini-SWE-agent — it
changes *only* the tool surface + the prompt that documents it, never the
model, temperature, task, or gold (the same-agent ablation, §4.2).
"""

from __future__ import annotations

from dataclasses import dataclass, field


@dataclass(frozen=True)
class Arm:
    name: str
    description: str
    #: Whether the agent may call ``codecache query`` as a tool in the loop.
    codecache_in_loop: bool
    #: Whether top-k from the index is injected once up front with no loop tool access (A4).
    oneshot_inject: bool
    #: Appended to the agent system prompt to document the available retrieval surface.
    prompt_addendum: str = ""
    #: Free-form notes / provenance for the run record.
    notes: str = ""


A0 = Arm(
    name="A0",
    description="grep/glob/read only — Claude Code-style agentic search (control).",
    codecache_in_loop=False,
    oneshot_inject=False,
    prompt_addendum=(
        "You may explore the repository only with standard shell tools "
        "(grep, ls/find, cat/sed). There is no code index available."
    ),
    notes="project_overview §5.3 arm A0",
)

A1 = Arm(
    name="A1",
    description="A0 + codecache_search (AST+BM25, plain) — index-as-tool, no enrichment toggle.",
    codecache_in_loop=True,
    oneshot_inject=False,
    prompt_addendum=(
        "In addition to standard shell tools, a code index is available. Run\n"
        "    codecache query \"<your search terms>\" --format json\n"
        "to retrieve the most relevant symbols (name, file, line range, body) "
        "ranked by relevance, instead of grepping blindly. Prefer it for "
        "locating where functionality lives."
    ),
    notes="project_overview §5.3 arm A1",
)

A4 = Arm(
    name="A4",
    description="One-shot top-k injection from the index (no loop access) — classic RAG baseline.",
    codecache_in_loop=False,
    oneshot_inject=True,
    prompt_addendum=(
        "The most relevant code for this task has been retrieved for you and is "
        "included below. You may also use standard shell tools to confirm details."
    ),
    notes="project_overview §5.3 arm A4 (RQ3 one-shot vs in-loop)",
)

#: Arms wired in R1. A2/A3/A5 deferred to R2/R3 (D22).
R1_ARMS: dict[str, Arm] = {a.name: a for a in (A0, A1, A4)}


@dataclass(frozen=True)
class Task:
    """One gold-labeled retrieval task over a named micro-suite corpus."""

    task_id: str
    corpus_id: str
    query: str
    query_type: str  # "keyword" | "semantic"
    gold_files: set[str] = field(default_factory=set)
    gold_blocks: set[tuple[str, str]] = field(default_factory=set)

    @staticmethod
    def from_dict(d: dict) -> "Task":
        return Task(
            task_id=d["task_id"],
            corpus_id=d["corpus_id"],
            query=d["query"],
            query_type=d.get("query_type", "keyword"),
            gold_files=set(d.get("gold_files", [])),
            gold_blocks={(b["file_path"], b["symbol_name"]) for b in d.get("gold_blocks", [])},
        )
