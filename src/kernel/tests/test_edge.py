"""Edge dataclass 与 Driver 枚举测试。"""

from src.kernel import Driver, Edge


def test_edge_construct():
    """测试名：test_edge_construct

    测试场景：显式 Driver 枚举 + 各必填字段构造 Edge 时正确保留。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Edge(name="e", inode="a", onode="b", driver=Driver.TICK,
      gate="g", gate_value=True, cmd="fn")。
    预期结果：driver is Driver.TICK、gate_value is True。
    测试后清理：无。
    """
    e = Edge(
        name="e",
        inode="a",
        onode="b",
        driver=Driver.TICK,
        gate="g",
        gate_value=True,
        cmd="fn",
    )
    assert e.driver is Driver.TICK
    assert e.gate_value is True


def test_edge_frozen():
    """测试名：test_edge_frozen

    测试场景：Edge 是 frozen dataclass，赋值应抛 FrozenInstanceError。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Edge 后尝试 e.cmd = "x"。
    预期结果：抛 Exception（frozen）。
    测试后清理：无。
    """
    e = Edge(
        name="e",
        inode="a",
        onode="b",
        driver=Driver.MANUAL,
        gate="g",
        gate_value=0,
        cmd="fn",
    )
    try:
        e.cmd = "x"  # type: ignore[misc]
    except Exception:
        return
    raise AssertionError("frozen dataclass 不应允许赋值")


def test_driver_str_enum():
    """测试名：test_driver_str_enum

    测试场景：Driver 是 str 子类 Enum，value 与反查一致。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：读 Driver.TICK.value / Driver.MANUAL.value，并 Driver("tick") 反查。
    预期结果：TICK.value == "tick"、MANUAL.value == "manual"、Driver("tick") is TICK。
    测试后清理：无。
    """
    assert Driver.TICK.value == "tick"
    assert Driver.MANUAL.value == "manual"
    assert Driver("tick") is Driver.TICK


def test_edge_parallel_defaults_to_false():
    """测试名：test_edge_parallel_defaults_to_false

    测试场景：Edge 不显式传 parallel 时默认 False（向后兼容所有现有图）。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Edge 不传 parallel。
    预期结果：e.parallel is False。
    测试后清理：无。
    """
    e = Edge(
        name="e",
        inode="a",
        onode="b",
        driver=Driver.TICK,
        gate="g",
        gate_value=True,
        cmd="fn",
    )
    assert e.parallel is False


def test_edge_parallel_true_explicit():
    """测试名：test_edge_parallel_true_explicit

    测试场景：Edge 显式 parallel=True 时正确保留。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Edge(parallel=True)。
    预期结果：e.parallel is True。
    测试后清理：无。
    """
    e = Edge(
        name="e",
        inode="a",
        onode="b",
        driver=Driver.TICK,
        gate="g",
        gate_value=True,
        cmd="fn",
        parallel=True,
    )
    assert e.parallel is True