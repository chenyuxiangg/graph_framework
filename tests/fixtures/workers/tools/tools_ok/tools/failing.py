"""形式 B 工具 fixture：call 抛 ValueError（异常路径测试）。"""

from __future__ import annotations

from typing import Any

TOOL: dict[str, Any] = {
    "name": "explode",
    "description": "总是抛 ValueError(boom)",
    "parameters": {"type": "object", "properties": {}},
}


def call(args: dict[str, Any]) -> dict[str, Any]:
    """总是抛错，用于验证工具异常的结构化返回。"""
    raise ValueError("boom")
