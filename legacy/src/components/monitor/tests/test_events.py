"""events 测试：SCH_EV_* 常量 + level 映射。"""

from __future__ import annotations

from src.components.monitor.events import Event


def test_event_constants_have_sch_ev_prefix() -> None:
    """测试名：test_event_constants_have_sch_ev_prefix

    测试场景：所有 Event 子项的 .name 字段都以 "sch_ev_" 开头。
    前置条件：events.py 已定义 SCH_EV_* 常量。
    是否使用 mock：No。
    测试步骤：遍历 Event 的非 _ 开头属性；过滤出含 .name 的项；逐个校验前缀。
    预期结果：所有 .name 都以 "sch_ev_" 开头。
    测试后清理：无。
    """
    for name in dir(Event):
        if name.startswith("_"):
            continue
        evt = getattr(Event, name)
        if not hasattr(evt, "name"):
            continue
        assert evt.name.startswith("sch_ev_"), f"{name} -> {evt.name}"


def test_event_levels_are_valid() -> None:
    """测试名：test_event_levels_are_valid

    测试场景：所有事件的 .level ∈ {debug, info, warning, error}。
    前置条件：events.py 已定义。
    是否使用 mock：No。
    测试步骤：遍历 Event 子项；过滤含 .level 的项；逐个断言 in valid set。
    预期结果：所有 level 都合法。
    测试后清理：无。
    """
    valid = {"debug", "info", "warning", "error"}
    for name in dir(Event):
        if name.startswith("_"):
            continue
        evt = getattr(Event, name)
        if not hasattr(evt, "level"):
            continue
        assert evt.level in valid, f"{name} -> {evt.level}"


def test_specific_event_levels() -> None:
    """测试名：test_specific_event_levels

    测试场景：关键事件 level 锁定（与 plan 对齐）。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：逐个断言 Event.SCH_EV_TICK/.SCH_EV_SOP_DONE/.SCH_EV_ERROR/.SCH_EV_TIMEOUT/
      .SCH_EV_NODE_FAIL/.SCH_EV_MANUAL_PENDING 的 level。
    预期结果：分别 == "debug"/"info"/"error"/"error"/"warning"/"warning"。
    测试后清理：无。
    """
    assert Event.SCH_EV_TICK.level == "debug"
    assert Event.SCH_EV_SOP_DONE.level == "info"
    assert Event.SCH_EV_ERROR.level == "error"
    assert Event.SCH_EV_TIMEOUT.level == "error"
    assert Event.SCH_EV_NODE_FAIL.level == "warning"
    assert Event.SCH_EV_MANUAL_PENDING.level == "warning"


def test_event_count() -> None:
    """测试名：test_event_count

    测试场景：事件数量稳定（plan 定义 17 个；这里给下界 15）。
    前置条件：events.py 已定义。
    是否使用 mock：No。
    测试步骤：统计 dir(Event) 内含 .name 字段且非 _ 开头的数量。
    预期结果：count >= 15（给后续新增留空间）。
    测试后清理：无。
    """
    n = sum(
        1 for name in dir(Event)
        if not name.startswith("_") and hasattr(getattr(Event, name), "name")
    )
    assert n >= 15  # 至少 15 个；后续新增不破坏
