"""示例 worker：回显输入并发送一条 progress 事件（M0 往返验收用）。"""

from __future__ import annotations

from typing import Any

from .ctx import WorkerContext


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """回显 ``inputs.message``；用于协议与生命周期测试。"""
    ctx.emit({"type": "progress", "percent": 100, "message": "echo"})
    return {"echo": inputs.get("message"), "node_id": ctx.node_id}
