"""software_team SOP 的评审结论解析 worker（W1，SOP 设计稿 v1 §5.2）。

评审 agent 的 ``content`` 是 JSON 字符串；本节点把它解析为结构化 verdict，
并在评审不通过/不可解析时抛错（``NODE_PYTHON_EXCEPTION`` → 实例 Crashed → run Failed）。
"""

from __future__ import annotations

import json
from typing import Any

from zgraph_worker import WorkerContext

APPROVED: str = "approved"
VERDICT_KEY: str = "verdict"
COMMENTS_KEY: str = "comments"
ARTIFACT_KEY: str = "artifact"


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """解析评审 agent 输出，返回 verdict / comments / artifact。

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
    异常：
        ``review`` 缺失/非 object、``content`` 非 JSON/非 object、缺 ``verdict`` 或
        ``verdict != "approved"`` 时抛 ``ValueError``（worker 侧映射为 ``NODE_PYTHON_EXCEPTION``）。
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
