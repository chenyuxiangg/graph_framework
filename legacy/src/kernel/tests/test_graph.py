"""Graph 与 Graph.build 测试。"""

import pytest

from src.kernel import Edge, Gate, Graph, GraphBuildError, GraphMode, Node, NodeAttr


def _node(name: str, *, is_src: bool = False, is_sink: bool = False) -> Node:
    return Node(
        name=name, iport=(), oport=(),
        attr=NodeAttr(is_src=is_src, is_sink=is_sink, role=""),
    )


def test_build_minimal():
    """测试名：test_build_minimal

    测试场景：1 节点 + 1 gate + 1 自环边能 build 出 Graph，派生索引正确填充。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 1 节点 (src+sink) + 1 gate + 1 自环边；调 Graph.build。
    预期结果：name/graph_mode/各索引（node_index/gate_index/edge_by_inode）正确。
    测试后清理：无。
    """
    n = _node("a", is_src=True, is_sink=True)
    g = Gate(name="ga", op="op", enum_dir=(True,))
    e = Edge(name="loop", inode="a", onode="a", driver="tick", gate="ga", gate_value=True, cmd="c")
    graph = Graph.build(
        name="x",
        graph_mode="directed_cycle",
        pre_handle="",
        post_handle="",
        nodes=[n],
        gates=[g],
        edges=[e],
        tick_period_s=1.0,
    )
    assert graph.name == "x"
    assert graph.graph_mode is GraphMode.DIRECTED_CYCLE
    assert "a" in graph.node_index
    assert graph.node_index["a"] is n
    assert "ga" in graph.gate_index
    assert "a" in graph.edge_by_inode
    assert len(graph.edge_by_inode["a"]) == 1


def test_build_indices_partition_edges():
    """测试名：test_build_indices_partition_edges

    测试场景：两条同 inode 同 gate 但 gate_value 不同的边都进对应索引。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 a(src)→b(sink) 两条边（gate_value True/False）；build。
    预期结果：edge_by_inode["a"] 长度 2、edge_by_gate["ga"] 长度 2。
    测试后清理：无。
    """
    n1 = _node("a", is_src=True)
    n2 = _node("b", is_sink=True)
    g = Gate(name="ga", op="op", enum_dir=(True, False))
    edges = [
        Edge(name="e1", inode="a", onode="b", driver="tick", gate="ga", gate_value=True, cmd="c"),
        Edge(name="e2", inode="a", onode="b", driver="tick", gate="ga", gate_value=False, cmd="c"),
    ]
    graph = Graph.build(
        name="x", graph_mode="directed_cycle",
        pre_handle="", post_handle="",
        nodes=[n1, n2], gates=[g], edges=edges,
        tick_period_s=1.0,
    )
    assert len(graph.edge_by_inode["a"]) == 2
    assert len(graph.edge_by_gate["ga"]) == 2


def test_build_no_src_raises():
    """测试名：test_build_no_src_raises

    测试场景：无 src 节点时 build 抛 E_NO_SRC。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 1 节点（仅 sink），调 Graph.build。
    预期结果：抛 GraphBuildError 且 message 含 "E_NO_SRC"。
    测试后清理：无。
    """
    n1 = _node("a", is_sink=True)
    g = Gate(name="ga", op="op", enum_dir=(True,))
    e = Edge(name="e", inode="a", onode="a", driver="tick", gate="ga", gate_value=True, cmd="c")
    with pytest.raises(GraphBuildError, match="E_NO_SRC"):
        Graph.build(
            name="x", graph_mode="directed_cycle",
            pre_handle="", post_handle="",
            nodes=[n1], gates=[g], edges=[e],
            tick_period_s=1.0,
    )


def test_build_no_sink_raises():
    """测试名：test_build_no_sink_raises

    测试场景：directed_cycle 下无 sink 节点时 build 抛 E_MULTI_SINK（"0 个 sink"）。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 1 节点（仅 src，directed_cycle），调 Graph.build。
    预期结果：抛 GraphBuildError 且 message 含 "E_MULTI_SINK"。
    测试后清理：无。
    """
    n1 = _node("a", is_src=True)
    g = Gate(name="ga", op="op", enum_dir=(True,))
    e = Edge(name="e", inode="a", onode="a", driver="tick", gate="ga", gate_value=True, cmd="c")
    with pytest.raises(GraphBuildError, match="E_MULTI_SINK"):
        Graph.build(
            name="x", graph_mode="directed_cycle",
            pre_handle="", post_handle="",
            nodes=[n1], gates=[g], edges=[e],
            tick_period_s=1.0,
    )


