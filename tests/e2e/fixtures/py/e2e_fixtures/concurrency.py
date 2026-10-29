"""AC-12 前置 fixture：三个并行分支记录执行区间（start/end 追加到共享日志）。

测试按区间是否重叠 / 峰值并发数判断全局 `max_workers` 与每 run
`max_workers_per_run`（D4）是否生效；payload：``{"log_file": ..., "sleep": 0.6}``。

日志行格式：``<run_tag> <tag> <pid> <wall-clock> start|end``。``run_tag`` 由测试注入，
既用于输出区分，也保证多 run 共享日志时**不会因跨 run PID 复用把区间错配**
（REV-015 #10）；配对键 = ``(run_tag, tag, pid)``。
"""

from __future__ import annotations

import os
import time
from typing import Any

from zgraph_worker import WorkerContext

from ._payload import unwrap_start

DEFAULT_SLEEP_SECONDS: float = 0.6


def a(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """分支 a。"""
    return _run(inputs, "a")


def b(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """分支 b。"""
    return _run(inputs, "b")


def c(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """分支 c。"""
    return _run(inputs, "c")


def _run(inputs: dict[str, Any], tag: str) -> dict[str, Any]:
    """写 start 行 → sleep → 写 end 行（同一主机 wall clock 可直接比较）。

    ``run_tag`` 由 payload 注入并随输出返回，供多 run 隔离用例区分两个 run 的输出。
    """
    payload = unwrap_start(inputs)
    log_file = str(payload["log_file"])
    run_tag = str(payload.get("run_tag", "run"))
    sleep_seconds = float(payload.get("sleep", DEFAULT_SLEEP_SECONDS))
    _append(log_file, f"{run_tag} {tag} {os.getpid()} {time.time():.6f} start")
    time.sleep(sleep_seconds)
    _append(log_file, f"{run_tag} {tag} {os.getpid()} {time.time():.6f} end")
    return {"tag": tag, "run_tag": payload.get("run_tag")}


def _append(path: str, line: str) -> None:
    """单次 write 追加一行（行短，避免多进程交错）。"""
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(line + "\n")
