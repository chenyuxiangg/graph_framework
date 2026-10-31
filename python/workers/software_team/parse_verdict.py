"""software_team SOP 的评审结论解析 worker（W1 v1 / W2a P v2）。

- ``run``：SOP 设计稿 v1 §5.2 契约——评审不通过/不可解析即抛错；
- ``parse_verdict_rework``：W2a opt-in 返工契约（DR-004 D1）——``approved``/``rework``
  都是正常输出，只有非法输入才抛错；v1 语义与默认行为保持不变。

``run`` 已在 REV-064 评审合并后冻结，故 ``parse_verdict_rework`` 保持独立实现
（不改动 v1 的异常前缀与判定顺序），两者的校验分支刻意重复。
"""

from __future__ import annotations

import json
from typing import Any

from zgraph_worker import WorkerContext

APPROVED: str = "approved"
REWORK: str = "rework"
VERDICTS: frozenset[str] = frozenset({APPROVED, REWORK})
VERDICT_KEY: str = "verdict"
COMMENTS_KEY: str = "comments"
ARTIFACT_KEY: str = "artifact"


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """解析评审 agent 输出，返回 verdict / comments / artifact（SOP v1 契约）。

    输入：
        ``review``（object，必填）：评审 agent 的整包输出；``content`` 必须是含
        ``verdict`` 的 JSON 字符串；
        ``artifact``（any，必填）：工作 agent 的整包产物，原样透传。
    输出：
        ``{"verdict": "approved", "comments": <str>, "artifact": <原样>}``。
    幂等性：
        纯函数，相同 inputs 输出相同。
    外部副作用：
        无（不写文件、不发网络、不写全局状态）。
    异常（全部 ``ValueError``，worker 侧映射为 ``NODE_PYTHON_EXCEPTION``）：
        缺 ``review`` 或 ``artifact``；``review`` 非 object；``content`` 缺失、非 string
        或纯空白；``content`` 非法 JSON 或解析后非 object；``verdict`` 缺失或
        ``verdict != "approved"``；``comments`` 存在但非 string。
    """
    if "review" not in inputs:
        raise ValueError("parse_verdict: missing input 'review'")
    if "artifact" not in inputs:
        raise ValueError("parse_verdict: missing input 'artifact'")
    review: Any = inputs["review"]
    if not isinstance(review, dict):
        raise ValueError("parse_verdict: 'review' must be an object")
    content: Any = review.get("content")
    if not isinstance(content, str) or not content.strip():
        raise ValueError("parse_verdict: review.content must be a non-empty JSON string")
    try:
        parsed: Any = json.loads(content)
    except json.JSONDecodeError as exc:
        raise ValueError(f"parse_verdict: review.content is not valid JSON: {exc}") from exc
    if not isinstance(parsed, dict):
        raise ValueError("parse_verdict: review.content must decode to an object")
    verdict: Any = parsed.get(VERDICT_KEY)
    if verdict != APPROVED:
        raise ValueError(f"parse_verdict: review verdict is not approved: {verdict!r}")
    comments: Any = parsed.get(COMMENTS_KEY, "")
    if not isinstance(comments, str):
        raise ValueError("parse_verdict: review comments must be a string")
    return {VERDICT_KEY: APPROVED, COMMENTS_KEY: comments, ARTIFACT_KEY: inputs["artifact"]}


def parse_verdict_rework(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """解析评审 agent 输出（P v2，opt-in 有限返工），返回 verdict / comments / artifact。

    与 v1 ``run`` 的唯一语义差异：``verdict == "rework"`` 是**正常输出**而非异常；
    ``run`` 的「非 approved 即抛错」契约保持不变（SOP v1 默认行为不变）。

    输入：
        ``review``（object，必填）：评审 agent 的整包输出；``content`` 必须是含
        ``verdict`` 的 JSON 字符串，且 ``verdict`` ∈ {``"approved"``, ``"rework"``}；
        ``artifact``（any，必填）：工作 agent 的整包产物，按对象 identity 原样透传。
    输出：
        ``{"verdict": "approved" | "rework", "comments": <str>, "artifact": <原样>}``
        （恰 3 键；``comments`` 缺省为 ``""``）。
    幂等性：
        纯函数，相同 ``inputs`` 输出相同（不含时间/随机），可安全重试。
    外部副作用：
        无（不写文件、不发网络、不写全局/进程外状态）。
    异常（全部 ``ValueError``，message 以 ``parse_verdict_rework: `` 开头，
    worker 侧映射为 ``NODE_PYTHON_EXCEPTION`` → 实例 Crashed → run Failed）：
        缺 ``review`` 或 ``artifact``；``review`` 非 object；``content`` 缺失、非 string
        或纯空白；``content`` 非法 JSON 或解析后非 object；``verdict`` 缺失或不在
        ``{approved, rework}``（大小写敏感）；``comments`` 存在但非 string。
    """
    if "review" not in inputs:
        raise ValueError("parse_verdict_rework: missing input 'review'")
    if "artifact" not in inputs:
        raise ValueError("parse_verdict_rework: missing input 'artifact'")
    review: Any = inputs["review"]
    if not isinstance(review, dict):
        raise ValueError("parse_verdict_rework: 'review' must be an object")
    content: Any = review.get("content")
    if not isinstance(content, str) or not content.strip():
        raise ValueError("parse_verdict_rework: review.content must be a non-empty JSON string")
    try:
        parsed: Any = json.loads(content)
    except json.JSONDecodeError as exc:
        raise ValueError(f"parse_verdict_rework: review.content is not valid JSON: {exc}") from exc
    if not isinstance(parsed, dict):
        raise ValueError("parse_verdict_rework: review.content must decode to an object")
    verdict: Any = parsed.get(VERDICT_KEY)
    if not isinstance(verdict, str) or verdict not in VERDICTS:
        raise ValueError(
            f"parse_verdict_rework: review verdict must be one of {sorted(VERDICTS)}: {verdict!r}"
        )
    comments: Any = parsed.get(COMMENTS_KEY, "")
    if not isinstance(comments, str):
        raise ValueError("parse_verdict_rework: review comments must be a string")
    return {VERDICT_KEY: verdict, COMMENTS_KEY: comments, ARTIFACT_KEY: inputs["artifact"]}
