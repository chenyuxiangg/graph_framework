"""SOP 终止判定。"""

from __future__ import annotations

from pathlib import Path

from src.kernel import Graph

from .schema import State


def is_sop_done(graph: Graph, state: State) -> bool:
    """SOP 终止 = current_node 是 sink 节点 AND（若 sink 有 oport）last_output 有产物。

    sink 节点 oport 非空时，必须等 worker 完成 record_completion（写 last_output）
    才能算 done——否则 current_node 已切到 sink 但 worker 还在异步跑，产物未落盘。
    sink 节点 oport 为空时（无产物），worker 完成即 done（last_output 自然为空）。
    """
    if state.current_node is None:
        return False
    node = graph.node_index.get(state.current_node)
    if node is None:
        return False
    if not node.attr.is_sink:
        return False
    # sink 有 oport → 必须 last_output 有产物（worker record_completion 写过）
    if node.oport:
        return bool(state.last_output.get(state.current_node))
    return True


def run_post_handle(graph: Graph, state: State, base_dir: Path) -> bool:
    """SOP 完成时调 graph.post_handle（空串 = 跳过）。返回 post_handle 返回值。"""
    from src.kernel import resolve_op

    if not graph.post_handle:
        return True
    fn = resolve_op(graph.post_handle)
    return bool(fn(base_dir=base_dir, state=state, graph=graph))