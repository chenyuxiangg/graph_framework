"""形式 A 工具 fixture：TOOLS 批量声明 + call(name, args) 分发。"""

from __future__ import annotations

import datetime
from typing import Any

TOOLS: list[dict[str, Any]] = [
    {
        "name": "now",
        "description": "当前时间（ISO8601，UTC）",
        "parameters": {"type": "object", "properties": {}, "required": []},
    },
    {
        "name": "add",
        "description": "两个数字相加，返回 {sum}",
        "parameters": {
            "type": "object",
            "properties": {"a": {"type": "number"}, "b": {"type": "number"}},
            "required": ["a", "b"],
        },
    },
]


def call(name: str, args: dict[str, Any]) -> dict[str, Any]:
    """按 name 分发；未知工具抛 KeyError（用于异常路径测试）。"""
    if name == "now":
        return {"now": datetime.datetime.now(datetime.timezone.utc).isoformat()}
    if name == "add":
        return {"sum": args["a"] + args["b"]}
    raise KeyError(f"unknown tool: {name}")
