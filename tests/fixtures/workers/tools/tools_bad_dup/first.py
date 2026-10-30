"""坏 fixture：与 second.py 声明同名工具 dup（discover → duplicate_tool）。"""

from __future__ import annotations

from typing import Any

TOOL: dict[str, Any] = {
    "name": "dup",
    "description": "first 模块的同名工具",
    "parameters": {"type": "object", "properties": {}},
}


def call(args: dict[str, Any]) -> dict[str, Any]:
    """返回来源标记。"""
    return {"source": "first"}
