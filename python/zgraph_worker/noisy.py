"""示例 worker：污染 stdout、制造 stderr 压力，用于隔离性验收。"""

from __future__ import annotations

import sys
from typing import Any

from .ctx import WorkerContext

STDERR_CHUNK: str = "x" * 1024
STDERR_LINES: int = 128


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """print 到 stdout（应进 stderr）+ 128 KiB stderr 压力，然后正常返回。"""
    print("user print must not pollute protocol")
    for _ in range(STDERR_LINES):
        sys.stderr.write(STDERR_CHUNK + "\n")
    sys.stderr.flush()
    return {"ok": True}
