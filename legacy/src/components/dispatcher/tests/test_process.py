"""ProcessDispatcher 测试。"""

from __future__ import annotations

import os
import sys
from pathlib import Path

from src.components.dispatcher import DispatchResult
from src.components.dispatcher.process import (
    DEFAULT_WORKER_CMD,
    ProcessDispatcher,
    _SafeFormat,
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


def _graph_with_node(name: str = "a") -> Graph:
    n = Node(name=name, iport=(), oport=(),
             attr=NodeAttr(is_src=True, is_sink=True, role="r"))
    return Graph.build(
        name="d", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n], gates=[], edges=[],
        tick_period_s=1.0,
    )


def test_safe_format_preserves_unknown_keys() -> None:
    """测试名：test_safe_format_preserves_unknown_keys

    测试场景：_SafeFormat 对缺 key 原样保留（str.format 默认 KeyError）。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：调 "{a} {b}".format_map(_SafeFormat(a="1"))。
    预期结果：返回 "1 {b}"（b 未传→原样保留）。
    测试后清理：无。
    """
    out = "{a} {b}".format_map(_SafeFormat(a="1"))
    assert out == "1 {b}"


def test_default_worker_cmd_has_all_placeholders() -> None:
    """测试名：test_default_worker_cmd_has_all_placeholders

    测试场景：DEFAULT_WORKER_CMD 模板包含全部 5 个占位符。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：遍历 {sop_name}/{instance_id}/{node_name}/{proc}/{base_dir} 占位符，
      断言每个都在 DEFAULT_WORKER_CMD 内。
    预期结果：5 个断言全部成立。
    测试后清理：无。
    """
    for ph in ("{sop_name}", "{instance_id}", "{node_name}", "{proc}", "{base_dir}"):
        assert ph in DEFAULT_WORKER_CMD


def test_dispatch_returns_result_with_pid() -> None:
    """测试名：test_dispatch_returns_result_with_pid

    测试场景：dispatch 拉起一个立即退出的子进程，返回合法 DispatchResult。
    前置条件：_graph_with_node 构造 1 节点图 + State。
    是否使用 mock：No。
    测试步骤：1. ProcessDispatcher(worker_cmd="true")；2. pd.dispatch(g, state, node, "abc", /tmp/work)；
      3. pd.wait(dr.pid, 2.0) 等子进程退出。
    预期结果：dr.pid>0；dr.backend=="process"；dr.dispatch_id 非空；wait 返回 0。
    测试后清理：wait 后子进程已退。
    """
    g = _graph_with_node("a")
    node = g.node_index["a"]
    state = State(sop_name="d", instance_id="abc", graph_name="d")
    # 模板跑一个立即 true 的进程
    pd = ProcessDispatcher(worker_cmd="true", timeout=2.0)
    dr = pd.dispatch(g, state, node, "abc", Path("/tmp/work"))
    assert isinstance(dr, DispatchResult)
    assert dr.pid > 0
    assert dr.backend == "process"
    assert len(dr.dispatch_id) > 0
    # 等子进程退
    rc = pd.wait(dr.pid, timeout=2.0)
    assert rc == 0


def test_dispatch_with_template_substitutes_placeholders() -> None:
    """测试名：test_dispatch_with_template_substitutes_placeholders

    测试场景：worker_cmd 用模板 + 占位符能正确替换。
    前置条件：_graph_with_node + State。
    是否使用 mock：No。
    测试步骤：1. ProcessDispatcher(worker_cmd="echo sop={sop_name} instance={instance_id} node={node_name} proc={proc}")；
      2. dispatch；3. wait。
    预期结果：dispatch 不抛错；wait 返回 0。
    测试后清理：wait 后子进程已退。
    """
    g = _graph_with_node("a")
    node = g.node_index["a"]
    state = State(sop_name="d", instance_id="abc", graph_name="d")
    # 用 echo 重定向到 /dev/null；只验证 dispatch 不抛
    pd = ProcessDispatcher(
        worker_cmd="echo sop={sop_name} instance={instance_id} node={node_name} proc={proc}",
        timeout=2.0,
    )
    dr = pd.dispatch(g, state, node, "abc", Path("/tmp/work"))
    rc = pd.wait(dr.pid, timeout=2.0)
    assert rc == 0


def test_alive_returns_true_for_self_returns_false_for_zero() -> None:
    """测试名：test_alive_returns_true_for_self_returns_false_for_zero

    测试场景：ProcessDispatcher.alive 对 pid=0 返回 False，对自身 pid 返回 True。
    前置条件：默认 ProcessDispatcher 实例。
    是否使用 mock：No。
    测试步骤：pd.alive(0) / pd.alive(os.getpid())。
    预期结果：False / True。
    测试后清理：无。
    """
    pd = ProcessDispatcher()
    assert pd.alive(0) is False
    assert pd.alive(os.getpid()) is True


def test_wait_returns_minus_one_on_timeout() -> None:
    """测试名：test_wait_returns_minus_one_on_timeout

    测试场景：wait 等一个不存在 pid 在 timeout 内返回 -1。
    前置条件：默认 ProcessDispatcher 实例。
    是否使用 mock：No。
    测试步骤：pd.wait(pid=999_999_999, timeout=0.2)。
    预期结果：返回 -1。
    测试后清理：无（无副作用）。
    """
    pd = ProcessDispatcher()
    rc = pd.wait(pid=999_999_999, timeout=0.2)
    assert rc == -1
