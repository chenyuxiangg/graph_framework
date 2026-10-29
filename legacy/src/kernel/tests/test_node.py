"""Node / NodeAttr dataclass 测试。"""

from src.kernel import Node, NodeAttr


def test_node_construct_with_attr():
    """测试名：test_node_construct_with_attr

    测试场景：显式 attr + proc 构造 Node 时各字段正确赋值。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Node(name="x", iport=(("a",),), oport=(("b","c"),),
      attr=NodeAttr(is_src=True, ...), proc=""——默认值)。
    预期结果：name/iport/oport/attr.is_src/attr.is_sink/attr.role/proc 全部符合预期。
    测试后清理：无（纯内存对象）。
    """
    n = Node(
        name="x",
        iport=(("a",),),
        oport=(("b", "c"),),
        attr=NodeAttr(is_src=True, is_sink=False, role="r"),
    )
    assert n.name == "x"
    assert n.iport == (("a",),)
    assert n.oport == (("b", "c"),)
    assert n.attr.is_src is True
    assert n.attr.is_sink is False
    assert n.attr.role == "r"
    assert n.proc == ""


def test_node_default_attr():
    """测试名：test_node_default_attr

    测试场景：仅传必填字段（name/iport/oport）时 attr / proc 取默认值。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Node(name="x", iport=(), oport=())。
    预期结果：attr.is_src=False, attr.is_sink=False, attr.role="", proc=""。
    测试后清理：无。
    """
    n = Node(name="x", iport=(), oport=())
    assert n.attr.is_src is False
    assert n.attr.is_sink is False
    assert n.attr.role == ""
    assert n.proc == ""


def test_node_attr_independent_defaults():
    """测试名：test_node_attr_independent_defaults

    测试场景：NodeAttr 默认值互相独立——只设 is_src 不影响 is_sink / role。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 NodeAttr(is_src=True)。
    预期结果：is_sink=False、role=""——is_src 的赋值不污染其它字段。
    测试后清理：无。
    """
    a = NodeAttr(is_src=True)
    assert a.is_sink is False
    assert a.role == ""


def test_node_with_proc():
    """测试名：test_node_with_proc

    测试场景：显式 proc 字段时正确保留。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Node(..., proc="x_proc")。
    预期结果：n.proc == "x_proc"。
    测试后清理：无。
    """
    n = Node(name="x", iport=(), oport=(), proc="x_proc")
    assert n.proc == "x_proc"


def test_node_frozen():
    """测试名：test_node_frozen

    测试场景：Node 是 frozen dataclass，赋值应抛 FrozenInstanceError。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Node 后尝试 n.name = "y"。
    预期结果：抛 Exception（frozen）。
    测试后清理：无。
    """
    n = Node(name="x", iport=(), oport=())
    try:
        n.name = "y"  # type: ignore[misc]
    except Exception:
        return
    raise AssertionError("frozen dataclass 不应允许赋值")


def test_node_attr_frozen():
    """测试名：test_node_attr_frozen

    测试场景：NodeAttr 是 frozen dataclass，赋值应抛 FrozenInstanceError。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 NodeAttr 后尝试 a.is_src = False。
    预期结果：抛 Exception（frozen）。
    测试后清理：无。
    """
    a = NodeAttr(is_src=True)
    try:
        a.is_src = False  # type: ignore[misc]
    except Exception:
        return
    raise AssertionError("frozen dataclass 不应允许赋值")


def test_node_empty_ports_allowed():
    """测试名：test_node_empty_ports_allowed

    测试场景：iport / oport 空 tuple 是合法的。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：构造 Node(name="x", iport=(), oport=())。
    预期结果：n.iport == ()、n.oport == ()。
    测试后清理：无。
    """
    n = Node(name="x", iport=(), oport=())
    assert n.iport == ()
    assert n.oport == ()