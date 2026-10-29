"""cycle 检测辅助。"""

from __future__ import annotations

from dataclasses import dataclass

from src.kernel import Graph, cycle_check

from ..state_manager import State


CYCLE_THRESHOLD = 100


@dataclass(frozen=True)
class CycleProgress:
    """cycle 进度诊断。"""

    max_count: int
    cycle_nodes: tuple[str, ...]
    over_threshold: bool


def detect_cycle_progress(graph: Graph, state: State) -> CycleProgress:
    """cycle 模式：调 kernel.cycle_check 拿所有环；统计 exit_cnt 是否超阈值。

    用 exit_cnt 而非 enter_cnt：只有 worker 真完成的次数算"进度"，in-flight 不算。
    """
    cycles = cycle_check(graph)
    cycle_nodes: set[str] = set()
    for c in cycles:
        cycle_nodes.update(c)
    if not cycle_nodes:
        return CycleProgress(max_count=0, cycle_nodes=(), over_threshold=False)
    max_count = max(state.exit_cnt.get(n, 0) for n in cycle_nodes)
    return CycleProgress(
        max_count=max_count,
        cycle_nodes=tuple(sorted(cycle_nodes)),
        over_threshold=max_count > CYCLE_THRESHOLD,
    )