"""evaluator 测试：gate 求值 / match_gate / should_run_cmd / select_next_edge / run_edge_cmd。"""

from __future__ import annotations

from pathlib import Path

import pytest

from src.components.scheduler.evaluator import (
    EdgeCmdError,
    GateEvalError,
    evaluate_gates,
    match_gate,
    run_edge_cmd,
    select_next_edge,
    select_next_edges,
    should_run_cmd,
)
from src.components.state_manager import State
from src.kernel import (
    Edge,
    Gate,
    Graph,
    GraphMode,
    Node,
    NodeAttr,
    register_op,
)


def _find_edge(graph: Graph, name: str) -> Edge:
    for e in graph.edges:
        if e.name == name:
            return e
    raise KeyError(name)


def _graph_two_tick_edges() -> Graph:
    """a 有两条 tick 出边（gate_value True / False 各自命中不同 cmd）。"""
    n_a = Node(name="a", iport=(), oport=(("f/a.md",),),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    g_ab = Gate(name="g_ab", op="op_ab", enum_dir=(True, False))
    e1 = Edge(name="a_to_b", inode="a", onode="b",
              driver="tick", gate="g_ab", gate_value=True, cmd="c1")
    e2 = Edge(name="a_loop", inode="a", onode="a",
              driver="tick", gate="g_ab", gate_value=False, cmd="c2")
    return Graph.build(
        name="ev", graph_mode=GraphMode.DIRECTED_CYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b], gates=[g_ab], edges=[e1, e2],
        tick_period_s=1.0,
    )


def test_match_gate_hits_when_value_equals() -> None:
    """测试名：test_match_gate_hits_when_value_equals

    测试场景：match_gate 在 gate_values[edge.gate] == edge.gate_value 时返回 True，否则 False。
    前置条件：_graph_two_tick_edges + 手工构造 gate_values 字典。
    是否使用 mock：No。
    测试步骤：1. match_gate(e1, {"g_ab": True})；2. match_gate(e1, {"g_ab": False})。
    预期结果：True / False。
    测试后清理：无。
    """
    g = _graph_two_tick_edges()
    e1 = _find_edge(g, "a_to_b")
    assert match_gate(e1, {"g_ab": True}) is True
    assert match_gate(e1, {"g_ab": False}) is False


def test_should_run_cmd_tick_always_true() -> None:
    """测试名：test_should_run_cmd_tick_always_true

    测试场景：driver=tick 的边 gate 通过后 should_run_cmd 恒为 True。
    前置条件：_graph_two_tick_edges + 空 triggered_edges 的 state。
    是否使用 mock：No。
    测试步骤：should_run_cmd(e1, state)（e1 是 tick 边）。
    预期结果：返回 True。
    测试后清理：无。
    """
    g = _graph_two_tick_edges()
    e1 = _find_edge(g, "a_to_b")
    assert should_run_cmd(e1, State(sop_name="s", instance_id="i", graph_name="g")) is True


def test_should_run_cmd_manual_requires_trigger() -> None:
    """测试名：test_should_run_cmd_manual_requires_trigger

    测试场景：driver=manual 的边在 state.triggered_edges 含 edge.name 时才跑。
    前置条件：临时图 a→b + manual 边；state 无 trigger 时跑/有 trigger 时跑。
    是否使用 mock：No。
    测试步骤：1. 空 triggered_edges → should_run_cmd=False；
      2. state.evolve(triggered_edges={"a_to_b":"alice"}) → True。
    预期结果：第一次 False，第二次 True。
    测试后清理：无。
    """
    n_a = Node(name="a", iport=(), oport=(("f/a.md",),),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    g_ab = Gate(name="g_ab", op="op_ab", enum_dir=(True,))
    e1 = Edge(name="a_to_b", inode="a", onode="b",
              driver="manual", gate="g_ab", gate_value=True, cmd="c1")
    graph = Graph.build(
        name="ev", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b], gates=[g_ab], edges=[e1],
        tick_period_s=1.0,
    )
    e = _find_edge(graph, "a_to_b")
    state = State(sop_name="s", instance_id="i", graph_name="g")
    assert should_run_cmd(e, state) is False
    state2 = state.evolve(triggered_edges={"a_to_b": "alice"})
    assert should_run_cmd(e, state2) is True


