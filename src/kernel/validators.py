"""跨字段一致性校验。

返回 list[Issue]，不抛异常；由调用方决定是否阻断。
"""

from dataclasses import dataclass
from typing import TYPE_CHECKING, Iterable, Literal

from .traversal import cycle_check

if TYPE_CHECKING:
    from .graph import Graph


LEVEL = Literal["debug", "info", "warning", "error"]


@dataclass(frozen=True)
class Issue:
    """一条校验问题。

    code    形如 E_DUP_NAME / W_CYCLE_ON_NOCYCLE，E=error 阻断，W=warning 仅记录。
    level   四级之一：debug / info / warning / error。
    message 人读描述。
    path    Pythonic 风格字符串定位，例 "nodes[2].name"。支持 tuple 输入自动转 str。
    """

    code: str
    level: LEVEL
    message: str
    path: str

    def __init__(
        self,
        code: str,
        level: LEVEL,
        message: str,
        path: str | tuple[str | int, ...],
    ) -> None:
        object.__setattr__(self, "code", code)
        object.__setattr__(self, "level", level)
        object.__setattr__(self, "message", message)
        object.__setattr__(self, "path", _path_to_str(path))


def _path_to_str(path: str | tuple[str | int, ...]) -> str:
    """tuple 路径转 Pythonic 字符串。例 ("nodes", 2, "name") -> "nodes[2].name"。"""
    if isinstance(path, str):
        return path
    if not path:
        return ""
    parts: list[str] = []
    for idx, seg in enumerate(path):
        if isinstance(seg, int):
            parts.append(f"[{seg}]")
        else:
            if idx > 0:
                parts.append(f".{seg}")
            else:
                parts.append(seg)
    return "".join(parts)


def validate(
    graph: "Graph",
    existing_sop_names: Iterable[str] | None = None,
) -> list[Issue]:
    """跨字段一致性校验，返回 Issue 列表。

    existing_sop_names: 调用方传入的已有 SOP 名集合，用于 E_DUP_GLOBAL 校验。
                       传 None 则跳过该项校验。
    """
    issues: list[Issue] = []
    _check_dup_names(graph, issues)
    _check_global_name(graph, existing_sop_names, issues)
    _check_src_uniqueness(graph, issues)
    _check_sink_uniqueness(graph, issues)
    _check_edge_endpoints(graph, issues)
    _check_gate_refs_and_values(graph, issues)
    _check_parallel_mixed(graph, issues)
    _check_cycle_on_nocycle(graph, issues)
    return issues


def _check_global_name(
    graph: "Graph",
    existing_sop_names: Iterable[str] | None,
    issues: list[Issue],
) -> None:
    """SOP name 跨图唯一。"""
    if existing_sop_names is None:
        return
    if graph.name not in existing_sop_names:
        return
    issues.append(
        Issue(
            code="E_DUP_GLOBAL",
            level="error",
            message=f"SOP name 「{graph.name}」 已被注册",
            path=("name",),
        )
    )


def _check_src_uniqueness(graph: "Graph", issues: list[Issue]) -> None:
    """src 节点有且仅有一个。"""
    src_nodes = [n for n in graph.nodes if n.attr.is_src]
    if len(src_nodes) == 0:
        issues.append(
            Issue(
                code="E_NO_SRC",
                level="error",
                message="图中没有 is_src=true 的节点",
                path=("nodes",),
            )
        )
        return
    if len(src_nodes) == 1:
        return
    for n in src_nodes:
        issues.append(
            Issue(
                code="E_NO_SRC",
                level="error",
                message=f"多个 src 节点: {n.name}",
                path=("nodes", _node_position(graph, n.name), "attr", "is_src"),
            )
        )


def _check_sink_uniqueness(graph: "Graph", issues: list[Issue]) -> None:
    """sink 节点有且仅有一个。"""
    sink_nodes = [n for n in graph.nodes if n.attr.is_sink]
    if len(sink_nodes) == 0:
        issues.append(
            Issue(
                code="E_MULTI_SINK",
                level="error",
                message="图中没有 is_sink=true 的节点",
                path=("nodes",),
            )
        )
        return
    if len(sink_nodes) == 1:
        return
    for n in sink_nodes:
        issues.append(
            Issue(
                code="E_MULTI_SINK",
                level="error",
                message=f"多个 sink 节点: {n.name}",
                path=("nodes", _node_position(graph, n.name), "attr", "is_sink"),
            )
        )


def _check_edge_endpoints(graph: "Graph", issues: list[Issue]) -> None:
    """边端点必须指向存在的节点。"""
    node_names = {n.name for n in graph.nodes}
    for idx, edge in enumerate(graph.edges):
        if edge.inode not in node_names:
            issues.append(
                Issue(
                    code="E_BAD_EDGE_ENDPOINT",
                    level="error",
                    message=f"edge 「{edge.name}」 的 inode 「{edge.inode}」 不存在",
                    path=("edges", idx, "inode"),
                )
            )
        if edge.onode not in node_names:
            issues.append(
                Issue(
                    code="E_BAD_EDGE_ENDPOINT",
                    level="error",
                    message=f"edge 「{edge.name}」 的 onode 「{edge.onode}」 不存在",
                    path=("edges", idx, "onode"),
                )
            )


