"""R2.7 — ContextBench corpus materialiser + task selector.

Builds the searchable corpus that R2.5a's ``contextbench.py`` deliberately left out.
R2.5a only maps ContextBench-Lite records → ``SweepQuery`` (query + gold labels).  R2.7
adds the two missing pieces:

  1. **Selector** (pure): given a list of in-memory ContextBench-Lite records, filter to
     py/ts tasks, cap to ≤ *max_repos* distinct repos, cap total tasks to *max_tasks*.
     The selection is **deterministic** — identical input always produces identical output.

  2. **Materialiser** (thin I/O): for each selected task, ``git clone`` once per repo into a
     gitignored cache dir and ``git checkout <base_commit>`` to produce a per-task on-disk
     tree that ``CodeCacheIndex`` can index.

Design decision: **shallow clone of the full repo** (``git clone --depth=1 <url> <dest>``)
is NOT used here because we need to check out a specific non-HEAD commit.  Instead:
  - ``git clone --no-checkout <url> <dest>`` (full object graph, no working tree) is
    performed ONCE per repo into ``<cache_root>/<owner>__<name>/``;
  - ``git worktree add <task_dir> <base_commit>`` is used to produce an isolated,
    per-task working tree without a full re-clone.
This approach re-uses one clone for all tasks from the same repo and avoids touching the
shared clone's working tree.

Selection rule (documented contract — tests mirror this):
  1. Filter: keep only records where ``language ∈ {"python", "typescript"}``.
  2. Stable sort by ``(repo, instance_id)`` — ties broken by ``instance_id`` lexicographically.
  3. Greedy repo admission: iterate sorted records in order; admit a record's repo to the
     active repo-set if (a) the repo is already in the set, OR (b) |repo-set| < *max_repos*.
  4. Task admission: admit the record to the output if its repo was admitted AND
     |output| < *max_tasks*.
  5. Return the admitted records in their sorted order (deterministic).

Path layout:
  - Clone cache: ``<cache_root>/<owner>__<name>/``          (bare/no-checkout clone)
  - Task worktree: ``<cache_root>/worktrees/<owner>__<name>/<commit[:8]>/``

Error handling:
  - ``git clone`` or ``git worktree add`` failure → raises ``CorpusMaterializeError``
    (a typed exception); callers may catch and skip-with-log.
  - Never crashes with an unhandled ``CalledProcessError`` or ``FileNotFoundError``.

Pure helpers (``repo_cache_dir``, ``clone_argv``, ``checkout_argv``, ``task_repo_dir``)
are tested independently without invoking real ``git`` — see ``test_contextbench_corpus.py``.
"""

from __future__ import annotations

import subprocess
from pathlib import Path

# Languages that CodeCache and astchunk both support (Go excluded: no astchunk grammar).
_SUPPORTED_LANGUAGES: frozenset[str] = frozenset({"python", "typescript"})


# ---------------------------------------------------------------------------
# Typed error
# ---------------------------------------------------------------------------


class CorpusMaterializeError(RuntimeError):
    """Raised when git clone or git worktree add fails for a ContextBench task.

    Callers should catch this error and log + skip the failing task rather than
    propagating it (graceful degradation for the scoped real-corpus run).
    """


# ---------------------------------------------------------------------------
# Pure helpers — tested without invoking real git
# ---------------------------------------------------------------------------


def repo_cache_dir(repo_url: str, cache_root: Path) -> Path:
    """Derive the on-disk cache directory for a repo from its clone URL.

    Rule: strip the ``.git`` suffix (if any), take the last two path segments
    (``<owner>/<name>``), and join with ``__`` (double underscore) to form a
    single directory name under *cache_root*.

    Examples::

        repo_cache_dir("https://github.com/astropy/astropy.git", Path("/cache"))
        → Path("/cache/astropy__astropy")

        repo_cache_dir("https://github.com/vuejs/core", Path("/cache"))
        → Path("/cache/vuejs__core")
    """
    # Strip trailing .git
    url = repo_url.rstrip("/")
    if url.endswith(".git"):
        url = url[: -len(".git")]
    # Last two segments: owner/name
    parts = url.split("/")
    owner = parts[-2] if len(parts) >= 2 else "unknown"
    name = parts[-1] if len(parts) >= 1 else "repo"
    dir_name = f"{owner}__{name}"
    return Path(cache_root) / dir_name


def clone_argv(repo_url: str, dest_dir: Path) -> list[str]:
    """Return the ``git clone`` argv for a no-checkout clone of *repo_url* → *dest_dir*.

    ``--no-checkout`` ensures no working tree is created in the shared clone dir;
    tasks use ``git worktree add`` to produce isolated working trees.

    Args:
        repo_url: The HTTPS clone URL (e.g. ``https://github.com/astropy/astropy.git``).
        dest_dir: Absolute path where the clone should land.

    Returns:
        A ``list[str]`` suitable for ``subprocess.run``.
    """
    return ["git", "clone", "--no-checkout", repo_url, str(dest_dir)]


def checkout_argv(base_commit: str) -> list[str]:
    """Return the ``git checkout`` argv for a commit hash.

    Used for a bare (non-worktree) checkout path; prefer ``worktree_add_argv``
    for the actual per-task materialisation flow which uses ``git worktree add``.

    Args:
        base_commit: The full or abbreviated commit hash to check out.

    Returns:
        A ``list[str]`` suitable for ``subprocess.run`` (run from the clone dir).
    """
    return ["git", "checkout", base_commit]


def worktree_add_argv(base_commit: str, worktree_path: Path) -> list[str]:
    """Return the ``git worktree add`` argv for a per-task isolated working tree.

    Args:
        base_commit: The commit hash to check out in the worktree.
        worktree_path: Absolute path where the worktree should be created.

    Returns:
        A ``list[str]`` suitable for ``subprocess.run`` (run from the clone dir).
    """
    return ["git", "worktree", "add", str(worktree_path), base_commit]


