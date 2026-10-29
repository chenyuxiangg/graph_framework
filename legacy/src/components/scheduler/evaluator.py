"""evaluator：gate.op 求值 + match_gate + should_run_cmd + select_next_edge + run_edge_cmd。"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from src.kernel import Edge, Gate, Graph, resolve_op

from ..state_manager import State


def evaluate_gates(
    graph: Graph, current_node: str, base_dir: Path, state: State
) -> dict[str, Any]:
    """对 current_node 所有出边涉及的 gate 求值。返回 {gate_name: gate_value}。

    同一 gate 多次求值复用同一结果。
    """
    out: dict[str, Any] = {}
    for edge in graph.edges:
        if edge.inode != current_node:
            continue
        if edge.gate in out:
            continue
        gate = graph.gate_index.get(edge.gate)
        if gate is None:
            continue
        try:
            fn = resolve_op(gate.op)
            value = fn(base_dir=base_dir, state=state, graph=graph, gate=gate)
        except Exception as exc:
            raise GateEvalError(f"gate.op '{gate.op}' 异常: {exc}") from exc
        out[edge.gate] = value
    return out


def match_gate(edge: Edge, gate_values: dict[str, Any]) -> bool:
    """gate_values[edge.gate] == edge.gate_value。"""
    return gate_values.get(edge.gate, object()) == edge.gate_value


def should_run_cmd(edge: Edge, state: State) -> bool:
    """cmd 是否在本次 tick 跑：
    - driver=tick → True
    - driver=manual → state.triggered_edges 包含 edge.name 才跑
    """
    if edge.driver == "tick":
        return True
    return edge.name in state.triggered_edges


def select_next_edge(
    graph: Graph, current_node: str, gate_values: dict[str, Any], state: State
) -> Edge | None:
    """选下一条推进边：match_gate + should_run_cmd 命中第一条。

    向后兼容便捷封装：本函数取 select_next_edges 的第一个元素。
    """
    edges = select_next_edges(graph, current_node, gate_values, state)
    return edges[0] if edges else None


def select_next_edges(
    graph: Graph, current_node: str, gate_values: dict[str, Any], state: State
) -> list[Edge]:
    """返回所有匹配 (match_gate + should_run_cmd) 的边。

    - 全部 parallel=True 时返回整组（parallel 语义，一次 tick 内并行 dispatch）。
    - 否则单条返回首条命中（向后兼容）。
    - 混合组（部分 parallel=True + 部分 parallel=False）由 validator 拦截至
      加载阶段，运行时不会出现：若出现，本函数退化为首条命中（fallback 安全）。
    """
    matched: list[Edge] = []
    for edge in graph.edges:
        if edge.inode != current_node:
            continue
        if not match_gate(edge, gate_values):
            continue
        if not should_run_cmd(edge, state):
            continue
        matched.append(edge)
    if not matched:
        return []
    if all(e.parallel for e in matched):
        return matched
    return [matched[0]]


def run_edge_cmd(graph: Graph, edge: Edge, state: State, base_dir: Path) -> None:
    """调 resolve_op(edge.cmd) 跑副作用动作。"""
    if not edge.cmd:
        return
    try:
        fn = resolve_op(edge.cmd)
        fn(base_dir=base_dir, state=state, graph=graph, edge=edge)
    except Exception as exc:
        raise EdgeCmdError(f"edge.cmd '{edge.cmd}' 异常: {exc}") from exc


class GateEvalError(Exception):
    """gate.op 求值异常。"""


class EdgeCmdError(Exception):
    """edge.cmd 执行异常。"""