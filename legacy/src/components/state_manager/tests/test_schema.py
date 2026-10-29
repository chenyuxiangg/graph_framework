"""State dataclass 测试。"""

from __future__ import annotations

import dataclasses

from src.components.state_manager.schema import State


def test_state_default_fields() -> None:
    """测试名：test_state_default_fields

    测试场景：State 构造时不传 optional 字段时取默认值。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 State(sop_name="s", instance_id="i", graph_name="g")，逐字段断言。
    预期结果：current_node is None；history/triggered_edges/enter_cnt/exit_cnt/last_output 均为空。
    测试后清理：无。
    """
    s = State(sop_name="s", instance_id="i", graph_name="g")
    assert s.current_node is None
    assert s.history == ()
    assert dict(s.triggered_edges) == {}
    assert dict(s.enter_cnt) == {}
    assert dict(s.exit_cnt) == {}
    assert dict(s.last_output) == {}


def test_state_evolve_returns_new_instance() -> None:
    """测试名：test_state_evolve_returns_new_instance

    测试场景：evolve 返回新 State 实例，原对象字段不被修改。
    前置条件：默认 State。
    是否使用 mock：No。
    测试步骤：1. s.evolve(current_node="planer")；2. 断言 s 与 s2 的 current_node。
    预期结果：s.current_node 仍为 None；s2.current_node == "planer"。
    测试后清理：无。
    """
    s = State(sop_name="s", instance_id="i", graph_name="g")
    s2 = s.evolve(current_node="planer")
    assert s.current_node is None  # 原对象未变
    assert s2.current_node == "planer"


def test_state_evolve_appends_history() -> None:
    """测试名：test_state_evolve_appends_history

    测试场景：evolve 替换 history 时新历史项正确进入。
    前置条件：默认 State。
    是否使用 mock：No。
    测试步骤：s.evolve(history=s.history + (("planer", "2025-01-01T00:00:00"),))。
    预期结果：s2.history 长度 1；首项 node == "planer"。
    测试后清理：无。
    """
    s = State(sop_name="s", instance_id="i", graph_name="g")
    s2 = s.evolve(history=s.history + (("planer", "2025-01-01T00:00:00"),))
    assert len(s2.history) == 1
    assert s2.history[0][0] == "planer"


def test_state_is_frozen() -> None:
    """测试名：test_state_is_frozen

    测试场景：State 是 frozen dataclass，写字段抛 FrozenInstanceError。
    前置条件：默认 State。
    是否使用 mock：No。
    测试步骤：1. s.current_node = "x"；2. 期望 FrozenInstanceError。
    预期结果：抛 FrozenInstanceError（不抛则断言失败）。
    测试后清理：无。
    """
    s = State(sop_name="s", instance_id="i", graph_name="g")
    try:
        s.current_node = "x"  # type: ignore[misc]
    except dataclasses.FrozenInstanceError:
        return
    raise AssertionError("expected FrozenInstanceError")
