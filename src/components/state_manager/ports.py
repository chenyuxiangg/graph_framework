"""port 记录辅助：iport 满足 / oport 产出映射到 State 字段。"""

from __future__ import annotations

from pathlib import Path

from src.kernel import Node, iport_satisfied, oport_produced
from src.kernel.ports import expand_iport_paths

from .schema import State


def is_input_ready(graph, node: Node, base_dir: Path) -> bool:
    """任一组 iport 满足即 True（用 kernel iport_satisfied）。"""
    return bool(iport_satisfied(node, base_dir))


def output_produced_groups(graph, node: Node, base_dir: Path) -> tuple[int, ...]:
    """返回已产出的 oport 组索引（OR）。"""
    return oport_produced(node, base_dir)


def record_output_produced(state: State, node_name: str, group_idx: int) -> State:
    """把 group_idx 对应的所有文件路径写入 last_output[node_name]。"""
    return state.evolve(last_output={**state.last_output, node_name: (f"group:{group_idx}",)})


def expand_iport_paths_for_node(node: Node, base_dir: Path) -> tuple[tuple[Path, ...], ...]:
    return expand_iport_paths(node, base_dir)