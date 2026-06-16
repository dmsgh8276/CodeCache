"""RED tests for R2.7 — ContextBench corpus materialiser (contextbench_corpus.py).

Pure-logic, binary-free, network-free, git-free.  Covers:

  1. selector — language filter: Go/other records are dropped; only py/ts admitted.
  2. selector — repo cap: result spans ≤ the configured max_repos distinct repos.
  3. selector — task cap + determinism: same input ⇒ same output; stable sort documented.
  4. materialiser — repo-cache-dir derivation: pure function from repo_url → Path.
  5. materialiser — clone/checkout argv: pure function producing correct git argv.
  6. materialiser — per-task working-tree path mapping: pure function, no git call.
  7. materialiser — failure handling: clone/checkout failure → typed error / skip, not crash.
  8. path alignment: gold_files (repo-relative posix) align with normalize_path'd retrieved
     paths so file-level matches are not silently zero.

The production module ``r1harness/contextbench_corpus.py`` does NOT exist yet — every
import here will fail with ImportError.  That is the correct RED state.

Public API expected (engineering lead must implement):

    from r1harness.contextbench_corpus import (
        select_tasks,          # (records, *, max_repos, max_tasks, languages) -> list[dict]
        repo_cache_dir,        # (repo_url, cache_root) -> Path
        clone_argv,            # (repo_url, dest_dir) -> list[str]
        checkout_argv,         # (base_commit) -> list[str]
        task_repo_dir,         # (cache_root, repo_url, base_commit) -> Path
        CorpusMaterializeError,  # typed error for clone/checkout failures
    )

Selection rule (must be documented in the module docstring and mirrored here):
  1. Filter: keep only records where language ∈ {python, typescript}.
  2. Stable sort by (repo, instance_id) — deterministic across any input ordering.
  3. Greedy repo admission: iterate sorted records; admit a repo to the repo-set until
     the repo-set hits max_repos distinct repos.  A record's repo is admitted if it is
     already in the set OR the set has < max_repos repos.
  4. Task admission: admit the record to the output until max_tasks records are in the output.
  5. Returns exactly min(admitted_tasks, max_tasks) records, deterministically.
"""

from __future__ import annotations

import subprocess
from pathlib import Path
from unittest.mock import patch

import pytest

# ---------------------------------------------------------------------------
# Inline fixture records (faithful to real ContextBench-Lite schema).
# ---------------------------------------------------------------------------

# Python records — two different repos.
REC_ASTROPY_1 = {
    "instance_id": "SWE-Bench-Verified__python__bugfix__aaa",
    "repo": "astropy/astropy",
    "repo_url": "https://github.com/astropy/astropy.git",
    "language": "python",
    "base_commit": "abc111",
    "problem_statement": "Fix the coordinate transform.",
    "gold_context": '[{"file": "astropy/coordinates/attributes.py", "start_line": 1, "end_line": 10, "content": "class Foo: pass"}]',
}

REC_ASTROPY_2 = {
    "instance_id": "SWE-Bench-Verified__python__bugfix__bbb",
    "repo": "astropy/astropy",
    "repo_url": "https://github.com/astropy/astropy.git",
    "language": "python",
    "base_commit": "abc222",
    "problem_statement": "Fix the WCS module.",
    "gold_context": '[{"file": "astropy/wcs/wcs.py", "start_line": 5, "end_line": 15, "content": "class WCS: pass"}]',
}

REC_PYTEST_1 = {
    "instance_id": "SWE-Bench-Verified__python__bugfix__ppp",
    "repo": "pytest-dev/pytest",
    "repo_url": "https://github.com/pytest-dev/pytest.git",
    "language": "python",
    "base_commit": "def333",
    "problem_statement": "Fix fixture scoping.",
    "gold_context": '[{"file": "src/_pytest/fixtures.py", "start_line": 10, "end_line": 20, "content": "def fixture(): pass"}]',
}