def test_select_next_edge_picks_first_matching() -> None:
    """测试名：test_select_next_edge_picks_first_matching

    测试场景：select_next_edge 按 gate_values 命中值返回对应边。
    前置条件：_graph_two_tick_edges。
    是否使用 mock：No。
    测试步骤：1. select_next_edge(g, "a", {"g_ab": True}, state)；
      2. select_next_edge(g, "a", {"g_ab": False}, state)。
    预期结果：第一次返回 a_to_b；第二次返回 a_loop。
    测试后清理：无。
    """
    g = _graph_two_tick_edges()
    state = State(sop_name="s", instance_id="i", graph_name="g")
    edge = select_next_edge(g, "a", {"g_ab": True}, state)
    assert edge is not None and edge.name == "a_to_b"
    edge2 = select_next_edge(g, "a", {"g_ab": False}, state)
    assert edge2 is not None and edge2.name == "a_loop"


def test_select_next_edge_returns_none_when_no_match() -> None:
    """测试名：test_select_next_edge_returns_none_when_no_match

    测试场景：gate_values 中无匹配时 select_next_edge 返回 None。
    前置条件：_graph_two_tick_edges；gate_values["g_ab"] = None。
    是否使用 mock：No。
    测试步骤：select_next_edge(g, "a", {"g_ab": None}, state)。
    预期结果：返回 None。
    测试后清理：无。
    """
    g = _graph_two_tick_edges()
    state = State(sop_name="s", instance_id="i", graph_name="g")
    edge = select_next_edge(g, "a", {"g_ab": None}, state)
    assert edge is None


def test_run_edge_cmd_skips_when_cmd_empty(tmp_path: Path) -> None:
    """测试名：test_run_edge_cmd_skips_when_cmd_empty

    测试场景：edge.cmd 为空字符串时 run_edge_cmd 视为 noop，不抛错。
    前置条件：tmp_path 已建；1 节点自环边 cmd=""。
    是否使用 mock：No。
    测试步骤：run_edge_cmd(graph, e, state, tmp_path)。
    预期结果：正常返回，无异常。
    测试后清理：pytest tmp_path 自动清理。
    """
    n = Node(name="a", iport=(), oport=(),
             attr=NodeAttr(is_src=True, is_sink=True, role="r"))
    g = Gate(name="ga", op="x", enum_dir=(True,))
    e = Edge(name="e", inode="a", onode="a",
             driver="tick", gate="ga", gate_value=True, cmd="")
    graph = Graph.build(
        name="x", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n], gates=[g], edges=[e],
        tick_period_s=1.0,
    )
    state = State(sop_name="s", instance_id="i", graph_name="x")
    run_edge_cmd(graph, e, state, tmp_path)


def test_evaluate_gates_dedupes_shared_gate_calls(tmp_path: Path) -> None:
    """测试名：test_evaluate_gates_dedupes_shared_gate_calls

    测试场景：单次 evaluate_gates 内，多条出边共享同一 gate → 该 gate.op 只调一次；
      不同 gate 各自调一次。证明求值结果按 gate 去重，不是按边去重。
    前置条件：tmp_path；register_op("shared_op", _shared)/("unique_op", _unique) 两个自记 op。
    是否使用 mock：Yes（注册自记 call_count 的 op）。
    测试步骤：1. 构造 3 条出边（e1/e2 共享 gate "g_shared" + 不同 gate_value，
      e3 独占 gate "g_unique"）；
      2. evaluate_gates(graph, "a", tmp_path, state)；
      3. 校验返回值与共享/独占 op 的 call_count。
    预期结果：result == {"g_shared": True, "g_unique": True}；
      shared_call_count == 1（共享求值一次，未做共享则会是 2）；
      unique_call_count == 1。
    测试后清理：pytest tmp_path 自动清理。
    """
    shared_calls = {"n": 0}
    unique_calls = {"n": 0}

    def _shared(base_dir, state, graph, gate) -> bool:
        shared_calls["n"] += 1
        return True

    def _unique(base_dir, state, graph, gate) -> bool:
        unique_calls["n"] += 1
        return True

    register_op("shared_op", _shared)
    register_op("unique_op", _unique)

    n_a = Node(name="a", iport=(), oport=(("f/a.md",),),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    n_c = Node(name="c", iport=(), oport=(),
               attr=NodeAttr(role="r"))
    g_shared = Gate(name="g_shared", op="shared_op", enum_dir=(True, False))
    g_unique = Gate(name="g_unique", op="unique_op", enum_dir=(True,))
    e1 = Edge(name="ab", inode="a", onode="b",
              driver="tick", gate="g_shared", gate_value=True, cmd="")
    e2 = Edge(name="aa", inode="a", onode="a",
              driver="tick", gate="g_shared", gate_value=False, cmd="")
    e3 = Edge(name="ac", inode="a", onode="c",
              driver="tick", gate="g_unique", gate_value=True, cmd="")
    graph = Graph.build(
        name="ev", graph_mode=GraphMode.DIRECTED_CYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b, n_c], gates=[g_shared, g_unique],
        edges=[e1,e2, e3],
        tick_period_s=1.0,
    )
    state = State(sop_name="s", instance_id="i", graph_name="g")
    result = evaluate_gates(graph, "a", tmp_path, state)
    assert result == {"g_shared": True, "g_unique": True}
    assert shared_calls["n"] == 1  # 2 条共享边只调 1 次
    assert unique_calls["n"] == 1  # 独占边调 1 次


