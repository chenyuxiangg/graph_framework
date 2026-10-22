"""共享 fixture。"""

from pathlib import Path

import pytest

from src.kernel import Edge, Gate, Graph, GraphMode, Node, NodeAttr


@pytest.fixture
def tmp_root(tmp_path: Path) -> Path:
    """每个测试的临时 state 根目录。"""
    return tmp_path


@pytest.fixture
def sample_graph() -> Graph:
    """一个最小但完整的 directed_cycle 示例图（与 software_team_graph.json 类似）。"""
    n1 = Node(
        name="a",
        iport=(("in/a.md",),),
        oport=(("out/a1.md",),),
        attr=NodeAttr(is_src=True, role="agent_a"),
    )
    n2 = Node(
        name="b",
        iport=(("out/a1.md",),),
        oport=(("out/b1.md",),),
        attr=NodeAttr(is_sink=True, role="agent_b"),
    )
    g1 = Gate(name="ga", op="check_a", enum_dir=(True, False))
    e1 = Edge(
        name="a_to_b",
        inode="a",
        onode="b",
        driver="tick",
        gate="ga",
        gate_value=True,
        cmd="cmd_a_to_b",
    )
    e2 = Edge(
        name="a_loop",
        inode="a",
        onode="a",
        driver="tick",
        gate="ga",
        gate_value=False,
        cmd="cmd_a_loop",
    )
    return Graph.build(
        name="sample",
        graph_mode=GraphMode.DIRECTED_CYCLE,
        pre_handle="",
        post_handle="",
        nodes=[n1, n2],
        gates=[g1],
        edges=[e1, e2],
        tick_period_s=1.0,
    )


@pytest.fixture
def linear_graph() -> Graph:
    """无环直线图：a -> b -> c。"""
    n_a = Node(name="a", iport=(), oport=(("f/a.md",),),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(("f/a.md",),), oport=(("f/b.md",),),
               attr=NodeAttr(role="r"))
    n_c = Node(name="c", iport=(("f/b.md",),), oport=(("f/c.md",),),
               attr=NodeAttr(is_sink=True, role="r"))
    g_ab = Gate(name="g_ab", op="op_ab", enum_dir=(True,))
    g_bc = Gate(name="g_bc", op="op_bc", enum_dir=(True,))
    e1 = Edge(name="ab", inode="a", onode="b", driver="tick", gate="g_ab", gate_value=True, cmd="c1")
    e2 = Edge(name="bc", inode="b", onode="c", driver="tick", gate="g_bc", gate_value=True, cmd="c2")
    return Graph.build(
        name="linear",
        graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="",
        post_handle="",
        nodes=[n_a, n_b, n_c],
        gates=[g_ab, g_bc],
        edges=[e1, e2],
        tick_period_s=1.0,
    )