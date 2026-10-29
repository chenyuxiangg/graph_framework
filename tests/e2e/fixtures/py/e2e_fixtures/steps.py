"""AC-07/AC-10 fixture：两节点串行步骤，各自写 pid 文件后睡眠并原样透传 payload。

``first`` → ``second`` 的输入端口收到 first 的**整包 output**（payload 原样透传），
因此两个节点都能读到 ``sleep`` 与各自 pid 文件路径；测试用「pid 文件是否出现/变化」
判断真实 worker 是否在飞、暂停期间是否被派发。
"""

from __future__ import annotations

import os
import time
from typing import Any

from zgraph_worker import WorkerContext

from ._payload import unwrap_start

DEFAULT_SLEEP_SECONDS: float = 0.2
POLL_INTERVAL_SECONDS: float = 0.05


def _step(inputs: dict[str, Any], ctx: WorkerContext, pid_file_key: str) -> dict[str, Any]:
    """写 pid 文件 → 睡眠 ``sleep`` 秒（期间自检取消）→ 原样透传 payload。"""
    payload = unwrap_start(inputs)
    pid_file = str(payload.get(pid_file_key) or "")
    if pid_file:
        with open(pid_file, "w", encoding="utf-8") as handle:
            handle.write(str(os.getpid()))
    seconds = float(payload.get("sleep", DEFAULT_SLEEP_SECONDS))
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        ctx.check_cancelled()
        time.sleep(POLL_INTERVAL_SECONDS)
    return payload


def first(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """第一步：写 ``first_pid_file`` 后睡眠，返回 payload（供 second 继续）。"""
    return _step(inputs, ctx, "first_pid_file")


def second(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """第二步：写 ``second_pid_file`` 后睡眠，返回 payload。"""
    return _step(inputs, ctx, "second_pid_file")
