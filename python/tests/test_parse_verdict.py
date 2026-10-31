"""``workers.software_team.parse_verdict`` 单测（W1：SOP v1 评审解析节点）。"""

from __future__ import annotations

import json
from typing import Any, cast

import pytest

from workers.software_team.parse_verdict import APPROVED, run

ARTIFACT: dict[str, Any] = {"documents": {"doc/a.md": "# a\n"}, "document_paths": ["doc/a.md"]}


def _review(payload: Any) -> dict[str, Any]:
    return {"content": json.dumps(payload), "stop_reason": "EndTurn", "iterations": 1}


def _run(inputs: dict[str, Any]) -> dict[str, Any]:
    return run(inputs, cast(Any, None))


def test_approved_returns_verdict_comments_and_artifact() -> None:
    output: dict[str, Any] = _run(
        {"review": _review({"verdict": APPROVED, "comments": "ok"}), "artifact": ARTIFACT}
    )
    assert output == {"verdict": APPROVED, "comments": "ok", "artifact": ARTIFACT}


def test_comments_default_to_empty_string() -> None:
    output: dict[str, Any] = _run({"review": _review({"verdict": APPROVED}), "artifact": ARTIFACT})
    assert output["comments"] == ""


def test_artifact_is_passed_through_by_identity() -> None:
    artifact: dict[str, Any] = {"document_paths": ["doc/x.md"]}
    output: dict[str, Any] = _run({"review": _review({"verdict": APPROVED}), "artifact": artifact})
    assert output["artifact"] is artifact


@pytest.mark.parametrize("bad_content", ["not-json", "[]", "null", "42", '"approved"', ""])
def test_invalid_content_raises(bad_content: str) -> None:
    with pytest.raises(ValueError):
        _run({"review": {"content": bad_content}, "artifact": ARTIFACT})


@pytest.mark.parametrize("bad_verdict", [None, "rework", "APPROVED", 1, True])
def test_non_approved_verdict_raises(bad_verdict: Any) -> None:
    with pytest.raises(ValueError):
        _run({"review": _review({"verdict": bad_verdict}), "artifact": ARTIFACT})


def test_missing_verdict_raises() -> None:
    with pytest.raises(ValueError):
        _run({"review": _review({"comments": "ok"}), "artifact": ARTIFACT})


def test_non_string_comments_raises() -> None:
    with pytest.raises(ValueError):
        _run({"review": _review({"verdict": APPROVED, "comments": 1}), "artifact": ARTIFACT})


def test_missing_review_raises() -> None:
    with pytest.raises(ValueError):
        _run({"artifact": ARTIFACT})


def test_missing_artifact_raises() -> None:
    with pytest.raises(ValueError):
        _run({"review": _review({"verdict": APPROVED})})


def test_review_must_be_object() -> None:
    with pytest.raises(ValueError):
        _run({"review": "approved", "artifact": ARTIFACT})


def test_content_must_be_string() -> None:
    with pytest.raises(ValueError):
        _run({"review": {"content": {"verdict": APPROVED}}, "artifact": ARTIFACT})
