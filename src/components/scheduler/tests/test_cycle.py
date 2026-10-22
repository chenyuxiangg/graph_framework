"""cycle 测试：detect_cycle_progress + CYCLE_THRESHOLD。"""

from __future__ import annotations

from src.components.scheduler.cycle import (
    CYCLE_THRESHOLD,
    detect_cycle_progress,
)
from src.components.state_manager import State
from src.kernel import (
    Edge,
    Gate,
    Graph,
    GraphMode,
    Node,
    NodeAttr,
)


def _cycle_graph() -> Graph:
    """a → b → a 自环图（ab/ba 用不同 gate_value）。"""
    n_a = Node(name="a", iport=(("f/b.md",),), oport=(("f/a.md",),),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(("f/a.md",),), oport=(("f/b.md",),),
               attr=NodeAttr(is_sink=True, role="r"))
    g_g = Gate(name="g_g", op="x", enum_dir=(True, False))
    e_ab = Edge(name="ab", inode="a", onode="b",
                driver="tick", gate="g_g", gate_value=True, cmd="")
    e_ba = Edge(name="ba", inode="b", onode="a",
                driver="tick", gate="g_g", gate_value=False, cmd="")
    return Graph.build(
        name="cy", graph_mode=GraphMode.DIRECTED_CYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b], gates=[g_g], edges=[e_ab, e_ba],
        tick_period_s=1.0,
    )


def test_detect_cycle_progress_no_cycles_when_dag() -> None:
    """测试名：test_detect_cycle_progress_no_cycles_when_dag

    测试场景：DAG 图（nocycle 模式）detect_cycle_progress 返回 max_count=0 / 空 nodes。
    前置条件：构造 1 节点 src+sink 图（nocycle）。
    是否使用 mock：No。
    测试步骤：1. detect_cycle_progress(g, state)；2. 断言 prog.max_count/cycle_nodes/over_threshold。
    预期结果：max_count == 0；cycle_nodes == ()；over_threshold is False。
    测试后清理：无。
    """
    n_a = Node(name="a", iport=(), oport=(),
               attr=NodeAttr(is_src=True, is_sink=True, role="r"))
    g = Graph.build(
        name="dag", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a], gates=[], edges=[],
        tick_period_s=1.0,
    )
    state = State(sop_name="s", instance_id="i", graph_name="g")
    prog = detect_cycle_progress(g, state)
    assert prog.max_count == 0
    assert prog.cycle_nodes == ()
    assert prog.over_threshold is False


def test_detect_cycle_progress_under_threshold() -> None:
    """测试名：test_detect_cycle_progress_under_threshold

    测试场景：cycle 图节点 exit_cnt < THRESHOLD → max_count=最大值，over_threshold=False。
    前置条件：_cycle_graph；state exit_cnt={"a":5, "b":7}。
    是否使用 mock：No。
    测试步骤：detect_cycle_progress(g, state)。
    预期结果：max_count == 7；cycle_nodes 含 "a"/"b"；over_threshold is False。
    测试后清理：无。
    """
    g = _cycle_graph()
    state = State(
        sop_name="s", instance_id="i", graph_name="g",
        exit_cnt={"a": 5, "b": 7},
    )
    prog = detect_cycle_progress(g, state)
    assert prog.max_count == 7
    assert "a" in prog.cycle_nodes and "b" in prog.cycle_nodes
    assert prog.over_threshold is False


def test_detect_cycle_progress_over_threshold() -> None:
    """测试名：test_detect_cycle_progress_over_threshold

    测试场景：cycle 节点 exit_cnt 超过 CYCLE_THRESHOLD → over_threshold=True。
    前置条件：_cycle_graph；state exit_cnt={"a":THRESHOLD+10, "b":1}。
    是否使用 mock：No。
    测试步骤：detect_cycle_progress(g, state)。
    预期结果：over_threshold is True；max_count == THRESHOLD+10。
    测试后清理：无。
    """
    g = _cycle_graph()
    state = State(
        sop_name="s", instance_id="i", graph_name="g",
        exit_cnt={"a": CYCLE_THRESHOLD + 10, "b": 1},
    )
    prog = detect_cycle_progress(g, state)
    assert prog.over_threshold is True
    assert prog.max_count == CYCLE_THRESHOLD + 10


def test_cycle_threshold_constant() -> None:
    """测试名：test_cycle_threshold_constant

    测试场景：CYCLE_THRESHOLD 常量稳定为 100。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：直接断言 CYCLE_THRESHOLD。
    预期结果：== 100。
    测试后清理：无。
    """
    assert CYCLE_THRESHOLD == 100
