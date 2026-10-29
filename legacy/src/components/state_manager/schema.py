"""State dataclass：SOP 实例运行时快照。"""

from __future__ import annotations

from dataclasses import dataclass, field, replace
from typing import Mapping


def _empty_str_map() -> Mapping[str, str]:
    return {}


def _empty_int_map() -> Mapping[str, int]:
    return {}


def _empty_files_map() -> Mapping[str, tuple[str, ...]]:
    return {}


@dataclass(frozen=True)
class State:
    """不可变 SOP 实例状态。

    enter_cnt / exit_cnt：节点调度计数（per-node）。
    - enter_cnt[node]：worker 进入 node.proc 的次数（dispatch 实际触达 proc）
    - exit_cnt[node]：worker 完成 node.proc 的次数（record_completion 已写盘）
    - 正常情况下 enter_cnt == exit_cnt；不等说明有 in-flight worker（wait 或 leak）
    """

    sop_name: str
    instance_id: str
    graph_name: str
    current_node: str | None = None
    history: tuple[tuple[str, str], ...] = ()
    triggered_edges: Mapping[str, str] = field(default_factory=_empty_str_map)
    enter_cnt: Mapping[str, int] = field(default_factory=_empty_int_map)
    exit_cnt: Mapping[str, int] = field(default_factory=_empty_int_map)
    last_output: Mapping[str, tuple[str, ...]] = field(default_factory=_empty_files_map)

    def evolve(self, **changes) -> "State":
        """生成新 State；不影响当前对象。"""
        return replace(self, **changes)