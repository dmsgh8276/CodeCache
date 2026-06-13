"""Unit tests for corpus loading + materialisation from the shared gold fixture."""

from r1harness import corpus
from r1harness.arms import Task


def test_load_auth_module_corpus():
    c = corpus.load_corpus("auth_module")
    assert c.id == "auth_module"
    assert len(c.chunks) >= 5
    # authenticate.py owns multiple symbols including the auth_q1 gold block
    names = {(ch["file_path"], ch["symbol_name"]) for ch in c.chunks}
    assert ("src/auth/authenticate.py", "authenticate_user") in names


def test_load_unknown_corpus_raises():
    try:
        corpus.load_corpus("does_not_exist")
    except KeyError as e:
        assert "does_not_exist" in str(e)
    else:
        raise AssertionError("expected KeyError for unknown corpus id")


def test_materialize_writes_concatenated_files(tmp_path):
    c = corpus.load_corpus("auth_module")
    written = corpus.materialize(c, tmp_path)
    # every distinct file path was written, in first-seen order, no duplicates
    rels = [p.relative_to(tmp_path).as_posix() for p in written]
    assert rels == c.files
    assert len(rels) == len(set(rels))

    auth_py = tmp_path / "src" / "auth" / "authenticate.py"
    assert auth_py.exists()
    text = auth_py.read_text(encoding="utf-8")
    # both symbols that share authenticate.py are present (concatenated)
    assert "def authenticate_user(" in text
    assert "def verify_password(" in text


def test_task_from_dict_matches_gold_fixture():
    import json
    from pathlib import Path

    task_path = Path(__file__).resolve().parents[1] / "tasks" / "auth_q1.json"
    task = Task.from_dict(json.loads(task_path.read_text(encoding="utf-8")))
    assert task.task_id == "auth_q1"
    assert task.corpus_id == "auth_module"
    assert task.gold_files == {"src/auth/authenticate.py"}
    assert task.gold_blocks == {("src/auth/authenticate.py", "authenticate_user")}