def _check_gate_refs_and_values(graph: "Graph", issues: list[Issue]) -> None:
    """门的引用、gate_value 在 enum_dir 内、同 gate 同 value 不重复。

    放宽：多边共享 (gate, gate_value) 仅在所有成员边 parallel=True 时允许。
    否则保持原 E_MULTI_MATCH_GATE 报错语义。
    """
    gate_map = {g.name: g for g in graph.gates}
    gate_value_groups: dict[tuple[str, object], list] = {}
    for idx, edge in enumerate(graph.edges):
        gate = gate_map.get(edge.gate)
        if gate is None:
            issues.append(
                Issue(
                    code="E_BAD_GATE_REF",
                    level="error",
                    message=f"edge 「{edge.name}」 引用的 gate 「{edge.gate}」 不存在",
                    path=("edges", idx, "gate"),
                )
            )
            continue
        if edge.gate_value not in gate.enum_dir:
            issues.append(
                Issue(
                    code="E_GATE_VALUE_NOT_IN_ENUM",
                    level="error",
                    message=(
                        f"edge 「{edge.name}」 的 gate_value 「{edge.gate_value}」 "
                        f"不在 gate 「{gate.name}」 的 enum_dir {gate.enum_dir} 中"
                    ),
                    path=("edges", idx, "gate_value"),
                )
            )
        key = (edge.gate, edge.gate_value)
        gate_value_groups.setdefault(key, []).append(edge)

    for (gate_name, gv), group in gate_value_groups.items():
        if len(group) <= 1:
            continue
        if all(e.parallel for e in group):
            continue  # 整组 parallel → 允许
        issues.append(
            Issue(
                code="E_MULTI_MATCH_GATE",
                level="error",
                message=(
                    f"gate 「{gate_name}」 的 gate_value={gv!r} 上挂了 {len(group)} 条边"
                ),
                path=("gates", _gate_position(graph, gate_name), "enum_dir"),
            )
        )


def _check_parallel_mixed(graph: "Graph", issues: list[Issue]) -> None:
    """同一 inode 的匹配边集合（gate+gate_value 相同视为同一匹配组）必须
    全 parallel 或全非 parallel。混合（部分 parallel=True + 部分 parallel=False）
    报 E_PARALLEL_MIXED。

    静态检查：按 inode 分组，再按 (gate, gate_value) 分组检查是否 mixed。
    """
    from .edge import Edge  # local import to avoid cycles

    by_inode: dict[str, list[Edge]] = {}
    for edge in graph.edges:
        by_inode.setdefault(edge.inode, []).append(edge)

    for inode, edges in by_inode.items():
        groups: dict[tuple[str, object], list[Edge]] = {}
        for edge in edges:
            groups.setdefault((edge.gate, edge.gate_value), []).append(edge)
        for (gate_name, gv), group in groups.items():
            if len(group) <= 1:
                continue
            parallel_count = sum(1 for e in group if e.parallel)
            non_parallel_count = len(group) - parallel_count
            if parallel_count > 0 and non_parallel_count > 0:
                parallel_names = ", ".join(e.name for e in group if e.parallel)
                non_parallel_names = ", ".join(
                    e.name for e in group if not e.parallel
                )
                issues.append(
                    Issue(
                        code="E_PARALLEL_MIXED",
                        level="error",
                        message=(
                            f"inode 「{inode}」 的 (gate={gate_name!r}, "
                            f"gate_value={gv!r}) 匹配组混合了 parallel 与"
                            f"非 parallel：parallel 边 [{parallel_names}] + "
                            f"非 parallel 边 [{non_parallel_names}]"
                        ),
                        path=("edges",),
                    )
                )


def _check_cycle_on_nocycle(graph: "Graph", issues: list[Issue]) -> None:
    """无环模式下若发现环则 warning。"""
    if graph.graph_mode.value != "directed_nocycle":
        return
    cycles = cycle_check(graph)
    if not cycles:
        return
    issues.append(
        Issue(
            code="W_CYCLE_ON_NOCYCLE",
            level="warning",
            message=(
                f"directed_nocycle 图中发现 {len(cycles)} 个环: "
                + "; ".join(" -> ".join(c) for c in cycles[:3])
            ),
            path=("graph_mode",),
        )
    )


def _check_dup_names(graph: "Graph", issues: list[Issue]) -> None:
    """节点 / 门 / 边内部 name 唯一性。"""
    _check_dup_kind(graph.nodes, "node", "E_DUP_NODE_NAME", issues)
    _check_dup_kind(graph.gates, "gate", "E_DUP_GATE_NAME", issues)
    _check_dup_kind(graph.edges, "edge", "E_DUP_EDGE_NAME", issues)


def _check_dup_kind(
    items: tuple, kind: str, code: str, issues: list[Issue]
) -> None:
    """单个类别内 name 唯一。"""
    seen: dict[str, None] = {}
    for idx, it in enumerate(items):
        if it.name in seen:
            issues.append(
                Issue(
                    code=code,
                    level="error",
                    message=f"重复的 {kind} name 「{it.name}」",
                    path=(f"{kind}s", idx, "name"),
                )
            )
        else:
            seen[it.name] = None


def _node_position(graph: "Graph", name: str) -> int:
    for idx, n in enumerate(graph.nodes):
        if n.name == name:
            return idx
    return -1


def _gate_position(graph: "Graph", name: str) -> int:
    for idx, g in enumerate(graph.gates):
        if g.name == name:
            return idx
    return -1