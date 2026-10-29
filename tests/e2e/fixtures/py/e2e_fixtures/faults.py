"""故障注入 fixture：未捕获异常 / 信号自杀（AC-05/AC-10 的崩溃路径）。

协议层故障（无 result、重复 result、result 后不退出、hello 超时）走
``tests/e2e/fixtures/workers/*.py`` 的 raw NDJSON 脚本（绕过 SDK，直接注入非法行为）。
"""

from __future__ import annotations

import os
import signal
from typing import Any

from zgraph_worker import WorkerContext


def value_error(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """未捕获异常 → ``NODE_PYTHON_EXCEPTION``（Crashed，run Failed）。"""
    raise ValueError("boom-e2e: value_error")


def kill_self(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """用户代码被信号杀死 → ``NODE_WORKER_CRASH``（Crashed，run Failed）。"""
    os.kill(os.getpid(), signal.SIGKILL)
    raise AssertionError("unreachable: SIGKILL must terminate the worker")
