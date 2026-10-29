"""示例 worker：可被取消的忙等（超时/取消验收用）。"""

from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Any

from .ctx import WorkerContext

CHECK_INTERVAL_S: float = 0.05


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """睡眠 ``inputs.seconds`` 秒；每 50ms 检查一次取消。"""
    pid_file: Any = inputs.get("pid_file")
    if pid_file:
        Path(str(pid_file)).write_text(str(os.getpid()), encoding="utf-8")
    seconds: float = float(inputs.get("seconds", 1.0))
    deadline: float = time.monotonic() + seconds
    while time.monotonic() < deadline:
        ctx.check_cancelled()
        time.sleep(CHECK_INTERVAL_S)
    return {"slept": seconds}
