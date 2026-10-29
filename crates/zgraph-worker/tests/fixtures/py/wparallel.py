"""executor 并发上限 fixture：记录执行区间（start/end 追加到共享日志文件）。

两个并行分支节点分别调用 ``a``/``b``，入口注入 ``{"x": {"log_file": ..., "sleep": ...}}``；
测试按区间是否重叠判断全局/每 run 并发上限是否生效。
"""

from __future__ import annotations

import os
import time
from typing import Any

from zgraph_worker import WorkerContext


def a(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """分支 a。"""
    return _run(inputs, "a")


def b(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """分支 b。"""
    return _run(inputs, "b")


def _run(inputs: dict[str, Any], tag: str) -> dict[str, Any]:
    """写 start 行 → sleep → 写 end 行（同一主机 wall clock 可直接比较）。"""
    payload = _broadcast_payload(inputs)
    log_file = str(payload["log_file"])
    sleep_seconds = float(payload.get("sleep", 0.5))
    started = time.time()
    _append(log_file, f"{tag} {os.getpid()} {started:.6f} start")
    time.sleep(sleep_seconds)
    _append(log_file, f"{tag} {os.getpid()} {time.time():.6f} end")
    return {"ok": True, "tag": tag}


def _append(path: str, line: str) -> None:
    """单次 write 追加一行（多进程共享日志，行很短，避免交错）。"""
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(line + "\n")


def _broadcast_payload(inputs: dict[str, Any]) -> dict[str, Any]:
    """v1 整包广播：``start`` 输出 ``{"x": obj}`` 被整体投递到目标端口 ``x``。"""
    value: Any = inputs.get("x") or {}
    if isinstance(value, dict) and isinstance(value.get("x"), dict):
        return value["x"]
    return value