def test_evaluate_gates_refreshes_per_call_with_state(tmp_path: Path) -> None:
    """测试名：test_evaluate_gates_refreshes_per_call_with_state

    测试场景：自环节点的 gate.op 依赖 state（exit_cnt）→ 每次 evaluate_gates
      都重读 state，gate 值随迭代轮次翻转，证明"gate 不缓存、按 tick 刷新"。
    前置条件：tmp_path；register_op("iter_gate", _iter_gate)（读 state.exit_cnt）。
    是否使用 mock：Yes（注册读 state 的 op，模拟依赖当前 state 的 gate）。
    测试步骤：1. 构造图 a→a 自环（gate_value=False，loop 边）+ a→b 出边（gate_value=True，exit 边）；
      2. 用 exit_cnt={"a":0} state 调 evaluate_gates → 选 loop；
      3. 用 exit_cnt={"a":1} state 再调 → 选 loop；
      4. 用 exit_cnt={"a":3} state 再调 → 选 exit；
      5. 校验 gate_value_count（同一 gate 在 3 次 evaluate_gates 调用里被各调 1 次 = 3 次）。
    预期结果：3 次 evaluate_gates 各调 gate.op 1 次（总 3 次，不是 1 次缓存值）；
      前两次 {"iter_gate": False} 选 loop 边；第三次 {"iter_gate": True} 选 exit 边。
    测试后清理：pytest tmp_path 自动清理。
    """
    call_count = {"n": 0}

    def _iter_gate(base_dir, state, graph, gate) -> bool:
        """根据 exit_cnt["a"] 决定：< 3 返回 False（走 loop），>= 3 返回 True（走 exit）。"""
        call_count["n"] += 1
        return state.exit_cnt.get("a", 0) >= 3

    register_op("iter_gate", _iter_gate)

    n_a = Node(name="a", iport=(), oport=(("f/a.md",),),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    g_it = Gate(name="iter_gate", op="iter_gate", enum_dir=(True, False))
    e_loop = Edge(name="a_loop", inode="a", onode="a",
                  driver="tick", gate="iter_gate", gate_value=False, cmd="")
    e_exit = Edge(name="a_to_b", inode="a", onode="b",
                  driver="tick", gate="iter_gate", gate_value=True, cmd="")
    graph = Graph.build(
        name="ev", graph_mode=GraphMode.DIRECTED_CYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b], gates=[g_it], edges=[e_loop, e_exit],
        tick_period_s=1.0,
    )

    # tick 0：循环 0 次 → gate=False → 走 loop 边
    state0 = State(sop_name="s", instance_id="i", graph_name="g",
                   exit_cnt={"a": 0})
    gv0 = evaluate_gates(graph, "a", tmp_path, state0)
    assert gv0 == {"iter_gate": False}
    assert select_next_edge(graph, "a", gv0, state0).name == "a_loop"

    # tick 1：循环 1 次 → gate 仍 False（gate 被重读 state，exit_cnt 已变）
    state1 = State(sop_name="s", instance_id="i", graph_name="g",
                   exit_cnt={"a": 1})
    gv1 = evaluate_gates(graph, "a", tmp_path, state1)
    assert gv1 == {"iter_gate": False}
    assert select_next_edge(graph, "a", gv1, state1).name == "a_loop"

    # tick 2：循环 3 次 → gate=True → 走 exit 边（不再自环）
    state3 = State(sop_name="s", instance_id="i", graph_name="g",
                   exit_cnt={"a": 3})
    gv3 = evaluate_gates(graph, "a", tmp_path, state3)
    assert gv3 == {"iter_gate": True}
    assert select_next_edge(graph, "a", gv3, state3).name == "a_to_b"

    # 关键断言：gate.op 共被调 3 次（每次 evaluate_gates 都重读 state，无缓存）
    assert call_count["n"] == 3


