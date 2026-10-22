"""traversal 测试。"""

import pytest

from src.kernel import (
    AmbiguousMatchError,
    CyclicGraphError,
    cycle_check,
    edges_from,
    edges_via_gate,
    match_gate,
    predecessors,
    successors,
    topo_sort,
)


def test_successors_and_predecessors(sample_graph):
    """测试名：test_successors_and_predecessors

    测试场景：successors / predecessors 按图结构正确返回邻居节点。
    前置条件：sample_graph fixture（a→b、a→a 自环）。
    是否使用 mock：No。
    测试步骤：1. successors(g, "a") / successors(g, "b")；
      2. predecessors(g, "a") / predecessors(g, "b")。
    预期结果：successors(a) == (b, a)；successors(b) == ()；
      predecessors(a) == (a,)（仅自环起点）；predecessors(b) == (a,)。
    测试后清理：无（fixture 自动）。
    """
    # sample_graph: a -> b (ga=True), a -> a self-loop (ga=False)
    # 1) 出边
    assert successors(sample_graph, "a") == ("b", "a")
    assert successors(sample_graph, "b") == ()  # b 没有出边
    # 2) 入边起点：a 的入边起点是 a（self-loop）；b 的入边起点是 a
    assert predecessors(sample_graph, "a") == ("a",)
    assert predecessors(sample_graph, "b") == ("a",)


def test_successors_unknown_node_raises(sample_graph):
    """测试名：test_successors_unknown_node_raises

    测试场景：successors 对不存在的节点抛 KeyError。
    前置条件：sample_graph fixture。
    是否使用 mock：No。
    测试步骤：successors(sample_graph, "ghost")。
    预期结果：抛 KeyError。
    测试后清理：无（fixture 自动）。
    """
    with pytest.raises(KeyError):
        successors(sample_graph, "ghost")


def test_edges_from_and_via_gate(sample_graph):
    """测试名：test_edges_from_and_via_gate

    测试场景：edges_from / edges_via_gate 按 inode / 按 gate 索引都能找到边。
    前置条件：sample_graph fixture。
    是否使用 mock：No。
    测试步骤：1. edges_from(g, "a")；2. edges_via_gate(g, "ga")。
    预期结果：两条边都各返回 2 条。
    测试后清理：无（fixture 自动）。
    """
    e1 = edges_from(sample_graph, "a")
    assert len(e1) == 2
    e2 = edges_via_gate(sample_graph, "ga")
    assert len(e2) == 2


def test_match_gate_returns_edge(sample_graph):
    """测试名：test_match_gate_returns_edge

    测试场景：match_gate 按 (gate, value) 命中时返回对应边；不命中返回 None。
    前置条件：sample_graph fixture。
    是否使用 mock：No。
    测试步骤：1. match_gate(g, "ga", True)；2. match_gate(g, "ga", "missing")。
    预期结果：第一条返回 a_to_b；第二条返回 None。
    测试后清理：无（fixture 自动）。
    """
    edge = match_gate(sample_graph, "ga", True)
    assert edge is not None and edge.name == "a_to_b"
    edge_none = match_gate(sample_graph, "ga", "missing")
    assert edge_none is None


def test_match_gate_ambiguous_raises():
    """测试名：test_match_gate_ambiguous_raises

    测试场景：同 gate 同 value 多边时 match_gate 抛 AmbiguousMatchError。
    前置条件：临时构造 Graph（含 a→a 自环 2 条同名 gate / 同 gate_value=True）。
    是否使用 mock：No。
    测试步骤：1. 用 Graph(...) 直接构造（绕过 build）；2. match_gate(g, "ga", True)。
    预期结果：抛 AmbiguousMatchError。
    测试后清理：无（无副作用）。
    """
    from src.kernel import Edge, Gate, Graph, GraphMode, Node, NodeAttr

    # 构造同 gate 同 value 多边的图（绕过 build 直接构造）
    g = Graph(
        name="x", graph_mode=GraphMode.DIRECTED_CYCLE,
        pre_handle="", post_handle="",
        nodes=(Node(name="a", iport=(), oport=(), attr=NodeAttr(is_src=True, role="")),),
        gates=(Gate(name="ga", op="f", enum_dir=(True,)),),
        edges=(
            Edge(name="e1", inode="a", onode="a", driver="tick", gate="ga", gate_value=True, cmd="c"),
            Edge(name="e2", inode="a", onode="a", driver="tick", gate="ga", gate_value=True, cmd="c"),
        ),
        tick_period_s=1.0,
    )
    with pytest.raises(AmbiguousMatchError):
        match_gate(g, "ga", True)


def test_cycle_check_returns_all_cycles(sample_graph):
    """测试名：test_cycle_check_returns_all_cycles

    测试场景：cycle_check 在有自环的 sample_graph 上能找到含 "a" 的环。
    前置条件：sample_graph fixture（含 a→a 自环）。
    是否使用 mock：No。
    测试步骤：调 cycle_check(sample_graph)。
    预期结果：cycles 元组至少有一个含 "a"。
    测试后清理：无（fixture 自动）。
    """
    cycles = cycle_check(sample_graph)
    # sample_graph 有 self-loop (a->a)
    assert any("a" in c for c in cycles)


def test_cycle_check_no_cycle(linear_graph):
    """测试名：test_cycle_check_no_cycle

    测试场景：cycle_check 在无环线性图上返回空 tuple。
    前置条件：linear_graph fixture（a→b→c 无环）。
    是否使用 mock：No。
    测试步骤：调 cycle_check(linear_graph)。
    预期结果：cycles == ()。
    测试后清理：无（fixture 自动）。
    """
    cycles = cycle_check(linear_graph)
    assert cycles == ()


def test_topo_sort_linear(linear_graph):
    """测试名：test_topo_sort_linear

    测试场景：线性图 topo_sort 按 a→b→c 顺序返回。
    前置条件：linear_graph fixture。
    是否使用 mock：No。
    测试步骤：调 topo_sort(linear_graph)。
    预期结果：order == ("a", "b", "c")。
    测试后清理：无（fixture 自动）。
    """
    order = topo_sort(linear_graph)
    assert order == ("a", "b", "c")


def test_topo_sort_cycle_raises(sample_graph):
    """测试名：test_topo_sort_cycle_raises

    测试场景：含环图 topo_sort 抛 CyclicGraphError。
    前置条件：sample_graph fixture（含 a→a 自环）。
    是否使用 mock：No。
    测试步骤：调 topo_sort(sample_graph)。
    预期结果：抛 CyclicGraphError。
    测试后清理：无（fixture 自动）。
    """
    with pytest.raises(CyclicGraphError):
        topo_sort(sample_graph)
