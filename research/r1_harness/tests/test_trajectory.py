"""Unit tests for the JSONL trajectory schema + Layer-2 metric extraction."""

from r1harness.trajectory import (
    TrajectoryLogger,
    TrajectoryMeta,
    read_trajectory,
    surfaced_lists,
    tokens_to_coverage,
    total_tokens,
    turns_to_coverage,
)


def _meta() -> TrajectoryMeta:
    return TrajectoryMeta(
        arm="A1",
        task_id="auth_q1",
        model="deterministic",
        temperature=0.0,
        corpus_id="auth_module",
        query="authenticate user credentials",
    )


def test_round_trip_meta_and_turns(tmp_path):
    path = tmp_path / "traj.jsonl"
    logger = TrajectoryLogger(path, _meta())
    logger.log_turn(
        action='codecache query "authenticate user credentials" --format json',
        action_kind="codecache_query",
        observation="{...}",
        prompt_tokens=120,
        completion_tokens=30,
        files_surfaced=["src/auth/authenticate.py"],
        blocks_surfaced=[("src/auth/authenticate.py", "authenticate_user")],
    )
    logger.log_turn(action="submit", action_kind="submit", observation="done", prompt_tokens=10, completion_tokens=5)

    meta, turns = read_trajectory(path)
    assert meta.arm == "A1"
    assert meta.query == "authenticate user credentials"
    assert [t.turn for t in turns] == [1, 2]
    # tuples are restored from JSON 2-element lists
    assert turns[0].blocks_surfaced == [("src/auth/authenticate.py", "authenticate_user")]
    assert turns[0].action_kind == "codecache_query"


def test_surfaced_lists_are_cumulative_in_order(tmp_path):
    path = tmp_path / "t.jsonl"
    logger = TrajectoryLogger(path, _meta())
    logger.log_turn("grep x", "bash", "...", files_surfaced=["a.py"], blocks_surfaced=[("a.py", "x")])
    logger.log_turn("cat b", "bash", "...", files_surfaced=["b.py"], blocks_surfaced=[("b.py", "y")])
    _, turns = read_trajectory(path)
    files, blocks = surfaced_lists(turns)
    assert files == ["a.py", "b.py"]
    assert blocks == [("a.py", "x"), ("b.py", "y")]


def test_turns_to_coverage_hits_on_second_turn(tmp_path):
    path = tmp_path / "t.jsonl"
    logger = TrajectoryLogger(path, _meta())
    gold = {("src/auth/authenticate.py", "authenticate_user")}
    logger.log_turn("grep wrong", "bash", "...", blocks_surfaced=[("other.py", "z")])
    logger.log_turn(
        'codecache query "..."',
        "codecache_query",
        "...",
        blocks_surfaced=[("src/auth/authenticate.py", "authenticate_user")],
    )
    _, turns = read_trajectory(path)
    assert turns_to_coverage(turns, gold) == 2


def test_tokens_to_coverage_sums_until_covered(tmp_path):
    path = tmp_path / "t.jsonl"
    logger = TrajectoryLogger(path, _meta())
    gold = {("a.py", "f")}
    logger.log_turn("t1", "bash", "...", prompt_tokens=100, completion_tokens=20, blocks_surfaced=[("x.py", "g")])
    logger.log_turn("t2", "codecache_query", "...", prompt_tokens=50, completion_tokens=10, blocks_surfaced=[("a.py", "f")])
    logger.log_turn("t3", "submit", "...", prompt_tokens=999, completion_tokens=999)
    _, turns = read_trajectory(path)
    # covered at turn 2: 100+20+50+10 = 180 ; turn 3 tokens excluded
    assert tokens_to_coverage(turns, gold) == 180
    assert total_tokens(turns) == 100 + 20 + 50 + 10 + 999 + 999


def test_coverage_never_reached_is_none(tmp_path):
    path = tmp_path / "t.jsonl"
    logger = TrajectoryLogger(path, _meta())
    gold = {("a.py", "f")}
    logger.log_turn("t1", "bash", "...", prompt_tokens=10, blocks_surfaced=[("x.py", "g")])
    _, turns = read_trajectory(path)
    assert turns_to_coverage(turns, gold) is None
    assert tokens_to_coverage(turns, gold) is None


def test_empty_gold_covered_immediately(tmp_path):
    path = tmp_path / "t.jsonl"
    logger = TrajectoryLogger(path, _meta())
    logger.log_turn("t1", "bash", "...", prompt_tokens=7)
    _, turns = read_trajectory(path)
    assert turns_to_coverage(turns, set()) == 1
    assert tokens_to_coverage(turns, set()) == 7