def test_evaluate_gates_raises_on_op_exception(tmp_path: Path) -> None:
    """测试名：test_evaluate_gates_raises_on_op_exception

    测试场景：gate.op 抛异常时 evaluate_gates 抛 GateEvalError。
    前置条件：tmp_path；register_op("op_boom", _boom)（抛 RuntimeError）。
    是否使用 mock：Yes（注册抛异常的 op）。
    测试步骤：evaluate_gates(graph, "a", tmp_path, state)。
    预期结果：抛 GateEvalError 且 message 含 "op_boom"。
    测试后清理：pytest tmp_path 自动清理。
    """
    def _boom(base_dir, state, graph, gate) -> bool:
        raise RuntimeError("boom")

    register_op("op_boom", _boom)

    n_a = Node(name="a", iport=(), oport=(),
               attr=NodeAttr(is_src=True, is_sink=True, role="r"))
    g_g = Gate(name="g_g", op="op_boom", enum_dir=(True,))
    e = Edge(name="aa", inode="a", onode="a",
             driver="tick", gate="g_g", gate_value=True, cmd="")
    graph = Graph.build(
        name="ev", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a], gates=[g_g], edges=[e],
        tick_period_s=1.0,
    )
    state = State(sop_name="s", instance_id="i", graph_name="g")
    with pytest.raises(GateEvalError, match="op_boom"):
        evaluate_gates(graph, "a", tmp_path, state)


def test_run_edge_cmd_raises_on_cmd_exception(tmp_path: Path) -> None:
    """测试名：test_run_edge_cmd_raises_on_cmd_exception

    测试场景：edge.cmd 函数抛异常时 run_edge_cmd 抛 EdgeCmdError。
    前置条件：tmp_path；register_op("cmd_boom", _boom)。
    是否使用 mock：Yes（注册抛异常的 cmd）。
    测试步骤：run_edge_cmd(graph, e, state, tmp_path)（e.cmd="cmd_boom"）。
    预期结果：抛 EdgeCmdError 且 message 含 "cmd_boom"。
    测试后清理：pytest tmp_path 自动清理。
    """
    def _boom(base_dir, state, graph, edge) -> None:
        raise RuntimeError("cmd boom")

    register_op("cmd_boom", _boom)

    n_a = Node(name="a", iport=(), oport=(),
               attr=NodeAttr(is_src=True, is_sink=True, role="r"))
    g_g = Gate(name="g_g", op="op", enum_dir=(True,))
    e = Edge(name="aa", inode="a", onode="a",
             driver="tick", gate="g_g", gate_value=True, cmd="cmd_boom")
    graph = Graph.build(
        name="ev", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a], gates=[g_g], edges=[e],
        tick_period_s=1.0,
    )
    state = State(sop_name="s", instance_id="i", graph_name="g")
    with pytest.raises(EdgeCmdError, match="cmd_boom"):
        run_edge_cmd(graph, e, state, tmp_path)


# ───────────── select_next_edges 多边并行测试 ─────────────


