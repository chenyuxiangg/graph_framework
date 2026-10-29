"""Dispatcher Protocol + DispatchResult dataclass。"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import Protocol

from src.kernel import Graph, Node

from ..state_manager import State


@dataclass(frozen=True)
class DispatchResult:
    """派发结果。"""

    pid: int           # worker pid；0 表示未启动
    dispatch_id: str   # uuid，方便追踪
    backend: str       # "process"


class Dispatcher(Protocol):
    """dispatch(node)：根据 node.proc 拉 worker 跑这个 proc。"""

    def dispatch(
        self,
        graph: Graph,
        state: State,
        node: Node,
        instance_id: str,
        work_dir: Path,
    ) -> DispatchResult:
        ...

    def alive(self, pid: int) -> bool:
        ...

    def wait(self, pid: int, timeout: float) -> int:
        ...