# TypeScript records.
REC_VUE_1 = {
    "instance_id": "Multi-SWE-Bench__typescript__bugfix__vvv",
    "repo": "vuejs/core",
    "repo_url": "https://github.com/vuejs/core.git",
    "language": "typescript",
    "base_commit": "ghi444",
    "problem_statement": "Fix reactivity tracking.",
    "gold_context": '[{"file": "packages/reactivity/src/effect.ts", "start_line": 1, "end_line": 5, "content": "export function effect(){}"}]',
}

# Go record — must be excluded.
REC_GO_1 = {
    "instance_id": "SWE-Bench__go__bugfix__ggg",
    "repo": "cli/cli",
    "repo_url": "https://github.com/cli/cli.git",
    "language": "go",
    "base_commit": "jkl555",
    "problem_statement": "Fix CLI flag parsing.",
    "gold_context": "[]",
}

# Java record — must be excluded.
REC_JAVA_1 = {
    "instance_id": "SWE-Bench__java__bugfix__jjj",
    "repo": "fasterxml/jackson",
    "repo_url": "https://github.com/fasterxml/jackson.git",
    "language": "java",
    "base_commit": "mno666",
    "problem_statement": "Fix JSON serialisation.",
    "gold_context": "[]",
}

# Fourth repo (should be excluded when max_repos=3).
REC_MATPLOTLIB_1 = {
    "instance_id": "SWE-Bench-Verified__python__bugfix__mmm",
    "repo": "matplotlib/matplotlib",
    "repo_url": "https://github.com/matplotlib/matplotlib.git",
    "language": "python",
    "base_commit": "pqr777",
    "problem_statement": "Fix colormap rendering.",
    "gold_context": '[{"file": "lib/matplotlib/cm.py", "start_line": 1, "end_line": 10, "content": "class ScalarMappable: pass"}]',
}

ALL_RECORDS = [REC_ASTROPY_1, REC_ASTROPY_2, REC_PYTEST_1, REC_VUE_1, REC_GO_1, REC_JAVA_1, REC_MATPLOTLIB_1]

# ---------------------------------------------------------------------------
# Production import — will fail ImportError in RED state.
# ---------------------------------------------------------------------------

from r1harness.contextbench_corpus import (  # type: ignore[import]  # noqa: E402
    CorpusMaterializeError,
    checkout_argv,
    clone_argv,
    repo_cache_dir,
    select_tasks,
    task_repo_dir,
)


# ---------------------------------------------------------------------------
# 1. Selector — language filter
# ---------------------------------------------------------------------------


def test_selector_drops_go_records():
    """Go records are excluded from the selector output."""
    result = select_tasks([REC_GO_1, REC_ASTROPY_1], max_repos=4, max_tasks=10)
    repos = {r["repo"] for r in result}
    assert "cli/cli" not in repos, "Go record must be excluded"
    assert "astropy/astropy" in repos


def test_selector_drops_unsupported_languages():
    """Java, Go, C, Rust records are excluded; only python and typescript admitted."""
    result = select_tasks(ALL_RECORDS, max_repos=4, max_tasks=20)
    langs = {r["language"] for r in result}
    assert langs <= {"python", "typescript"}, f"Unexpected languages: {langs - {'python', 'typescript'}}"


def test_selector_admits_python_and_typescript():
    """Python and TypeScript records are both admitted when cap allows."""
    result = select_tasks([REC_ASTROPY_1, REC_VUE_1], max_repos=2, max_tasks=10)
    langs = {r["language"] for r in result}
    assert "python" in langs
    assert "typescript" in langs


# ---------------------------------------------------------------------------
# 2. Selector — repo cap
# ---------------------------------------------------------------------------


def test_selector_repo_cap_exactly_max_repos():
    """Result spans ≤ max_repos distinct repos when the input has more."""
    result = select_tasks(ALL_RECORDS, max_repos=2, max_tasks=20)
    repos = {r["repo"] for r in result}
    assert len(repos) <= 2, f"Expected ≤2 repos, got {len(repos)}: {repos}"


