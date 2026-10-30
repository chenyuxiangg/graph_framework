"""AC-08 工具 fixture（形式 B）：`add` 返回两数之和。"""

from __future__ import annotations

from typing import Any

TOOL: dict[str, Any] = {
    "name": "add",
    "description": "把两个数字相加，返回 {\"sum\": a+b}",
    "parameters": {
        "type": "object",
        "properties": {
            "a": {"type": "number"},
            "b": {"type": "number"},
        },
        "required": ["a", "b"],
        "additionalProperties": False,
    },
}


def call(args: dict[str, Any]) -> dict[str, Any]:
    """把参数 a/b 相加并返回 ``{"sum": a+b}``。"""
    return {"sum": args["a"] + args["b"]}
