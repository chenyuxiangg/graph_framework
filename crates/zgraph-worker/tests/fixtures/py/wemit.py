"""executor 集成测试 fixture：emit 流式事件（逐类型覆盖协议 §2 的 emit 表）。

用于验证 supervisor 逐帧回调 → `ctx.emit(StreamingEvent)` → 内核 trace sink。
"""

from __future__ import annotations

from typing import Any

from zgraph_worker import WorkerContext


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """发射全部 5 类流式事件后返回。"""
    ctx.emit({"type": "token", "text": "tok-1"})
    ctx.emit({"type": "reasoning", "text": "why-1"})
    ctx.emit(
        {
            "type": "tool_call_start",
            "call_id": "call-1",
            "tool_name": "search",
            "args_summary": {"q": "x"},
        }
    )
    ctx.emit(
        {
            "type": "tool_call_end",
            "call_id": "call-1",
            "tool_name": "search",
            "ok": True,
            "result_summary": {"hits": 1},
        }
    )
    ctx.emit({"type": "progress", "percent": 40, "message": "step"})
    return {"ok": True, "emit_count": 5}
