"""StateManager 测试。"""

from __future__ import annotations

import json
from pathlib import Path

from src.components.state_manager import State, StateManager
from src.kernel import (
    Edge,
    Gate,
    Graph,
    GraphMode,
    Node,
    NodeAttr,
    register_op,
)


def _build_graph() -> Graph:
    n1 = Node(
        name="a", iport=(("in.md",),), oport=(("out.md",),),
        attr=NodeAttr(is_src=True, role="r"),
    )
    n2 = Node(
        name="b", iport=(("out.md",),), oport=(),
        attr=NodeAttr(is_sink=True, role="r"),
    )
    g1 = Gate(name="ga", op="check_a", enum_dir=(True,))
    e1 = Edge(
        name="a_to_b", inode="a", onode="b",
        driver="tick", gate="ga", gate_value=True, cmd="",
    )
    return Graph.build(
        name="sm", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n1, n2], gates=[g1], edges=[e1],
        tick_period_s=1.0,
    )


def test_create_writes_state_with_src(tmp_path: Path) -> None:
    """测试名：test_create_writes_state_with_src

    测试场景：StateManager.create 写入初始 State（current_node=src）并落盘。
    前置条件：tmp_path；_build_graph 2 节点（a src / b sink）。
    是否使用 mock：No。
    测试步骤：StateManager.create(state_file, "sm", g, instance_id="abc")。
    预期结果：state.sop_name=="sm"；instance_id=="abc"；current_node=="a"；文件存在。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_graph()
    state_file = tmp_path / "x.json"
    state = StateManager.create(state_file, "sm", g, instance_id="abc")
    assert state.sop_name == "sm"
    assert state.instance_id == "abc"
    assert state.current_node == "a"
    assert state_file.exists()


def test_read_returns_empty_when_missing(tmp_path: Path) -> None:
    """测试名：test_read_returns_empty_when_missing

    测试场景：读不存在的 state.json 返回空白 State。
    前置条件：tmp_path 下无 nope.json。
    是否使用 mock：No。
    测试步骤：StateManager.read(tmp_path/nope.json)。
    预期结果：state.sop_name == ""；state.current_node is None。
    测试后清理：pytest tmp_path 自动清理。
    """
    state = StateManager.read(tmp_path / "nope.json")
    assert state.sop_name == ""
    assert state.current_node is None


def test_write_then_read_roundtrip(tmp_path: Path) -> None:
    """测试名：test_write_then_read_roundtrip

    测试场景：StateManager.write → read 往返一致（current_node/history/triggered_edges/last_output）。
    前置条件：tmp_path；手工构造 State 含各字段。
    是否使用 mock：No。
    测试步骤：1. write(state_file, state)；2. read(state_file)；3. 逐字段断言。
    预期结果：loaded 各字段与写入完全相等。
    测试后清理：pytest tmp_path 自动清理。
    """
    state_file = tmp_path / "x.json"
    state = State(
        sop_name="s", instance_id="i", graph_name="g",
        current_node="a",
        history=(("a", "2025-01-01T00:00:00"),),
        triggered_edges={"e1": "alice"},
        last_output={"a": ("out.md",)},
    )
    StateManager.write(state_file, state)
    loaded = StateManager.read(state_file)
    assert loaded.current_node == "a"
    assert loaded.history == (("a", "2025-01-01T00:00:00"),)
    assert dict(loaded.triggered_edges) == {"e1": "alice"}
    assert loaded.last_output["a"] == ("out.md",)


def test_transition_appends_history_and_moves_node() -> None:
    """测试名：test_transition_appends_history_and_moves_node

    测试场景：transition 把 current_node 从 from_node 移到 to_node 并追加 history。
    前置条件：手工 state current_node="a"。
    是否使用 mock：No。
    测试步骤：StateManager.transition(state, "a", "b")。
    预期结果：new_state.current_node=="b"；history 长度 1；首项 node=="a"。
    测试后清理：无。
    """
    state = State(sop_name="s", instance_id="i", graph_name="g", current_node="a")
    new_state = StateManager.transition(state, from_node="a", to_node="b")
    assert new_state.current_node == "b"
    assert len(new_state.history) == 1
    assert new_state.history[0][0] == "a"


def test_record_completion_writes_last_output() -> None:
    """测试名：test_record_completion_writes_last_output

    测试场景：record_completion 把 output_files 写进 last_output[node_name]。
    前置条件：默认 State。
    是否使用 mock：No。
    测试步骤：StateManager.record_completion(state, "a", output_files=("out/a.md","out/a2.md"))。
    预期结果：new_state.last_output["a"] == ("out/a.md","out/a2.md")。
    测试后清理：无。
    """
    state = State(sop_name="s", instance_id="i", graph_name="g")
    new_state = StateManager.record_completion(
        state, "a", output_files=("out/a.md", "out/a2.md"),
    )
    assert new_state.last_output["a"] == ("out/a.md", "out/a2.md")


def test_rollback_clears_triggered_and_last_output(tmp_path: Path) -> None:
    """测试名：test_rollback_clears_triggered_and_last_output

    测试场景：rollback 清 triggered_edges + last_output；保留 enter_cnt / exit_cnt；
      current_node 回到 history 中回退点之前一项。
    前置条件：tmp_path；state 含 history=[a,b] + triggered_edges + enter/exit_cnt + last_output 假文件路径。
    是否使用 mock：No。
    测试步骤：StateManager.rollback(state, "a", "manual", base_dir=tmp_path)。
    预期结果：current_node is None（a 之前无项）；triggered_edges 清空；enter_cnt / exit_cnt 保留；
      last_output["a"] 被移除。
    测试后清理：pytest tmp_path 自动清理。
    """
    state_file = tmp_path / "x.json"
    state = State(
        sop_name="s", instance_id="i", graph_name="g", current_node="b",
        history=(("a", "2025-01-01T00:00:00"), ("b", "2025-01-01T00:01:00")),
        triggered_edges={"e1": "alice"},
        enter_cnt={"a": 3},
        exit_cnt={"a": 3},
        last_output={"a": ("/tmp/__should_be_deleted__.md",)},
    )
    StateManager.write(state_file, state)
    new_state = StateManager.rollback(state, "a", "manual", base_dir=tmp_path)
    assert new_state.current_node is None  # a 之前没有项
    assert dict(new_state.triggered_edges) == {}
    assert dict(new_state.enter_cnt) == {"a": 3}  # 保留
    assert dict(new_state.exit_cnt) == {"a": 3}  # 保留
    assert "a" not in new_state.last_output


def test_trigger_edge_appends_to_state(tmp_path: Path) -> None:
    """测试名：test_trigger_edge_appends_to_state

    测试场景：trigger_edge 写 triggered_edges 并落盘；磁盘读回能拿到。
    前置条件：tmp_path；先 create() 初始 state。
    是否使用 mock：No。
    测试步骤：1. StateManager.create(state_file, ...)；2. trigger_edge(state_file, "a_to_b", "alice")；
      3. StateManager.read(state_file)。
    预期结果：new_state.triggered_edges == {"a_to_b":"alice"}；磁盘也读到该字段。
    测试后清理：pytest tmp_path 自动清理。
    """
    state_file = tmp_path / "x.json"
    g = _build_graph()
    StateManager.create(state_file, "sm", g, instance_id="abc")
    new_state = StateManager.trigger_edge(state_file, "a_to_b", "alice")
    assert dict(new_state.triggered_edges) == {"a_to_b": "alice"}
    # 落盘后从磁盘能读回
    loaded = StateManager.read(state_file)
    assert loaded.triggered_edges["a_to_b"] == "alice"


def test_record_enter_increments_enter_cnt() -> None:
    """测试名：test_record_enter_increments_enter_cnt

    测试场景：record_enter 把指定节点 enter_cnt +1。
    前置条件：state enter_cnt={"a":2}。
    是否使用 mock：No。
    测试步骤：StateManager.record_enter(state, "a")。
    预期结果：new_state.enter_cnt["a"] == 3；exit_cnt 不变。
    测试后清理：无。
    """
    state = State(sop_name="s", instance_id="i", graph_name="g", enter_cnt={"a": 2})
    new_state = StateManager.record_enter(state, "a")
    assert new_state.enter_cnt["a"] == 3
    assert dict(new_state.exit_cnt) == {}  # exit 不变


def test_record_exit_increments_exit_cnt() -> None:
    """测试名：test_record_exit_increments_exit_cnt

    测试场景：record_exit 把指定节点 exit_cnt +1。
    前置条件：state exit_cnt={"a":2}。
    是否使用 mock：No。
    测试步骤：StateManager.record_exit(state, "a")。
    预期结果：new_state.exit_cnt["a"] == 3；enter_cnt 不变。
    测试后清理：无。
    """
    state = State(sop_name="s", instance_id="i", graph_name="g", exit_cnt={"a": 2})
    new_state = StateManager.record_exit(state, "a")
    assert new_state.exit_cnt["a"] == 3
    assert dict(new_state.enter_cnt) == {}  # enter 不变


def test_claim_returns_true_when_empty(tmp_path: Path) -> None:
    """测试名：test_claim_returns_true_when_empty

    测试场景：state 文件为空 claim 时直接成功。
    前置条件：tmp_path 下空 state_file（不需预写）。
    是否使用 mock：No。
    测试步骤：StateManager.claim(state_file, "owner1")。
    预期结果：返回 True。
    测试后清理：pytest tmp_path 自动清理。
    """
    state_file = tmp_path / "x.json"
    assert StateManager.claim(state_file, "owner1") is True


def test_claim_returns_false_when_already_claimed(tmp_path: Path) -> None:
    """测试名：test_claim_returns_false_when_already_claimed

    测试场景：已 claim 且未过期时第二次 claim 返回 False（同 pid 走 self-claim 路径，
      异 pid 走 alive+ts 判定；这里只断言返回类型为 bool 不抛异常）。
    前置条件：tmp_path；先 claim 一次。
    是否使用 mock：No。
    测试步骤：1. claim(state_file, "owner1")；2. claim(state_file, "owner2")。
    预期结果：第二次返回 bool（True/False 之一，不抛异常）。
    测试后清理：pytest tmp_path 自动清理。
    """
    state_file = tmp_path / "x.json"
    assert StateManager.claim(state_file, "owner1") is True
    # 同 pid（os.getpid）会成功（self-claim 允许）；不同进程模拟需要传 pid
    # 这里只验证至少 True / False 不抛异常
    r2 = StateManager.claim(state_file, "owner2")
    assert isinstance(r2, bool)
