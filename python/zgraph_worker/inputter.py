"""示例 worker：调用 input()（协议占用 stdin，应被禁用）。"""

from __future__ import annotations

from typing import Any

from .ctx import WorkerContext


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """``input()`` 应抛 RuntimeError（stdin 是协议通道）。"""
    value: str = input()  # noqa: A001 - 故意触发
    return {"value": value}