def test_build_dup_node_name_raises():
    """测试名：test_build_dup_node_name_raises

    测试场景：两个 node 同名时 build 抛 E_DUP_NODE_NAME。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 nodes=[_node("a",src), _node("a",sink)]，调 Graph.build。
    预期结果：抛 GraphBuildError 且 message 含 "E_DUP_NODE_NAME"。
    测试后清理：无。
    """
    n1 = _node("a", is_src=True)
    n2 = _node("a", is_sink=True)
    with pytest.raises(GraphBuildError, match="E_DUP_NODE_NAME"):
        Graph.build(
            name="x", graph_mode="directed_cycle",
            pre_handle="", post_handle="",
            nodes=[n1, n2], gates=[], edges=[],
            tick_period_s=1.0,
    )


def test_build_bad_edge_endpoint_raises():
    """测试名：test_build_bad_edge_endpoint_raises

    测试场景：边指向不存在的节点（onode="ghost"）时 build 抛 E_BAD_EDGE_ENDPOINT。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 1 节点 + 边指向 "ghost"；调 Graph.build。
    预期结果：抛 GraphBuildError 且 message 含 "E_BAD_EDGE_ENDPOINT"。
    测试后清理：无。
    """
    n1 = _node("a", is_src=True, is_sink=True)
    g = Gate(name="ga", op="op", enum_dir=(True,))
    e = Edge(name="e", inode="a", onode="ghost", driver="tick", gate="ga", gate_value=True, cmd="c")
    with pytest.raises(GraphBuildError, match="E_BAD_EDGE_ENDPOINT"):
        Graph.build(
            name="x", graph_mode="directed_cycle",
            pre_handle="", post_handle="",
            nodes=[n1], gates=[g], edges=[e],
            tick_period_s=1.0,
    )


def test_build_existing_sop_dup_raises():
    """测试名：test_build_existing_sop_dup_raises

    测试场景：图名已在 existing_sop_names 中时抛 E_DUP_GLOBAL。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：build 时传 existing_sop_names=["dup"]，图 name="dup"。
    预期结果：抛 GraphBuildError 且 message 含 "E_DUP_GLOBAL"。
    测试后清理：无。
    """
    n1 = _node("a", is_src=True, is_sink=True)
    g = Gate(name="ga", op="op", enum_dir=(True,))
    e = Edge(name="loop", inode="a", onode="a", driver="tick", gate="ga", gate_value=True, cmd="c")
    with pytest.raises(GraphBuildError, match="E_DUP_GLOBAL"):
        Graph.build(
            name="dup", graph_mode="directed_cycle",
            pre_handle="", post_handle="",
            nodes=[n1], gates=[g], edges=[e],
            existing_sop_names=["dup"],
            tick_period_s=1.0,
        )


def test_build_invalid_graph_mode_raises():
    """测试名：test_build_invalid_graph_mode_raises

    测试场景：graph_mode 不在 GraphMode 取值中时抛 GraphBuildError。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：build 时 graph_mode="bogus"。
    预期结果：抛 GraphBuildError 且 message 含 "graph_mode"。
    测试后清理：无。
    """
    n1 = _node("a", is_src=True, is_sink=True)
    g = Gate(name="ga", op="op", enum_dir=(True,))
    e = Edge(name="loop", inode="a", onode="a", driver="tick", gate="ga", gate_value=True, cmd="c")
    with pytest.raises(GraphBuildError, match="graph_mode"):
        Graph.build(
            name="x", graph_mode="bogus",  # type: ignore[arg-type]
            pre_handle="", post_handle="",
            nodes=[n1], gates=[g], edges=[e],
            tick_period_s=1.0,
    )


def test_cached_property_lazy_compute():
    """测试名：test_cached_property_lazy_compute

    测试场景：node_index / gate_index / edge_by_inode / edge_by_gate 是 @cached_property，
      首次访问计算并填充。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：build Graph 后访问四个索引属性。
    预期结果：四个索引值与构造时传入的 nodes/gates/edges 一一对应。
    测试后清理：无。
    """
    n_a = _node("a", is_src=True, is_sink=True)
    g = Gate(name="ga", op="op", enum_dir=(True,))
    e = Edge(name="loop", inode="a", onode="a", driver="tick", gate="ga", gate_value=True, cmd="c")
    graph = Graph.build(
        name="x", graph_mode="directed_cycle",
        pre_handle="", post_handle="",
        nodes=[n_a], gates=[g], edges=[e],
        tick_period_s=1.0,
    )
    assert graph.node_index == {"a": n_a}
    assert graph.gate_index == {"ga": g}
    assert graph.edge_by_inode == {"a": (e,)}
    assert graph.edge_by_gate == {"ga": (e,)}