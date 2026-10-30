"""AC-12 fixture：多 run 并发隔离的进程区间 + ``ctx.user_id`` / ``ctx.get_var`` 回显。

分支节点 `a`/`b`/`c` 沿用 AC-12 前置（`concurrency.py`）的区间日志协议：
每次执行把 ``start``/``end`` 墙钟追加到共享日志，测试按区间求「全局峰值」与
「每 run 峰值」，用真实 Python 子进程证明全局 `max_workers` 与每 run
`max_workers_per_run`（D4）生效。

与 `concurrency.py` 的差异（M5 全栈隔离断言需要）：

- 输出额外回显 ``ctx.user_id`` 与 ``ctx.get_var("tenant")``，用于断言 10 个并发 run
  的 `user_id` / `global_vars` 互不串（AC-12）；
- 额外的 ``echo`` 节点给「含 agent_call 的 run」提供一条真实的 Python 子进程区间，
  并把 run 级 payload 透传给 agent 节点（输出带 ``message`` 键）。

日志行格式：``<run_tag> <tag> <pid> <wall-clock> start|end``。
"""

from __future__ import annotations

import os
import time
from typing import Any

from zgraph_worker import WorkerContext

from ._payload import unwrap_start

DEFAULT_SLEEP_SECONDS: float = 0.35


def a(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """三并发分支 a。"""
    return _run_branch(inputs, ctx, "a")


def b(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """三并发分支 b。"""
    return _run_branch(inputs, ctx, "b")


def c(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """三并发分支 c。"""
    return _run_branch(inputs, ctx, "c")


def echo(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """agent 图的前置节点：记录区间并把 payload 的 ``text`` 透传给 agent。"""
    payload = _echo_payload(inputs)
    log_file = str(payload["log_file"])
    run_tag = str(payload.get("run_tag", "run"))
    started = time.time()
    _append(log_file, f"{run_tag} echo {os.getpid()} {started:.6f} start")
    _append(log_file, f"{run_tag} echo {os.getpid()} {time.time():.6f} end")
    text: Any = payload.get("text")
    return {
        "message": text if isinstance(text, str) else str(text),
        "run_tag": payload.get("run_tag"),
        "user_id": ctx.user_id,
        "tenant": ctx.get_var("tenant"),
    }


def _run_branch(
    inputs: dict[str, Any], ctx: WorkerContext, tag: str
) -> dict[str, Any]:
    """写 start 行 → sleep → 写 end 行 → 回显 run 级 user_id/global 变量。"""
    payload = unwrap_start(inputs)
    log_file = str(payload["log_file"])
    run_tag = str(payload.get("run_tag", "run"))
    sleep_seconds = float(payload.get("sleep", DEFAULT_SLEEP_SECONDS))
    _append(log_file, f"{run_tag} {tag} {os.getpid()} {time.time():.6f} start")
    time.sleep(sleep_seconds)
    _append(log_file, f"{run_tag} {tag} {os.getpid()} {time.time():.6f} end")
    return {
        "tag": tag,
        "run_tag": payload.get("run_tag"),
        "user_id": ctx.user_id,
        "tenant": ctx.get_var("tenant"),
    }


def _echo_payload(inputs: dict[str, Any]) -> dict[str, Any]:
    """从 ``message`` 端口取出 start 整包 output 内的测试 payload。

    通路：``start`` 输出 ``{"message": payload}`` → 整包投递到 echo 的 ``message``
    端口，因此 echo 看到 ``{"message": {"message": payload}}``。
    """
    value: Any = inputs.get("message")
    if isinstance(value, dict):
        inner: Any = value.get("message")
        if isinstance(inner, dict):
            return inner
        return value
    return {}


def _append(path: str, line: str) -> None:
    """单次 write 追加一行（行短，避免多进程交错）。"""
    with open(path, "a", encoding="utf-8") as handle:
        handle.write(line + "\n")