def test_selector_repo_cap_3_repos():
    """With max_repos=3, at most 3 distinct repos appear."""
    result = select_tasks(ALL_RECORDS, max_repos=3, max_tasks=20)
    repos = {r["repo"] for r in result}
    assert len(repos) <= 3, f"Expected ≤3 repos, got {len(repos)}: {repos}"


def test_selector_repo_cap_1_repo():
    """With max_repos=1, only one distinct repo appears in the output."""
    result = select_tasks(ALL_RECORDS, max_repos=1, max_tasks=20)
    repos = {r["repo"] for r in result}
    assert len(repos) == 1, f"Expected exactly 1 repo, got {repos}"


# ---------------------------------------------------------------------------
# 3. Selector — task cap + determinism
# ---------------------------------------------------------------------------


def test_selector_task_cap_respected():
    """Output length ≤ max_tasks even when more tasks are available."""
    result = select_tasks(ALL_RECORDS, max_repos=4, max_tasks=2)
    assert len(result) <= 2, f"Expected ≤2 tasks, got {len(result)}"


def test_selector_deterministic_same_input_same_output():
    """Same input always produces the same output (independent of list order stability)."""
    result1 = select_tasks(ALL_RECORDS, max_repos=4, max_tasks=10)
    result2 = select_tasks(ALL_RECORDS, max_repos=4, max_tasks=10)
    ids1 = [r["instance_id"] for r in result1]
    ids2 = [r["instance_id"] for r in result2]
    assert ids1 == ids2, "selector must be deterministic"


def test_selector_deterministic_after_input_shuffle():
    """Shuffling the input records does NOT change the output (sort rule is stable)."""
    import random

    records = list(ALL_RECORDS)
    result1 = select_tasks(records, max_repos=4, max_tasks=10)
    shuffled = list(records)
    random.seed(42)
    random.shuffle(shuffled)
    result2 = select_tasks(shuffled, max_repos=4, max_tasks=10)
    ids1 = [r["instance_id"] for r in result1]
    ids2 = [r["instance_id"] for r in result2]
    assert ids1 == ids2, f"Determinism broken after shuffle:\n  before: {ids1}\n  after:  {ids2}"


def test_selector_stable_sort_order_by_repo_then_instance_id():
    """Output is sorted by (repo, instance_id) — the documented selection rule."""
    result = select_tasks([REC_ASTROPY_2, REC_ASTROPY_1, REC_PYTEST_1], max_repos=4, max_tasks=10)
    ids = [r["instance_id"] for r in result]
    expected = sorted([REC_ASTROPY_1["instance_id"], REC_ASTROPY_2["instance_id"], REC_PYTEST_1["instance_id"]])
    # The output must be in (repo, instance_id) sorted order.
    # Both astropy records share the same repo so their relative order is by instance_id.
    assert ids == expected, f"Sort order incorrect: {ids} != {expected}"


def test_selector_empty_input_returns_empty():
    """Empty input → empty output."""
    assert select_tasks([], max_repos=4, max_tasks=10) == []


def test_selector_only_excluded_languages_returns_empty():
    """All-Go/Java input → empty output."""
    assert select_tasks([REC_GO_1, REC_JAVA_1], max_repos=4, max_tasks=10) == []


# ---------------------------------------------------------------------------
# 4. Materialiser — repo-cache-dir derivation (pure, no git)
# ---------------------------------------------------------------------------


def test_repo_cache_dir_github_url(tmp_path):
    """repo_cache_dir derives <cache_root>/<owner>__<name>/ from a GitHub clone URL."""
    url = "https://github.com/astropy/astropy.git"
    result = repo_cache_dir(url, tmp_path)
    assert result == tmp_path / "astropy__astropy"


