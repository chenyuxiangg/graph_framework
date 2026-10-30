"""坏 fixture：同时声明 TOOL 与 TOOLS（discover → invalid_declaration）。"""

from __future__ import annotations

from typing import Any

TOOL: dict[str, Any] = {
    "name": "both",
    "description": "同时声明 TOOL 与 TOOLS",
    "parameters": {"type": "object", "properties": {}},
}
TOOLS: list[dict[str, Any]] = [TOOL]


def call(name: str, args: dict[str, Any]) -> dict[str, Any]:
    """不可达：声明形式非法，discover 在调用前即报错。"""
    return {"ok": True}
