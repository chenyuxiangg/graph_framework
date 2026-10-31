"""``workers.software_team.parse_verdict`` 单测（W1 v1 + W2a P v2 返工解析）。"""

from __future__ import annotations

import json
from typing import Any, cast

import pytest

from workers.software_team.parse_verdict import (
    APPROVED,
    REWORK,
    VERDICTS,
    parse_verdict_rework,
    run,
)

ARTIFACT: dict[str, Any] = {"documents": {"doc/a.md": "# a\n"}, "document_paths": ["doc/a.md"]}
ERROR_PREFIX: str = "parse_verdict_rework:"


def _review(payload: Any) -> dict[str, Any]:
    return {"content": json.dumps(payload), "stop_reason": "EndTurn", "iterations": 1}


def _rework_review() -> dict[str, Any]:
    return _review({"verdict": REWORK})


def _run(inputs: dict[str, Any]) -> dict[str, Any]:
    return run(inputs, cast(Any, None))


def _rework(inputs: dict[str, Any]) -> dict[str, Any]:
    return parse_verdict_rework(inputs, cast(Any, None))


def _assert_rework_error(inputs: dict[str, Any], fragment: str) -> None:
    with pytest.raises(ValueError) as excinfo:
        _rework(inputs)
    message: str = str(excinfo.value)
    assert message.startswith(ERROR_PREFIX)
    assert fragment in message


# --------------------------------------------------------------------------- #
# v1 ``run``：既有契约（非 approved 即异常）保持不变
# --------------------------------------------------------------------------- #


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
    assert output["artifact"] == artifact


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


# ISSUE-101 补充的 v1 边界用例（docstring 异常清单同步补全）。
def test_content_key_missing_raises() -> None:
    with pytest.raises(ValueError):
        _run({"review": {"stop_reason": "EndTurn"}, "artifact": ARTIFACT})


@pytest.mark.parametrize("blank_content", [" ", "  \n\t "])
def test_blank_content_raises(blank_content: str) -> None:
    with pytest.raises(ValueError):
        _run({"review": {"content": blank_content}, "artifact": ARTIFACT})


@pytest.mark.parametrize("bad_content", [None, 1, [], True])
def test_content_non_string_raises(bad_content: Any) -> None:
    with pytest.raises(ValueError):
        _run({"review": {"content": bad_content}, "artifact": ARTIFACT})


def test_parsed_content_must_be_object() -> None:
    with pytest.raises(ValueError):
        _run({"review": {"content": "[]"}, "artifact": ARTIFACT})


# --------------------------------------------------------------------------- #
# P v2 ``parse_verdict_rework``：approved / rework 均为正常输出
# --------------------------------------------------------------------------- #


def test_verdicts_is_closed_immutable_set() -> None:
    assert isinstance(VERDICTS, frozenset)
    assert VERDICTS == frozenset({APPROVED, REWORK})
    assert APPROVED == "approved"
    assert REWORK == "rework"


def test_rework_approved_returns_exact_three_keys() -> None:
    output: dict[str, Any] = _rework(
        {"review": _review({"verdict": APPROVED, "comments": "ok"}), "artifact": ARTIFACT}
    )
    assert output == {"verdict": APPROVED, "comments": "ok", "artifact": ARTIFACT}
    assert set(output) == {"verdict", "comments", "artifact"}


def test_rework_verdict_is_normal_output() -> None:
    output: dict[str, Any] = _rework(
        {"review": _review({"verdict": REWORK, "comments": "please fix"}), "artifact": ARTIFACT}
    )
    assert output == {"verdict": REWORK, "comments": "please fix", "artifact": ARTIFACT}


def test_rework_comments_default_to_empty_string() -> None:
    output: dict[str, Any] = _rework({"review": _rework_review(), "artifact": ARTIFACT})
    assert output["comments"] == ""


def test_rework_comments_are_preserved_verbatim() -> None:
    comments: str = "第一行\n第二行：请补测试"
    output: dict[str, Any] = _rework(
        {"review": _review({"verdict": REWORK, "comments": comments}), "artifact": ARTIFACT}
    )
    assert output["comments"] == comments


