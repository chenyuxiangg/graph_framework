"""AC-08 工具 fixture（形式 B）：`explode` 总是抛 ``ValueError("boom")``。

用于验证「工具抛异常 → TOOL_ERROR 喂回 LLM → run 继续」语义（05 §5.6.6）。
"""

from __future__ import annotations

from typing import Any

TOOL: dict[str, Any] = {
    "name": "explode",
    "description": "总是抛 ValueError(\"boom\")，用于工具异常语义验收",
    "parameters": {
        "type": "object",
        "properties": {},
        "required": [],
        "additionalProperties": False,
    },
}


def call(args: dict[str, Any]) -> dict[str, Any]:
    """总是抛 ``ValueError("boom")``（参数未使用）。"""
    del args
    raise ValueError("boom")
