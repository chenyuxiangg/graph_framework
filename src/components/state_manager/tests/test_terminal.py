"""terminal 测试：is_sop_done + run_post_handle。"""

from __future__ import annotations

from pathlib import Path

from src.components.state_manager import State
from src.components.state_manager.terminal import is_sop_done, run_post_handle
from src.kernel import (
    Edge,
    Gate,
    Graph,
    GraphMode,
    Node,
    NodeAttr,
    register_op,
)


def _build_graph_with_post_handle(post_handle: str = "") -> Graph:
    n1 = Node(name="a", iport=(), oport=(),
              attr=NodeAttr(is_src=True, is_sink=True, role="r"))
    return Graph.build(
        name="t", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle=post_handle,
        nodes=[n1], gates=[], edges=[],
        tick_period_s=1.0,
    )


def test_is_sop_done_false_when_current_none() -> None:
    """测试名：test_is_sop_done_false_when_current_none

    测试场景：current_node=None（SOP 未启动）→ is_sop_done 返回 False。
    前置条件：_build_graph_with_post_handle（a src+sink）。
    是否使用 mock：No。
    测试步骤：is_sop_done(g, state)（state.current_node=None）。
    预期结果：返回 False。
    测试后清理：无。
    """
    g = _build_graph_with_post_handle()
    state = State(sop_name="t", instance_id="i", graph_name="t")
    assert is_sop_done(g, state) is False


def test_is_sop_done_true_when_current_is_sink() -> None:
    """测试名：test_is_sop_done_true_when_current_is_sink

    测试场景：current_node 是 sink 节点 → is_sop_done 返回 True。
    前置条件：_build_graph_with_post_handle；state.current_node="a"（a is_sink=True）。
    是否使用 mock：No。
    测试步骤：is_sop_done(g, state)。
    预期结果：返回 True。
    测试后清理：无。
    """
    g = _build_graph_with_post_handle()
    state = State(sop_name="t", instance_id="i", graph_name="t", current_node="a")
    assert is_sop_done(g, state) is True


def test_is_sop_done_false_when_current_not_sink() -> None:
    """测试名：test_is_sop_done_false_when_current_not_sink

    测试场景：current_node 是非 sink 的中间节点 → is_sop_done 返回 False。
    前置条件：临时 Graph（a src / b 普通 / c sink）；state.current_node="b"。
    是否使用 mock：No。
    测试步骤：is_sop_done(g, state)。
    预期结果：返回 False。
    测试后清理：无。
    """
    n1 = Node(name="a", iport=(), oport=(),
              attr=NodeAttr(is_src=True, role="r"))
    n2 = Node(name="b", iport=(), oport=(),
              attr=NodeAttr(role="r"))
    n3 = Node(name="c", iport=(), oport=(),
              attr=NodeAttr(is_sink=True, role="r"))
    g = Graph.build(
        name="t", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n1, n2, n3], gates=[], edges=[],
        tick_period_s=1.0,
    )
    state = State(sop_name="t", instance_id="i", graph_name="t", current_node="b")
    assert is_sop_done(g, state) is False


def test_is_sop_done_false_when_sink_has_oport_but_no_outputs() -> None:
    """测试名：test_is_sop_done_false_when_sink_has_oport_but_no_outputs

    测试场景：sink 节点 oport 非空但 last_output 还没产物（worker 未 record_completion）
      → is_sop_done 返回 False。这是真实场景：current_node 已切到 sink，
      worker 还在异步跑、产物还没落盘。
    前置条件：临时 Graph（a sink，oport=(("doc/out.md",),)）；state.current_node="a"，
      state.last_output={}。
    是否使用 mock：No。
    测试步骤：is_sop_done(g, state)。
    预期结果：返回 False（不等同于 sink 即 done）。
    测试后清理：无。
    """
    n1 = Node(name="a", iport=(), oport=(("doc/out.md",),),
              attr=NodeAttr(is_src=True, is_sink=True, role="r"))
    g = Graph.build(
        name="t", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n1], gates=[], edges=[],
        tick_period_s=1.0,
    )
    state = State(sop_name="t", instance_id="i", graph_name="t",
                  current_node="a", last_output={})
    assert is_sop_done(g, state) is False


def test_is_sop_done_true_when_sink_has_oport_and_outputs() -> None:
    """测试名：test_is_sop_done_true_when_sink_has_oport_and_outputs

    测试场景：sink 节点 oport 非空 + last_output 有产物（worker 已 record_completion）
      → is_sop_done 返回 True。
    前置条件：临时 Graph（a sink，oport=(("doc/out.md",),)）；state.current_node="a"，
      state.last_output={"a": ("/path/doc/out.md",)}。
    是否使用 mock：No。
    测试步骤：is_sop_done(g, state)。
    预期结果：返回 True。
    测试后清理：无。
    """
    n1 = Node(name="a", iport=(), oport=(("doc/out.md",),),
              attr=NodeAttr(is_src=True, is_sink=True, role="r"))
    g = Graph.build(
        name="t", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n1], gates=[], edges=[],
        tick_period_s=1.0,
    )
    state = State(sop_name="t", instance_id="i", graph_name="t",
                  current_node="a", last_output={"a": ("/path/doc/out.md",)})
    assert is_sop_done(g, state) is True


def test_run_post_handle_skips_when_empty() -> None:
    """测试名：test_run_post_handle_skips_when_empty

    测试场景：post_handle 为空字符串时 run_post_handle 跳过并返回 True。
    前置条件：post_handle="" 的图。
    是否使用 mock：No。
    测试步骤：run_post_handle(g, state, /tmp)。
    预期结果：返回 True（noop）。
    测试后清理：无。
    """
    g = _build_graph_with_post_handle(post_handle="")
    state = State(sop_name="t", instance_id="i", graph_name="t")
    assert run_post_handle(g, state, Path("/tmp")) is True


def test_run_post_handle_runs_registered_fn(tmp_path: Path) -> None:
    """测试名：test_run_post_handle_runs_registered_fn

    测试场景：post_handle 是注册到 op 的函数名时 run_post_handle 调它并返回其返回值。
    前置条件：tmp_path；register_op("test_post", _post)（返回 True）。
    是否使用 mock：Yes（注册 stub 函数返回 True）。
    测试步骤：run_post_handle(g, state, tmp_path)（g.post_handle="test_post"）。
    预期结果：返回 True。
    测试后清理：pytest tmp_path 自动清理。
    """
    def _post(base_dir, state, graph) -> bool:
        return True

    register_op("test_post", _post)

    g = _build_graph_with_post_handle(post_handle="test_post")
    state = State(sop_name="t", instance_id="i", graph_name="t")
    assert run_post_handle(g, state, tmp_path) is True
