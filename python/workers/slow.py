"""示例 worker：睡眠指定秒数，供 AC-06 超时（进程组 kill）与取消验收使用。"""

from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Any

from zgraph_worker import WorkerContext

from ._inputs import unwrap_port

CHECK_INTERVAL_S: float = 0.05
DEFAULT_SECONDS: float = 1.0


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """睡眠 ``seconds`` 秒，每 50ms 自检一次取消。

    输入：
        ``seconds``（number，可选，缺省 1.0）：睡眠秒数；也接受上游整包 output dict 形态
        （取其 ``seconds`` 字段）。
        ``pid_file``（string，可选）：非空时把本 worker 进程 PID 写入该文件，供外部断言
        进程组已被 kill。
    输出：
        ``{"slept": seconds, "cancelled": False}``；收到取消请求时抛 ``WorkerCancelled``
        且不返回结果。
    幂等性：
        睡眠本身不改变业务状态；可选 PID 文件覆盖写自身 PID，重复执行结果相同（最后一次
        执行者胜出）。相同输入得到相同输出，可安全重试。
    外部副作用：
        1) 若 ``pid_file`` 非空则写该文件（覆盖写，内容为十进制 PID）；
        2) 每 50ms 调用 ``ctx.check_cancelled()``，取消时抛 ``WorkerCancelled``。
        无网络、无其它文件写入。
    """
    raw_seconds: Any = unwrap_port(inputs, "seconds", "seconds")
    seconds: float = float(DEFAULT_SECONDS if raw_seconds is None else raw_seconds)
    raw_pid_file: Any = unwrap_port(inputs, "pid_file", "pid_file")
    if raw_pid_file:
        Path(str(raw_pid_file)).write_text(str(os.getpid()), encoding="utf-8")
    deadline: float = time.monotonic() + seconds
    while time.monotonic() < deadline:
        ctx.check_cancelled()
        time.sleep(CHECK_INTERVAL_S)
    return {"slept": seconds, "cancelled": False}
