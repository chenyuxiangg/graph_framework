"""测试 fixture：故意返回非 dict，用于 NODE_RETURN_NOT_DICT 验收。"""

from __future__ import annotations

from typing import Any

from zgraph_worker.ctx import WorkerContext


def run(inputs: dict[str, Any], ctx: WorkerContext) -> int:
    """返回 int（非法输出）。"""
    return 42