def _graph_parallel_fanout() -> Graph:
    """a 节点有 3 条 parallel=True 出边（gate_value=True 共用），均指向不同 onode。
    """
    n_a = Node(name="a", iport=(), oport=(("f/a.md",),),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(), oport=(), attr=NodeAttr(role="r"))
    n_c = Node(name="c", iport=(), oport=(), attr=NodeAttr(role="r"))
    n_d = Node(name="d", iport=(), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    g_ab = Gate(name="g_ab", op="op_ab", enum_dir=(True,))
    e1 = Edge(name="a_to_b", inode="a", onode="b",
              driver="tick", gate="g_ab", gate_value=True, cmd="c1",
              parallel=True)
    e2 = Edge(name="a_to_c", inode="a", onode="c",
              driver="tick", gate="g_ab", gate_value=True, cmd="c2",
              parallel=True)
    e3 = Edge(name="a_to_d", inode="a", onode="d",
              driver="tick", gate="g_ab", gate_value=True, cmd="c3",
              parallel=True)
    return Graph.build(
        name="ev", graph_mode=GraphMode.DIRECTED_CYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b, n_c, n_d], gates=[g_ab], edges=[e1, e2, e3],
        tick_period_s=1.0,
    )


def test_select_next_edges_returns_all_parallel_group() -> None:
    """测试名：test_select_next_edges_returns_all_parallel_group

    测试场景：inode 的所有匹配边都是 parallel=True 时，select_next_edges 返回整组。
    前置条件：_graph_parallel_fanout。
    是否使用 mock：No。
    测试步骤：select_next_edges(g, "a", {"g_ab": True}, state)。
    预期结果：返回 3 条边，names 按声明顺序（a_to_b, a_to_c, a_to_d）。
    测试后清理：无。
    """
    g = _graph_parallel_fanout()
    state = State(sop_name="s", instance_id="i", graph_name="g")
    edges = select_next_edges(g, "a", {"g_ab": True}, state)
    assert len(edges) == 3
    assert [e.name for e in edges] == ["a_to_b", "a_to_c", "a_to_d"]


def test_select_next_edges_returns_empty_when_no_match() -> None:
    """测试名：test_select_next_edges_returns_empty_when_no_match

    测试场景：gate_values 无匹配时 select_next_edges 返回空列表。
    前置条件：_graph_parallel_fanout；gate_values["g_ab"] = None。
    是否使用 mock：No。
    测试步骤：select_next_edges(g, "a", {"g_ab": None}, state)。
    预期结果：返回 []（与 select_next_edge 返回 None 的语义不同——多边版用空列表）。
    测试后清理：无。
    """
    g = _graph_parallel_fanout()
    state = State(sop_name="s", instance_id="i", graph_name="g")
    edges = select_next_edges(g, "a", {"g_ab": None}, state)
    assert edges == []


def test_select_next_edge_returns_first_of_parallel_group() -> None:
    """测试名：test_select_next_edge_returns_first_of_parallel_group

    测试场景：select_next_edge 是 select_next_edges 的向后兼容便捷封装，
    返回整组中的第一条（primary）。
    前置条件：_graph_parallel_fanout。
    是否使用 mock：No。
    测试步骤：select_next_edge(g, "a", {"g_ab": True}, state)。
    预期结果：返回 a_to_b（edges[0]）。
    测试后清理：无。
    """
    g = _graph_parallel_fanout()
    state = State(sop_name="s", instance_id="i", graph_name="g")
    edge = select_next_edge(g, "a", {"g_ab": True}, state)
    assert edge is not None
    assert edge.name == "a_to_b"


def test_select_next_edges_non_parallel_returns_single() -> None:
    """测试名：test_select_next_edges_non_parallel_returns_single

    测试场景：匹配边非 parallel 时 select_next_edges 只返回首条命中
    （向后兼容单条语义）。
    前置条件：_graph_two_tick_edges（a→b True / a→a False，全 parallel=False 默认）。
    是否使用 mock：No。
    测试步骤：select_next_edges(g, "a", {"g_ab": True}, state)。
    预期结果：返回 [a_to_b]（仅 1 条）。
    测试后清理：无。
    """
    g = _graph_two_tick_edges()
    state = State(sop_name="s", instance_id="i", graph_name="g")
    edges = select_next_edges(g, "a", {"g_ab": True}, state)
    assert len(edges) == 1
    assert edges[0].name == "a_to_b"
