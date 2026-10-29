"""Graph 聚合根与 GraphMode 枚举。"""

from collections import defaultdict
from dataclasses import dataclass
from enum import Enum
from functools import cached_property
from typing import Mapping, Sequence

from .edge import Edge
from .gate import Gate
from .node import Node
from .validators import validate


class GraphMode(str, Enum):
    """图的模式。"""

    DIRECTED_CYCLE = "directed_cycle"
    DIRECTED_NOCYCLE = "directed_nocycle"


class GraphBuildError(Exception):
    """Graph.build 构造失败。"""

    def __init__(self, message: str, path: str = "") -> None:
        super().__init__(message)
        self.path = path


@dataclass(frozen=True)
class Graph:
    """图模型聚合根。

    构造路径仅 Graph.build()。Graph 一旦构造则不可变，不支持运行时
    修改

    tick_period_s：调度器轮询周期（秒）。Orchestrator.run 在两次 tick 之间
    sleep 此时长，给 worker 子进程足够时间写产物 + record_exit，
    保证下次 tick 的 gate 评估能看见最新的 exit_cnt。
    """

    name: str
    graph_mode: GraphMode
    pre_handle: str
    post_handle: str
    nodes: tuple[Node, ...]
    gates: tuple[Gate, ...]
    edges: tuple[Edge, ...]
    tick_period_s: float

    @cached_property
    def node_index(self) -> Mapping[str, Node]:
        """name -> Node；首次访问时计算并缓存。"""
        return {n.name: n for n in self.nodes}

    @cached_property
    def gate_index(self) -> Mapping[str, Gate]:
        """name -> Gate；首次访问时计算并缓存。"""
        return {g.name: g for g in self.gates}

    @cached_property
    def edge_by_inode(self) -> Mapping[str, tuple[Edge, ...]]:
        """inode -> 该起点出边集合；首次访问时计算并缓存。"""
        raw: dict[str, list[Edge]] = defaultdict(list)
        for e in self.edges:
            raw[e.inode].append(e)
        return {k: tuple(v) for k, v in raw.items()}

    @cached_property
    def edge_by_gate(self) -> Mapping[str, tuple[Edge, ...]]:
        """gate -> 挂在该门上的边集合；首次访问时计算并缓存。"""
        raw: dict[str, list[Edge]] = defaultdict(list)
        for e in self.edges:
            raw[e.gate].append(e)
        return {k: tuple(v) for k, v in raw.items()}

    @staticmethod
    def build(
        name: str,
        graph_mode: GraphMode | str,
        pre_handle: str,
        post_handle: str,
        nodes: Sequence[Node],
        gates: Sequence[Gate],
        edges: Sequence[Edge],
        tick_period_s: float,
        existing_sop_names: Sequence[str] | None = None,
    ) -> "Graph":
        """构造 + 校验。失败抛 GraphBuildError。

        existing_sop_names: 已有 SOP 名集合，用于 E_DUP_GLOBAL 校验。
        派生索引（node_index / gate_index / edge_by_inode / edge_by_gate）
        首次访问时由 @cached_property 即时计算，无需构造时填充。

        tick_period_s：调度器轮询周期（秒）。Orchestrator.run 在两次 tick 之间
        sleep 此时长。loader 强制要求 JSON 显式声明（无默认值）——避免隐式
        hardcode 0.05 类短周期导致 gate 与 worker 竞态。
        """
        graph_mode = _to_graph_mode(graph_mode)
        _check_top_level_types(name, pre_handle, post_handle, nodes, gates, edges)

        nodes_t = tuple(nodes)
        gates_t = tuple(gates)
        edges_t = tuple(edges)

        tmp = Graph(
            name=name,
            graph_mode=graph_mode,
            pre_handle=pre_handle,
            post_handle=post_handle,
            nodes=nodes_t,
            gates=gates_t,
            edges=edges_t,
            tick_period_s=tick_period_s,
        )
        _raise_on_validation_errors(tmp, existing_sop_names)

        return Graph(
            name=name,
            graph_mode=graph_mode,
            pre_handle=pre_handle,
            post_handle=post_handle,
            nodes=nodes_t,
            gates=gates_t,
            edges=edges_t,
            tick_period_s=tick_period_s,
        )


def _to_graph_mode(value: GraphMode | str) -> GraphMode:
    """接受 GraphMode 或字符串，返回 GraphMode。

    - 已是 GraphMode：原样返回
    - 字符串：必须命中 GraphMode 的取值；否则抛 GraphBuildError
    - 其他类型：抛 GraphBuildError
    """
    if isinstance(value, GraphMode):
        return value
    if isinstance(value, str):
        try:
            return GraphMode(value)
        except ValueError as exc:
            raise GraphBuildError(
                f"graph_mode 非法: {value!r}", "graph_mode"
            ) from exc
    raise GraphBuildError(
        f"graph_mode 类型非法: {type(value).__name__}", "graph_mode"
    )


def _check_top_level_types(
    name: object,
    pre_handle: object,
    post_handle: object,
    nodes: object,
    gates: object,
    edges: object,
) -> None:
    """顶层字段类型 / 形态校验。"""
    if not isinstance(name, str) or not name:
        raise GraphBuildError("name 必须是非空字符串", "name")
    if not isinstance(pre_handle, str):
        raise GraphBuildError("pre_handle 必须是字符串", "pre_handle")
    if not isinstance(post_handle, str):
        raise GraphBuildError("post_handle 必须是字符串", "post_handle")
    if not isinstance(nodes, (tuple, list)):
        raise GraphBuildError("nodes 必须是序列", "nodes")
    if not isinstance(gates, (tuple, list)):
        raise GraphBuildError("gates 必须是序列", "gates")
    if not isinstance(edges, (tuple, list)):
        raise GraphBuildError("edges 必须是序列", "edges")


def _raise_on_validation_errors(
    graph: "Graph", existing_sop_names: Sequence[str] | None
) -> None:
    """跨字段一致性校验；error 抛 GraphBuildError，warning 静默。"""
    issues = validate(graph, existing_sop_names=existing_sop_names)
    errors = [i for i in issues if i.level == "error"]
    if not errors:
        return
    first = errors[0]
    raise GraphBuildError(f"{first.code}: {first.message}", first.path)