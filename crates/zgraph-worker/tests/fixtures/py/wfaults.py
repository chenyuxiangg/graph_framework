"""executor 集成测试 fixture：各种失败模式（每个函数对应一个 python_node）。

失败语义见 `doc/spec-v1.2/04-节点接口与扩展点.md` §4.3.4 与
`doc/architecture-v2.md` §8.2；错误码映射由 `zgraph-worker` 的 executor 负责。
"""

from __future__ import annotations

import os
import signal
import subprocess
import sys
import time
from typing import Any

from zgraph_worker import BusinessError, WorkerContext, WorkerError


class BizError(WorkerError):
    """自定义业务码（验证 code 原样进入 `BusinessError`）。"""

    code = "BIZ_ORDER_NOT_FOUND"


def value_error(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """未捕获异常 → NODE_PYTHON_EXCEPTION。"""
    raise ValueError("boom: value_error")


def ok(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """正常返回（并发隔离测试中的健康分支）。"""
    return {"ok": True}


def business_error(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """SDK 业务异常 → NODE_BUSINESS_ERROR。"""
    raise BusinessError("order not found")


def biz_code(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """自定义业务码 → BusinessError{code: BIZ_ORDER_NOT_FOUND}。"""
    raise BizError("custom business code")


def exit_zero(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """用户代码直接退出进程（无 result）→ NODE_WORKER_PROTOCOL。"""
    os._exit(0)


def kill_self(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """用户代码被信号杀死 → NODE_WORKER_CRASH。"""
    os.kill(os.getpid(), signal.SIGKILL)
    raise AssertionError("unreachable")


def sleep_forever(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """超时 fixture：写 pid 文件、派孙进程、长睡（验证进程组 kill）。

    入口注入整包广播：``inputs = {"x": {"pid_file": ..., "child_pid_file": ...}}``。
    """
    payload = _broadcast_payload(inputs)
    pid_value = str(payload.get("pid_file") or os.environ.get("ZGRAPH_FIXTURE_PID_FILE") or "")
    if pid_value:
        with open(pid_value, "w", encoding="utf-8") as handle:
            handle.write(str(os.getpid()))
    child_file = str(payload.get("child_pid_file") or os.environ.get("ZGRAPH_FIXTURE_CHILD_PID_FILE") or "")
    if child_file:
        child = subprocess.Popen(  # noqa: S603 - fixture 只在自己的进程组里派 sleep
            [sys.executable, "-c", "import time\ntime.sleep(60)"]
        )
        with open(child_file, "w", encoding="utf-8") as handle:
            handle.write(str(child.pid))
    time.sleep(60)
    return {"finished": False}


def context_probe(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """回传 ctx 关键字段，验证 run 载荷（seed/user_id/global_vars/deadline，D2/D5）。"""
    return {
        "seed": ctx.seed,
        "user_id": ctx.user_id,
        "greeting": ctx.get_var("greeting"),
        "has_deadline": ctx.deadline_remaining_ms() > 0,
        "node_id": ctx.node_id,
    }



def _broadcast_payload(inputs: dict[str, Any]) -> dict[str, Any]:
    """v1 整包广播：上游 output 整体投递到目标端口，``start`` 输出是 ``{"x": obj}``。"""
    value: Any = inputs.get("x") or {}
    if isinstance(value, dict) and isinstance(value.get("x"), dict):
        return value["x"]
    return value
