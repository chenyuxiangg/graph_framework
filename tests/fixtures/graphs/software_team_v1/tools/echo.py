"""software_team v1 e2e 工具 fixture：`echo` 原样返回参数。

用于 ``HARNESS_MAX_ITERATIONS`` 失败路径：评审 agent 的 ``max_iterations=1``，fake LLM
第一次响应返回 ``tool_calls``（调用本工具）→ 工具执行成功后进入下一轮检查 → 达到
``max_iterations`` 上限 → ``HARNESS_MAX_ITERATIONS`` → 实例 Crashed → run Failed。
（05 §5.5.2：``max_iterations`` 是 LLM 调用轮次预算；工具执行发生在预算检查之后。）
"""

from __future__ import annotations

from typing import Any

TOOL: dict[str, Any] = {
    "name": "echo",
    "description": "原样返回 note 参数（software_team e2e 工具调用 fixture）",
    "parameters": {
        "type": "object",
        "properties": {"note": {"type": "string"}},
        "additionalProperties": False,
    },
}


def call(args: dict[str, Any]) -> dict[str, Any]:
    """返回 ``{"echo": <note>}``（无副作用）。"""
    return {"echo": args.get("note", "")}
