"""ports 测试。"""

from pathlib import Path

import pytest

from src.kernel import Node
from src.kernel.ports import (
    PortPathError,
    expand_iport_paths,
    expand_oport_paths,
    iport_satisfied,
    oport_produced,
    port_file_path,
)


def _n(iport, oport) -> Node:
    return Node(name="x", iport=iport, oport=oport)


def test_iport_satisfied_or_of_and(tmp_path: Path):
    """测试名：test_iport_satisfied_or_of_and

    测试场景：iport 二维 OR-of-AND 语义——任一组满足即满足；组内全文件需存在。
    前置条件：pytest tmp_path 已建；a.md / c.md 已存在，b.md 后续写入。
    是否使用 mock：No。
    测试步骤：1. 先写 a.md、c.md；2. 构造两组 iport（单 a / 双 b+c）；3. 调 iport_satisfied；
      4. 断言 (0,)；5. 补写 b.md；6. 再调断言 (0, 1)。
    预期结果：b.md 缺失时仅 group 0 满足；补齐后两组都满足。
    测试后清理：pytest tmp_path 自动清理。
    """
    a = tmp_path / "a.md"
    b = tmp_path / "b.md"
    c = tmp_path / "c.md"
    a.write_text("a")
    c.write_text("c")
    node = _n(iport=(("a.md",), ("b.md", "c.md")), oport=())
    # group 0: a.md 存在 → 满足
    # group 1: b.md 不存在 → 不满足
    assert iport_satisfied(node, tmp_path) == (0,)
    b.write_text("b")
    # 两组都满足
    assert iport_satisfied(node, tmp_path) == (0, 1)


def test_iport_satisfied_empty(tmp_path: Path):
    """测试名：test_iport_satisfied_empty

    测试场景：iport 空 tuple 的节点视为无前置输入（直接满足）。
    前置条件：pytest tmp_path 已建；空 iport/oport Node。
    是否使用 mock：No。
    测试步骤：构造 Node(name="x", iport=(), oport=())；调 iport_satisfied。
    预期结果：返回 ()（空 tuple 表示全部 0 个组已满足）。
    测试后清理：pytest tmp_path 自动清理。
    """
    node = _n(iport=(), oport=())
    assert iport_satisfied(node, tmp_path) == ()


def test_oport_produced(tmp_path: Path):
    """测试名：test_oport_produced

    测试场景：oport 二维 OR-of-AND 产出判定——组内文件全部存在即该组已产出。
    前置条件：pytest tmp_path 下写 x.md；y.md 缺失。
    是否使用 mock：No。
    测试步骤：构造两组 oport（单 x / 单 y）；调 oport_produced。
    预期结果：仅 group 0（x.md 存在）返回 → (0,)。
    测试后清理：pytest tmp_path 自动清理。
    """
    (tmp_path / "x.md").write_text("x")
    node = _n(iport=(), oport=(("x.md",), ("y.md",)))
    assert oport_produced(node, tmp_path) == (0,)


def test_port_file_path_relative(tmp_path: Path):
    """测试名：test_port_file_path_relative

    测试场景：相对路径 port 解析后落在 base_dir 下。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：调 port_file_path("sub/x.md", tmp_path)。
    预期结果：等于 (tmp_path/sub/x.md).resolve()。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = port_file_path("sub/x.md", tmp_path)
    assert p == (tmp_path / "sub" / "x.md").resolve()


def test_port_file_path_escape_rejected(tmp_path: Path):
    """测试名：test_port_file_path_escape_rejected

    测试场景：port 相对路径试图逃出 base_dir 时抛 PortPathError。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：调 port_file_path("../etc/passwd", tmp_path)。
    预期结果：抛 PortPathError 且 message 含 "逃出"。
    测试后清理：pytest tmp_path 自动清理。
    """
    with pytest.raises(PortPathError, match="逃出"):
        port_file_path("../etc/passwd", tmp_path)


def test_port_file_path_absolute_rejected(tmp_path: Path):
    """测试名：test_port_file_path_absolute_rejected

    测试场景：port 绝对路径被拒绝（要求相对 base_dir）。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：调 port_file_path("/etc/passwd", tmp_path)。
    预期结果：抛 PortPathError 且 message 含 "相对路径"。
    测试后清理：pytest tmp_path 自动清理。
    """
    with pytest.raises(PortPathError, match="相对路径"):
        port_file_path("/etc/passwd", tmp_path)


def test_resolve_port_groups_shape(tmp_path: Path):
    """测试名：test_resolve_port_groups_shape

    测试场景：expand_iport_paths / expand_oport_paths 保持原始二维结构。
    前置条件：pytest tmp_path 已建；Node iport=((a.md, b.md),) oport=((c.md,),)。
    是否使用 mock：No。
    测试步骤：分别调 expand_iport_paths / expand_oport_paths 后取 shape。
    预期结果：iport 1 组 × 2 文件；oport 1 组 × 1 文件。
    测试后清理：pytest tmp_path 自动清理。
    """
    node = _n(iport=(("a.md", "b.md"),), oport=(("c.md",),))
    ipaths = expand_iport_paths(node, tmp_path)
    assert len(ipaths) == 1 and len(ipaths[0]) == 2
    opaths = expand_oport_paths(node, tmp_path)
    assert len(opaths) == 1 and len(opaths[0]) == 1