def task_repo_dir(cache_root: Path, repo_url: str, base_commit: str) -> Path:
    """Return the on-disk path for a per-task working tree.

    Layout: ``<cache_root>/worktrees/<owner>__<name>/<commit[:8]>/``

    This path is deterministic (same URL + commit → same path) and distinct
    across different commits for the same repo.

    Args:
        cache_root: Root cache directory (e.g. ``cache/contextbench_repos``).
        repo_url:   The HTTPS clone URL.
        base_commit: The base commit hash.

    Returns:
        The ``Path`` for the per-task worktree directory (not yet created).
    """
    clone_dir_name = repo_cache_dir(repo_url, cache_root).name  # e.g. "astropy__astropy"
    # Use first 8 chars of commit as the sub-dir (enough for disambiguation).
    commit_prefix = base_commit[:8]
    return Path(cache_root) / "worktrees" / clone_dir_name / commit_prefix


# ---------------------------------------------------------------------------
# Selector (pure)
# ---------------------------------------------------------------------------


def select_tasks(
    records: list[dict],
    *,
    max_repos: int = 3,
    max_tasks: int = 15,
    languages: frozenset[str] = _SUPPORTED_LANGUAGES,
) -> list[dict]:
    """Select a deterministic, scoped subset of ContextBench-Lite tasks.

    Selection rule (documented; tests mirror this contract):
      1. Filter: keep only records where ``language ∈ languages``.
      2. Stable sort by ``(repo, instance_id)`` (both ascending, lexicographic).
      3. Greedy repo admission: iterate; admit a repo if already seen OR |seen| < max_repos.
      4. Task admission: admit the record if its repo was admitted AND |output| < max_tasks.

    Args:
        records:   In-memory ContextBench-Lite records (list of dicts).
        max_repos: Maximum distinct repos to include (default 3).
        max_tasks: Maximum total tasks to return (default 15).
        languages: Languages to admit (default: python + typescript).

    Returns:
        A list of admitted records in (repo, instance_id) sorted order.
        Empty list if no records pass the language filter.
    """
    # Step 1: language filter.
    filtered = [r for r in records if r.get("language") in languages]

    # Step 2: stable sort by (repo, instance_id).
    filtered.sort(key=lambda r: (r.get("repo", ""), r.get("instance_id", "")))

    # Steps 3 + 4: greedy admission.
    admitted_repos: set[str] = set()
    output: list[dict] = []
    for rec in filtered:
        if len(output) >= max_tasks:
            break
        repo = rec.get("repo", "")
        if repo not in admitted_repos:
            if len(admitted_repos) >= max_repos:
                continue  # repo cap exhausted; skip records from new repos
            admitted_repos.add(repo)
        output.append(rec)

    return output


# ---------------------------------------------------------------------------
# Materialiser (thin I/O)
# ---------------------------------------------------------------------------


def _run_git(args: list[str], cwd: Path | None, error_prefix: str) -> None:
    """Run a git subcommand; raise CorpusMaterializeError on non-zero exit."""
    result = subprocess.run(
        args,
        cwd=str(cwd) if cwd else None,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        raise CorpusMaterializeError(f"{error_prefix} (exit {result.returncode}): {result.stderr.strip()}")


def ensure_clone(repo_url: str, clone_dir: Path) -> None:
    """Ensure a bare/no-checkout clone of *repo_url* exists at *clone_dir*.

    If *clone_dir* already exists (from a previous run), this is a no-op.
    On network or auth failure, raises ``CorpusMaterializeError``.

    Args:
        repo_url:  The HTTPS clone URL.
        clone_dir: Where to clone (``<cache_root>/<owner>__<name>``).
    """
    if clone_dir.exists():
        # Already cloned; reuse.
        return
    clone_dir.parent.mkdir(parents=True, exist_ok=True)
    _run_git(
        clone_argv(repo_url, clone_dir),
        cwd=None,
        error_prefix=f"git clone {repo_url!r} failed",
    )


def materialize_task(
    record: dict,
    *,
    cache_root: Path,
) -> Path:
    """Materialise one ContextBench-Lite task as an on-disk working tree.

    Flow:
      1. Derive the shared clone dir from ``record["repo_url"]``.
      2. ``ensure_clone`` (no-op if already cloned; clone if missing).
      3. Derive the per-task worktree path from ``record["base_commit"]``.
      4. If the worktree path already exists, return it (idempotent/cheap re-run).
      5. ``git worktree add <worktree_path> <base_commit>`` to produce the working tree.

    Args:
        record:     A ContextBench-Lite dict with keys ``repo_url`` and ``base_commit``.
        cache_root: The root cache directory (``cache/contextbench_repos``).

    Returns:
        The ``Path`` of the materialised per-task working tree.

    Raises:
        CorpusMaterializeError: if the git clone or worktree-add fails.
    """
    repo_url: str = record["repo_url"]
    base_commit: str = record["base_commit"]

    clone_dir = repo_cache_dir(repo_url, cache_root)
    worktree_path = task_repo_dir(cache_root, repo_url, base_commit)

    # Step 2: clone once.
    ensure_clone(repo_url, clone_dir)

    # Step 4: idempotent — worktree already exists.
    if worktree_path.exists():
        return worktree_path

    # Step 5: create the per-task worktree.
    worktree_path.parent.mkdir(parents=True, exist_ok=True)
    _run_git(
        worktree_add_argv(base_commit, worktree_path),
        cwd=clone_dir,
        error_prefix=f"git worktree add {worktree_path} {base_commit!r} failed",
    )
    return worktree_path
