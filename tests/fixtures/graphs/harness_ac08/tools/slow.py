"""AC-08 工具 fixture（形式 B）：`slow` 睡 ``seconds`` 秒后返回。

声明 ``timeout_ms = 300``，用于验证「工具超时 → 杀进程组 → TOOL_ERROR 喂回 →
run 继续」语义（05 §5.6.5 / 05 §5.8）。
"""

from __future__ import annotations

import time
from typing import Any

TOOL: dict[str, Any] = {
    "name": "slow",
    "description": "睡眠 seconds 秒后返回 {\"slept\": seconds}；声明 300ms 工具超时",
    "parameters": {
        "type": "object",
        "properties": {
            "seconds": {"type": "number"},
        },
        "required": ["seconds"],
        "additionalProperties": False,
    },
    "timeout_ms": 300,
}


def call(args: dict[str, Any]) -> dict[str, Any]:
    """睡眠 ``args["seconds"]`` 秒后返回实际睡眠时长。"""
    seconds: float = float(args["seconds"])
    time.sleep(seconds)
    return {"slept": seconds}
