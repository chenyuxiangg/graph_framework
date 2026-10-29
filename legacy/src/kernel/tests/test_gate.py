"""Gate dataclass 测试。"""

from src.kernel import Gate


def test_gate_construct():
    """测试名：test_gate_construct

    测试场景：异构 enum_dir（True/False/str）能被 Gate 接受。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Gate(name="g", op="fn", enum_dir=(True, False, "x"))。
    预期结果：name/op/enum_dir 三个字段完整保留。
    测试后清理：无。
    """
    g = Gate(name="g", op="fn", enum_dir=(True, False, "x"))
    assert g.name == "g"
    assert g.op == "fn"
    assert g.enum_dir == (True, False, "x")


def test_gate_frozen():
    """测试名：test_gate_frozen

    测试场景：Gate 是 frozen dataclass。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Gate 后尝试 g.op = "other"。
    预期结果：抛 Exception。
    测试后清理：无。
    """
    g = Gate(name="g", op="fn", enum_dir=(True,))
    try:
        g.op = "other"  # type: ignore[misc]
    except Exception:
        return
    raise AssertionError("frozen dataclass 不应允许赋值")


def test_gate_empty_enum_dir():
    """测试名：test_gate_empty_enum_dir

    测试场景：enum_dir 空 tuple 合法。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Gate(..., enum_dir=())。
    预期结果：g.enum_dir == ()。
    测试后清理：无。
    """
    g = Gate(name="g", op="fn", enum_dir=())
    assert g.enum_dir == ()