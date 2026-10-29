"""图遍历与查询（纯函数）。"""

from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .edge import Edge
    from .graph import Graph


class CyclicGraphError(Exception):
    """图中存在环，但调用方要求无环拓扑。"""


class AmbiguousMatchError(Exception):
    """match_gate 发现同 gate 同 value 上挂了 ≥2 条边。"""


def successors(graph: "Graph", node_name: str) -> tuple[str, ...]:
    """返回 node_name 的所有出边终点（去重、保序）。"""
    if node_name not in graph.node_index:
        raise KeyError(f"节点 「{node_name}」 不存在")
    seen: dict[str, None] = {}
    for e in graph.edge_by_inode.get(node_name, ()):
        if e.onode not in seen:
            seen[e.onode] = None
    return tuple(seen.keys())


def predecessors(graph: "Graph", node_name: str) -> tuple[str, ...]:
    """返回 node_name 的所有入边起点（去重、保序）。"""
    if node_name not in graph.node_index:
        raise KeyError(f"节点 「{node_name}」 不存在")
    seen: dict[str, None] = {}
    for e in graph.edges:
        if e.onode == node_name and e.inode not in seen:
            seen[e.inode] = None
    return tuple(seen.keys())


def edges_from(graph: "Graph", node_name: str) -> tuple["Edge", ...]:
    """返回 node_name 的所有出边（按 graph.edges 顺序）。"""
    if node_name not in graph.node_index:
        raise KeyError(f"节点 「{node_name}」 不存在")
    return graph.edge_by_inode.get(node_name, ())


def edges_via_gate(graph: "Graph", gate_name: str) -> tuple["Edge", ...]:
    """返回挂在 gate_name 上的所有边。"""
    if gate_name not in graph.gate_index:
        raise KeyError(f"门 「{gate_name}」 不存在")
    return graph.edge_by_gate.get(gate_name, ())


def match_gate(graph: "Graph", gate_name: str, enum_value: object) -> "Edge | None":
    """根据 enum_value 匹配挂在 gate_name 上的边。

    返回 0 或 1 条边；≥2 条抛 AmbiguousMatchError（理论上校验阶段已挡掉）。
    """
    candidates = [e for e in edges_via_gate(graph, gate_name) if e.gate_value == enum_value]
    if len(candidates) > 1:
        names = ", ".join(e.name for e in candidates)
        raise AmbiguousMatchError(
            f"gate 「{gate_name}」 的 gate_value={enum_value!r} 匹配了 {len(candidates)} 条边: {names}"
        )
    return candidates[0] if candidates else None


def cycle_check(graph: "Graph") -> tuple[tuple[str, ...], ...]:
    """返回所有简单环（节点名列表的元组）。

    使用 DFS 三色标记法。返回空 tuple 表示无环。
    cycle 模式下由监控器组件决定是否记录；nocycle 模式下 validators 包装成 W_CYCLE_ON_NOCYCLE。
    """
    adj: dict[str, list[str]] = {n.name: [] for n in graph.nodes}
    for e in graph.edges:
        if e.inode in adj:
            adj[e.inode].append(e.onode)

    WHITE, GRAY, BLACK = 0, 1, 2
    color: dict[str, int] = {n: WHITE for n in adj}
    cycles: list[list[str]] = []
    path: list[str] = []
    path_set: set[str] = set()

    def dfs(u: str) -> None:
        color[u] = GRAY
        path.append(u)
        path_set.add(u)
        for v in adj.get(u, []):
            if v not in color:
                # onode 不在节点列表（理论已被校验器挡掉），跳过
                continue
            if color[v] == GRAY:
                # 找到环：从 v 在 path 中的位置到末尾
                idx = path.index(v)
                cycle = path[idx:] + [v]
                cycles.append(cycle)
            elif color[v] == WHITE:
                dfs(v)
        path.pop()
        path_set.discard(u)
        color[u] = BLACK

    for node in adj:
        if color[node] == WHITE:
            dfs(node)

    # 去重：相同环（不同起点表示）会重复出现，按 sorted tuple 去重
    seen: set[tuple[str, ...]] = set()
    unique: list[tuple[str, ...]] = []
    for c in cycles:
        # 去掉末尾的重复首节点，得到环本体
        body = tuple(c[:-1])
        key = tuple(sorted(body))
        if key not in seen:
            seen.add(key)
            unique.append(body)
    return tuple(unique)


def topo_sort(graph: "Graph") -> tuple[str, ...]:
    """Kahn 算法，返回节点拓扑序。环存在抛 CyclicGraphError。

    仅在 graph_mode=directed_nocycle 场景使用；cycle 模式下调用前自行确认无环。
    """
    indeg: dict[str, int] = {n.name: 0 for n in graph.nodes}
    for e in graph.edges:
        if e.inode in indeg and e.onode in indeg:
            indeg[e.onode] += 1

    # 按 graph.nodes 顺序初始化队列，保证稳定性
    queue = [n for n in indeg if indeg[n] == 0]
    order: list[str] = []
    while queue:
        u = queue.pop(0)
        order.append(u)
        for e in graph.edge_by_inode.get(u, ()):
            indeg[e.onode] -= 1
            if indeg[e.onode] == 0:
                queue.append(e.onode)

    if len(order) != len(indeg):
        raise CyclicGraphError(
            f"图 「{graph.name}」 存在环，无法进行拓扑排序（已排出 {len(order)}/{len(indeg)}）"
        )
    return tuple(order)