def test_rework_artifact_is_passed_through_by_identity() -> None:
    artifact: dict[str, Any] = {"document_paths": ["doc/x.md"]}
    output: dict[str, Any] = _rework({"review": _rework_review(), "artifact": artifact})
    assert output["artifact"] is artifact
    assert output["artifact"] == artifact


@pytest.mark.parametrize("artifact", [None, "plain", 42, ["a.b"], {"k": "v"}])
def test_rework_artifact_accepts_any_value(artifact: Any) -> None:
    output: dict[str, Any] = _rework({"review": _rework_review(), "artifact": artifact})
    assert output["artifact"] is artifact


# --------------------------------------------------------------------------- #
# P v2 异常矩阵：全部 ValueError，message 以 ``parse_verdict_rework:`` 开头
# --------------------------------------------------------------------------- #


def test_rework_missing_review_raises() -> None:
    _assert_rework_error({"artifact": ARTIFACT}, "missing input 'review'")


def test_rework_missing_artifact_raises() -> None:
    _assert_rework_error({"review": _rework_review()}, "missing input 'artifact'")


@pytest.mark.parametrize("bad_review", ["approved", None, [], 1, True])
def test_rework_review_must_be_object(bad_review: Any) -> None:
    _assert_rework_error(
        {"review": bad_review, "artifact": ARTIFACT}, "'review' must be an object"
    )


def test_rework_content_key_missing_raises() -> None:
    _assert_rework_error(
        {"review": {"stop_reason": "EndTurn"}, "artifact": ARTIFACT}, "review.content"
    )


@pytest.mark.parametrize("bad_content", [None, 1, {"verdict": REWORK}, [REWORK], True])
def test_rework_content_must_be_string(bad_content: Any) -> None:
    _assert_rework_error(
        {"review": {"content": bad_content}, "artifact": ARTIFACT}, "review.content"
    )


@pytest.mark.parametrize("blank_content", ["", " ", "  \n\t "])
def test_rework_blank_content_raises(blank_content: str) -> None:
    _assert_rework_error(
        {"review": {"content": blank_content}, "artifact": ARTIFACT}, "review.content"
    )


@pytest.mark.parametrize("invalid_json", ["not-json", "approved", '{"verdict": }', "{rework}"])
def test_rework_invalid_json_raises(invalid_json: str) -> None:
    _assert_rework_error(
        {"review": {"content": invalid_json}, "artifact": ARTIFACT}, "not valid JSON"
    )


@pytest.mark.parametrize("non_object", ["[]", "null", "42", '"rework"', "true"])
def test_rework_content_must_decode_to_object(non_object: str) -> None:
    _assert_rework_error(
        {"review": {"content": non_object}, "artifact": ARTIFACT}, "decode to an object"
    )


def test_rework_missing_verdict_raises() -> None:
    _assert_rework_error(
        {"review": _review({"comments": "ok"}), "artifact": ARTIFACT}, "verdict"
    )


@pytest.mark.parametrize("bad_verdict", [None, "", "rejected", "approved ", 1, True, ["rework"]])
def test_rework_unknown_verdict_raises(bad_verdict: Any) -> None:
    _assert_rework_error(
        {"review": _review({"verdict": bad_verdict}), "artifact": ARTIFACT}, "verdict"
    )


@pytest.mark.parametrize("case_variant", ["REWORK", "Rework", "APPROVED", "Approved"])
def test_rework_verdict_is_case_sensitive(case_variant: str) -> None:
    _assert_rework_error(
        {"review": _review({"verdict": case_variant}), "artifact": ARTIFACT}, "verdict"
    )


@pytest.mark.parametrize("verdict", [APPROVED, REWORK])
@pytest.mark.parametrize("bad_comments", [None, 1, [], {"c": 1}, True])
def test_rework_non_string_comments_raises(verdict: str, bad_comments: Any) -> None:
    _assert_rework_error(
        {"review": _review({"verdict": verdict, "comments": bad_comments}), "artifact": ARTIFACT},
        "comments",
    )