def test_repo_cache_dir_no_dot_git_suffix(tmp_path):
    """repo_cache_dir handles URLs without .git suffix."""
    url = "https://github.com/vuejs/core"
    result = repo_cache_dir(url, tmp_path)
    assert result == tmp_path / "vuejs__core"


def test_repo_cache_dir_is_under_cache_root(tmp_path):
    """The returned path is always a direct child of cache_root."""
    url = "https://github.com/pytest-dev/pytest.git"
    result = repo_cache_dir(url, tmp_path)
    assert result.parent == tmp_path


def test_repo_cache_dir_uses_double_underscore_separator(tmp_path):
    """The separator between owner and name is '__' (double underscore)."""
    url = "https://github.com/mui/material-ui.git"
    result = repo_cache_dir(url, tmp_path)
    assert result.name == "mui__material-ui"


# ---------------------------------------------------------------------------
# 5. Materialiser — clone/checkout argv (pure, no subprocess)
# ---------------------------------------------------------------------------


def test_clone_argv_contains_git_clone():
    """clone_argv starts with 'git' and 'clone'."""
    args = clone_argv("https://github.com/astropy/astropy.git", Path("/tmp/astropy__astropy"))
    assert args[0] == "git"
    assert "clone" in args


def test_clone_argv_includes_url_and_dest():
    """clone_argv includes the URL and dest dir."""
    url = "https://github.com/astropy/astropy.git"
    dest = Path("/tmp/cache/astropy__astropy")
    args = clone_argv(url, dest)
    assert url in args
    assert str(dest) in args


def test_checkout_argv_contains_git_checkout():
    """checkout_argv starts with 'git' and 'checkout'."""
    args = checkout_argv("abc123def456")
    assert args[0] == "git"
    assert "checkout" in args


def test_checkout_argv_includes_commit():
    """checkout_argv includes the commit hash."""
    commit = "6500928dc0e57be8f06d1162eacc3ba5e2eff692"
    args = checkout_argv(commit)
    assert commit in args


# ---------------------------------------------------------------------------
# 6. Materialiser — task_repo_dir (per-task working-tree path, pure)
# ---------------------------------------------------------------------------


def test_task_repo_dir_is_under_cache_root(tmp_path):
    """task_repo_dir returns a path that is a descendant of cache_root."""
    result = task_repo_dir(tmp_path, "https://github.com/astropy/astropy.git", "abc123")
    assert str(result).startswith(str(tmp_path))


def test_task_repo_dir_includes_commit_in_path(tmp_path):
    """task_repo_dir embeds the base_commit (or its prefix) in the returned path."""
    commit = "abc123def456"
    result = task_repo_dir(tmp_path, "https://github.com/astropy/astropy.git", commit)
    # The path must contain the commit (or at least the first 8 chars) so different
    # commits for the same repo produce distinct directories.
    path_str = str(result)
    assert commit[:8] in path_str or commit in path_str, f"Expected commit {commit!r} in path {path_str!r}"


def test_task_repo_dir_different_commits_produce_different_paths(tmp_path):
    """Different base_commits for the same repo → different task directories."""
    url = "https://github.com/astropy/astropy.git"
    path_a = task_repo_dir(tmp_path, url, "commit_aaaa")
    path_b = task_repo_dir(tmp_path, url, "commit_bbbb")
    assert path_a != path_b, "Different commits must produce distinct task dirs"


def test_task_repo_dir_same_commit_same_path(tmp_path):
    """Same URL + commit always returns the same path (idempotent)."""
    url = "https://github.com/astropy/astropy.git"
    commit = "abc123"
    path_a = task_repo_dir(tmp_path, url, commit)
    path_b = task_repo_dir(tmp_path, url, commit)
    assert path_a == path_b


# ---------------------------------------------------------------------------
# 7. Materialiser — failure handling (mock subprocess boundary)
# ---------------------------------------------------------------------------


