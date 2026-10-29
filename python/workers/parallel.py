"""示例 worker：``valid_parallel.json`` 的并行分支 ``a`` / ``b``（AC-09）。"""

from __future__ import annotations

import time
from typing import Any

from zgraph_worker import WorkerContext

PARALLEL_SLEEP_S: float = 0.2


def a(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """并行分支 a：睡眠约 0.2s 后返回分支标记，供「Running 区间重叠」断言。

    输入：
        ``inputs``：框架注入的全部已声明端口（fixture 中为 ``x``）。
    输出：
        ``{"worker": "a", "x": inputs.get("x")}``。
    幂等性：
        只读输入 + 固定时长睡眠，相同输入输出相同，可安全重试。
    外部副作用：
        无（不写文件、不发网络、不写全局状态）；仅 sleep 与一条可丢 progress 事件。
    """
    ctx.emit({"type": "progress", "percent": 50, "message": "branch a"})
    time.sleep(PARALLEL_SLEEP_S)
    return {"worker": "a", "x": inputs.get("x")}


def b(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """并行分支 b：睡眠约 0.2s 后返回分支标记，供「Running 区间重叠」断言。

    输入：
        ``inputs``：框架注入的全部已声明端口（fixture 中为 ``x``）。
    输出：
        ``{"worker": "b", "x": inputs.get("x")}``。
    幂等性：
        只读输入 + 固定时长睡眠，相同输入输出相同，可安全重试。
    外部副作用：
        无（不写文件、不发网络、不写全局状态）；仅 sleep 与一条可丢 progress 事件。
    """
    ctx.emit({"type": "progress", "percent": 50, "message": "branch b"})
    time.sleep(PARALLEL_SLEEP_S)
    return {"worker": "b", "x": inputs.get("x")}
