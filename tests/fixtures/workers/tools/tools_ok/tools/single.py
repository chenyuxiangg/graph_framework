"""形式 B 工具 fixture：单工具 TOOL + call(args)。"""

from __future__ import annotations

from typing import Any

TOOL: dict[str, Any] = {
    "name": "echo",
    "description": "回显 text 参数",
    "parameters": {
        "type": "object",
        "properties": {"text": {"type": "string"}},
        "required": ["text"],
    },
}


def call(args: dict[str, Any]) -> dict[str, Any]:
    """回显输入文本。"""
    return {"echo": args.get("text")}