def test_clone_failure_raises_corpus_materialize_error(tmp_path):
    """A failing git clone raises CorpusMaterializeError, not an unhandled exception."""
    from r1harness.contextbench_corpus import materialize_task

    rec = REC_ASTROPY_1
    cache_root = tmp_path / "repos"

    with patch("subprocess.run") as mock_run:
        mock_run.return_value = subprocess.CompletedProcess(
            args=["git", "clone", "..."],
            returncode=128,
            stdout="",
            stderr="fatal: repository not found",
        )
        with pytest.raises(CorpusMaterializeError):
            materialize_task(rec, cache_root=cache_root)


def test_checkout_failure_raises_corpus_materialize_error(tmp_path):
    """A failing git checkout raises CorpusMaterializeError, not an unhandled exception."""
    from r1harness.contextbench_corpus import materialize_task

    rec = REC_ASTROPY_1
    cache_root = tmp_path / "repos"

    call_count = 0

    def fake_run(args, **kwargs):
        nonlocal call_count
        call_count += 1
        if call_count == 1:
            # First call: clone succeeds (create the dest dir so checkout is reached).
            dest = Path(args[-1])
            dest.mkdir(parents=True, exist_ok=True)
            return subprocess.CompletedProcess(args=args, returncode=0, stdout="", stderr="")
        else:
            # Second call: checkout fails.
            return subprocess.CompletedProcess(args=args, returncode=1, stdout="", stderr="error: pathspec not in tree")

    with patch("subprocess.run", side_effect=fake_run):
        with pytest.raises(CorpusMaterializeError):
            materialize_task(rec, cache_root=cache_root)


# ---------------------------------------------------------------------------
# 8. Path alignment — gold_files align with normalize_path output
# ---------------------------------------------------------------------------


def test_path_alignment_gold_files_match_normalize_path(tmp_path):
    """gold_files from parse_contextbench_records align with normalize_path output.

    Proves that a retrieved absolute path (from codecache under the repo dir)
    normalize_path-relativised to the repo_dir matches the gold_files frozenset.
    """
    from r1harness.codecache_tool import normalize_path
    from r1harness.contextbench import parse_contextbench_records

    # A record whose gold_context references a repo-relative file.
    gold_relative = "astropy/coordinates/attributes.py"
    record = {
        "instance_id": "test_path_alignment",
        "repo": "astropy/astropy",
        "repo_url": "https://github.com/astropy/astropy.git",
        "language": "python",
        "base_commit": "abc123",
        "problem_statement": "Test path alignment.",
        "gold_context": (
            '[{"file": "astropy/coordinates/attributes.py",'
            ' "start_line": 344, "end_line": 396, "content": "class Foo: pass"}]'
        ),
    }

    sweep_queries = parse_contextbench_records([record])
    assert len(sweep_queries) == 1
    sq = sweep_queries[0]

    # Simulate: codecache returns absolute path under a cloned repo dir.
    # The repo dir is tmp_path (as if we cloned astropy there).
    repo_dir = tmp_path
    abs_path = str(repo_dir / gold_relative)

    # normalize_path must yield the repo-relative posix string.
    normalized = normalize_path(abs_path, repo_dir)
    assert normalized == gold_relative, (
        f"normalize_path({abs_path!r}, {repo_dir!r}) = {normalized!r}, expected {gold_relative!r}"
    )

    # The normalised path must be in the gold_files frozenset.
    assert normalized in sq.gold_files, f"Normalised path {normalized!r} not found in gold_files {sq.gold_files}"


def test_path_alignment_non_verbatim_path_returns_original():
    """normalize_path falls back gracefully when path is not under repo_dir."""
    from r1harness.codecache_tool import normalize_path

    # A path that is not under any repo_dir → returns as-is (posix-normalised).
    result = normalize_path("/some/other/dir/file.py", Path("/totally/different/dir"))
    assert result == "/some/other/dir/file.py"
