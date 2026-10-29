"""AC-06 进程组 kill fixture：worker 自己再派一个**孙进程**后长睡。

验证 D1「超时/取消对进程组发 SIGKILL，孙进程不残留」：worker 与孙进程都必须
在 kill 后从进程表消失（测试侧轮询 ``kill(pid, 0)``）。

pid 文件来源（按优先级）：``inputs.x`` 里的 ``worker_pid_file`` / ``child_pid_file``，
或环境变量 ``ZGRAPH_FIXTURE_PID_FILE`` / ``ZGRAPH_FIXTURE_CHILD_PID_FILE``。
"""

from __future__ import annotations

import os
import subprocess
import sys
import time
from typing import Any

from zgraph_worker import WorkerContext

from ._payload import unwrap_start

SLEEP_SECONDS: float = 60.0
POLL_INTERVAL_SECONDS: float = 0.05


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """写自身 pid → 派孙进程并写其 pid → 长睡（等待被内核 kill 进程组）。

    输入：``x`` 端口（整包广播）内的 ``worker_pid_file`` / ``child_pid_file``。
    输出：正常返回 ``{"finished": True}``（超时用例下永远走不到）。
    幂等性：仅覆盖写 pid 文件；重复执行结果相同（最后一次执行者胜出）。
    外部副作用：写两个 pid 文件、派生一个 ``sleep`` 孙进程。
    """
    payload = unwrap_start(inputs)
    worker_pid_file = str(
        payload.get("worker_pid_file") or os.environ.get("ZGRAPH_FIXTURE_PID_FILE") or ""
    )
    if worker_pid_file:
        with open(worker_pid_file, "w", encoding="utf-8") as handle:
            handle.write(str(os.getpid()))
    child_pid_file = str(
        payload.get("child_pid_file") or os.environ.get("ZGRAPH_FIXTURE_CHILD_PID_FILE") or ""
    )
    if child_pid_file:
        child = subprocess.Popen(  # noqa: S603 - fixture 只在自己的进程组里派 sleep
            [sys.executable, "-c", "import time\ntime.sleep(60)"]
        )
        with open(child_pid_file, "w", encoding="utf-8") as handle:
            handle.write(str(child.pid))
    deadline = time.monotonic() + SLEEP_SECONDS
    while time.monotonic() < deadline:
        ctx.check_cancelled()
        time.sleep(POLL_INTERVAL_SECONDS)
    return {"finished": True}
