"""Unit tests for the codecache tool adapter's pure parsing/normalisation.

These need no binary — they exercise §6.4.2 JSON parsing and the path
relativisation that makes retrieved paths gold-comparable. The end-to-end run
against the real binary is verified separately (see README "Running").
"""

import json
from pathlib import Path

from r1harness.codecache_tool import normalize_path, parse_query_json


def test_normalize_absolute_path_under_repo(tmp_path):
    abs_fp = (tmp_path / "src" / "auth" / "authenticate.py")
    abs_fp.parent.mkdir(parents=True)
    abs_fp.write_text("x", encoding="utf-8")
    assert normalize_path(str(abs_fp), tmp_path) == "src/auth/authenticate.py"


def test_normalize_relative_path_backslashes_to_posix():
    assert normalize_path("src\\auth\\authenticate.py", None) == "src/auth/authenticate.py"


def test_normalize_path_not_under_repo_falls_back(tmp_path):
    # an unrelated absolute path is posix-normalised, not crashed on
    out = normalize_path("other/place/file.py", tmp_path)
    assert out == "other/place/file.py"


def test_parse_query_json_dedups_files_keeps_block_order(tmp_path):
    payload = {
        "query": "authenticate user credentials",
        "total_results": 3,
        "total_tokens": 280,
        "chunks": [
            {"symbol_name": "authenticate_user", "file_path": str(tmp_path / "src/auth/authenticate.py"),
             "symbol_type": "function", "language": "python", "bm25_score": 9.1, "chunk_text": "def authenticate_user(): ..."},
            {"symbol_name": "verify_password", "file_path": str(tmp_path / "src/auth/authenticate.py"),
             "symbol_type": "function", "language": "python", "bm25_score": 4.0, "chunk_text": "def verify_password(): ..."},
            {"symbol_name": "generate_session_token", "file_path": str(tmp_path / "src/auth/session.py"),
             "symbol_type": "function", "language": "python", "bm25_score": 2.0, "chunk_text": "def generate_session_token(): ..."},
        ],
    }
    qr = parse_query_json(json.dumps(payload), "authenticate user credentials", repo_dir=tmp_path)
    # file list dedups authenticate.py to one entry, first-seen order
    assert qr.files == ["src/auth/authenticate.py", "src/auth/session.py"]
    # block list keeps all three, best-first order, relativised
    assert qr.blocks[0] == ("src/auth/authenticate.py", "authenticate_user")
    assert len(qr.blocks) == 3
    assert qr.total_tokens == 280
