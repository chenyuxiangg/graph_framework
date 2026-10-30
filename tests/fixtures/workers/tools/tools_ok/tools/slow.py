"""形式 B 工具 fixture：按参数 seconds 睡眠（用于超时/取消路径，声明 300ms 超时）。"""

from __future__ import annotations

import time
from typing import Any

TOOL: dict[str, Any] = {
    "name": "sleepy",
    "description": "睡眠 seconds 秒后返回 {slept}",
    "parameters": {
        "type": "object",
        "properties": {"seconds": {"type": "number", "minimum": 0}},
        "required": ["seconds"],
    },
    "timeout_ms": 300,
}


def call(args: dict[str, Any]) -> dict[str, Any]:
    """睡眠并返回实际睡眠秒数。"""
    seconds: float = float(args.get("seconds", 0))
    time.sleep(seconds)
    return {"slept": seconds}
