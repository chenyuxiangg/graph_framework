"""Scheduler 主循环测试。"""

from __future__ import annotations

import itertools
from pathlib import Path

import pytest

from src.components import Scheduler
from src.components.dispatcher import DispatchResult
from src.components.monitor import Monitor
from src.components.scheduler.core import MAX_INFLIGHT_SKIPS, _TickAbort
from src.components.scheduler.evaluator import EdgeCmdError, GateEvalError
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
    n_a = Node(name="a", iport=(), oport=(("f/a.md",),),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(("f/a.md",),), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    g_g = Gate(name="g_g", op="sch_always_true", enum_dir=(True,))
    e_ab = Edge(name="ab", inode="a", onode="b",
                driver="tick", gate="g_g", gate_value=True, cmd="")
    return Graph.build(
        name="sch", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b], gates=[g_g], edges=[e_ab],
        tick_period_s=1.0,
    )


def _register_gate() -> None:
    """注册 _build_graph 使用的 gate op（test 间复用同一 registry）。"""
    register_op(
        "sch_always_true",
        lambda base_dir, state, graph, gate: True,
    )


def test_tick_returns_state_not_found_when_missing(tmp_path: Path) -> None:
    """测试名：test_tick_returns_state_not_found_when_missing

    测试场景：tick 调用时不存在的 instance 路径 → 返回 STATE_NOT_FOUND alarm。
    前置条件：tmp_path；_build_graph；scheduler root=tmp_path。
    是否使用 mock：No。
    测试步骤：1. sch = Scheduler(graph=g, root=tmp_path)；2. sch.tick("sch", "no_instance")。
    预期结果：alarms 含 "STATE_NOT_FOUND"。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_graph()
    sch = Scheduler(graph=g, root=tmp_path)
    alarms = sch.tick("sch", "no_instance")
    assert "STATE_NOT_FOUND" in alarms


def test_tick_no_dispatcher_when_next_node_iport_missing(tmp_path: Path) -> None:
    """测试名：test_tick_no_dispatcher_when_next_node_iport_missing

    测试场景：next_node 的 iport 缺失时 tick 不应推进 state。

    新语义：iport 是 next_node 的进入条件。next_node=b 有非空 iport 且未写产物
    → _iport_ready(b) False → AWAITING_IPORT 返回，state 不动。
    前置条件：tmp_path；_build_graph（b 的 iport=(("f/a.md",),)）；手工写入 current_node="a" state。
    是否使用 mock：No。
    测试步骤：1. 写初始 state；2. sch.tick("sch", "abc")；3. 读 state 校验 current_node。
    预期结果：alarms == []；new_state.current_node == "a"（未推进）。
    测试后清理：pytest tmp_path 自动清理。
    """
    _register_gate()
    g = _build_graph()
    state_file = tmp_path / "sch" / "abc.json"
    state_file.parent.mkdir(parents=True)
    from src.components.state_manager import State, StateManager
    state = State(sop_name="sch", instance_id="abc", graph_name="sch",
                  current_node="a")
    StateManager.write(state_file, state)

    sch = Scheduler(graph=g, root=tmp_path, dispatcher=None)
    alarms = sch.tick("sch", "abc")
    assert alarms == []
    new_state = StateManager.read(state_file)
    assert new_state.current_node == "a"  # 未推进


def test_trigger_edge_writes_to_state(tmp_path: Path) -> None:
    """测试名：test_trigger_edge_writes_to_state

    测试场景：Scheduler.trigger_edge 把 edge_name + approver 写进 state.triggered_edges 并落盘。
    前置条件：tmp_path；_build_graph；写初始 state。
    是否使用 mock：No。
    测试步骤：1. sch.trigger_edge(state_file, "ab", "alice")；2. 读 state。
    预期结果：new_state.triggered_edges["ab"] == "alice"。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_graph()
    state_file = tmp_path / "sch" / "abc.json"
    state_file.parent.mkdir(parents=True)
    from src.components.state_manager import State, StateManager
    StateManager.write(
        state_file,
        State(sop_name="sch", instance_id="abc", graph_name="sch",
              current_node="a"),
    )
    sch = Scheduler(graph=g, root=tmp_path, dispatcher=None)
    sch.trigger_edge(state_file, "ab", "alice")
    new_state = StateManager.read(state_file)
    assert new_state.triggered_edges["ab"] == "alice"


# ---------------------------------------------------------------------------
# 以下为 tick 阶段拆分（_TickAbort / 4 道守卫 / _dispatch_edges）配套的测试。
# ---------------------------------------------------------------------------

_OP_SEQ = itertools.count()


class _RecordingLogger:
    """记录 emit 的 (event_name, fields) 序列，供断言事件内容与顺序。

    Monitor._level_method 会一次性取 debug / info / warn / error 四个属性，
    缺一个就在首次 emit 时 AttributeError——所以五个方法名都定义。
    """

    def __init__(self) -> None:
        self.events: list[tuple[str, dict[str, object]]] = []

    def debug(self, name: str, **fields: object) -> None:
        self.events.append((name, fields))

    def info(self, name: str, **fields: object) -> None:
        self.events.append((name, fields))

    def warning(self, name: str, **fields: object) -> None:
        self.events.append((name, fields))

    def warn(self, name: str, **fields: object) -> None:
        self.events.append((name, fields))

    def error(self, name: str, **fields: object) -> None:
        self.events.append((name, fields))


class _RecordingDispatcher:
    """鸭子类型的 Dispatcher：按顺序记录被 dispatch 的 node.name。"""

    def __init__(self) -> None:
        self.dispatched: list[str] = []

    def dispatch(
        self,
        graph: Graph,
        state: State,
        node: Node,
        instance_id: str,
        work_dir: Path,
    ) -> DispatchResult:
        self.dispatched.append(node.name)
        return DispatchResult(pid=4242, dispatch_id="d1", backend="process")

    def alive(self, pid: int) -> bool:
        return True

    def wait(self, pid: int, timeout: float) -> int:
        return 0


def _op_name(prefix: str) -> str:
    """生成进程内唯一的 op 名（registry 全局，重复注册抛 DuplicateRegistrationError）。"""
    return f"{prefix}_{next(_OP_SEQ)}"


def _boom(**kwargs: object) -> None:
    """统一异常 op。"""
    raise RuntimeError("boom")


def _true_gate_op() -> str:
    """注册并返回恒 True 的 gate op 名。"""
    name = _op_name("gate_true")
    register_op(name, lambda **kwargs: True)
    return name


def _false_gate_op() -> str:
    """注册并返回恒 False 的 gate op 名。"""
    name = _op_name("gate_false")
    register_op(name, lambda **kwargs: False)
    return name


def _raise_gate_op() -> str:
    """注册并返回抛异常的 gate op 名。"""
    name = _op_name("gate_raise")
    register_op(name, _boom)
    return name


def _raise_cmd_op() -> str:
    """注册并返回抛异常的 edge.cmd op 名。"""
    name = _op_name("cmd_raise")
    register_op(name, _boom)
    return name


def _state(**kw: object) -> State:
    """构造测试用 State（补齐 sop_name / instance_id / graph_name 三个必填字段）。"""
    base: dict[str, object] = {
        "sop_name": "sch", "instance_id": "abc", "graph_name": "sch",
    }
    base.update(kw)
    return State(**base)  # type: ignore[arg-type]


def _write_state(root: Path, state: State) -> Path:
    """把 state 落到 <root>/<sop>/<instance>.json，返回 state_file 路径。"""
    state_file = root / state.sop_name / f"{state.instance_id}.json"
    state_file.parent.mkdir(parents=True, exist_ok=True)
    StateManager.write(state_file, state)
    return state_file


def _build_chain_graph(gate_op: str, cmd: str = "") -> Graph:
    """a(is_src) -> b(is_sink) 单边图；b 的 iport 非空（用于阶段 3 守卫）。"""
    n_a = Node(name="a", iport=(), oport=(),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(("f/a.md",),), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    g1 = Gate(name="g1", op=gate_op, enum_dir=(True,))
    e_ab = Edge(name="ab", inode="a", onode="b",
                driver="tick", gate="g1", gate_value=True, cmd=cmd)
    return Graph.build(
        name="sch_chain", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b], gates=[g1], edges=[e_ab],
        tick_period_s=1.0,
    )


def _build_parallel_graph(gate_op: str, cmd_ab: str = "", cmd_ac: str = "") -> Graph:
    """a(is_src) 并行走到 b(is_sink) 与 c 的双边图（每条边独立 gate，避免 E_MULTI_MATCH_GATE）。"""
    n_a = Node(name="a", iport=(), oport=(),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    n_c = Node(name="c", iport=(), oport=(), attr=NodeAttr(role="r"))
    g1 = Gate(name="g1", op=gate_op, enum_dir=(True,))
    g2 = Gate(name="g2", op=gate_op, enum_dir=(True,))
    e_ab = Edge(name="ab", inode="a", onode="b", driver="tick",
                gate="g1", gate_value=True, cmd=cmd_ab, parallel=True)
    e_ac = Edge(name="ac", inode="a", onode="c", driver="tick",
                gate="g2", gate_value=True, cmd=cmd_ac, parallel=True)
    return Graph.build(
        name="sch_par", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=[n_a, n_b, n_c], gates=[g1, g2], edges=[e_ab, e_ac],
        tick_period_s=1.0,
    )


def _make_scheduler(
    graph: Graph, root: Path, dispatcher: object = None
) -> tuple[Scheduler, _RecordingLogger]:
    """构造注入 _RecordingLogger 的 Scheduler，返回 (sch, logger)。"""
    logger = _RecordingLogger()
    sch = Scheduler(
        graph=graph, root=root, dispatcher=dispatcher, monitor=Monitor(logger),
    )
    return sch, logger


def _names(logger: _RecordingLogger) -> list[str]:
    """事件名序列（按 emit 顺序）。"""
    return [name for name, _ in logger.events]


def _fields(logger: _RecordingLogger, event_name: str) -> list[dict[str, object]]:
    """某个事件名对应的全部 fields（按 emit 顺序）。"""
    return [f for name, f in logger.events if name == event_name]


def test_tick_abort_carries_alarms_through_raise() -> None:
    """测试名：test_tick_abort_carries_alarms_through_raise

    测试场景：_TickAbort 抛出 / 捕获后，.alarms 仍是同一个 list 对象
    （阶段守卫把 alarms 就地追加后原样交回，tick 靠这一条保持返回对象同一性）。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：1. alarms = ["A", "B"]；2. pytest.raises 捕获 raise _TickAbort(alarms)；
              3. 校验 isinstance 与 .alarms 同一性。
    预期结果：exc.value 是 Exception 子类；exc.value.alarms is alarms。
    测试后清理：无外部资源。
    """
    alarms = ["A", "B"]
    with pytest.raises(_TickAbort) as exc:
        raise _TickAbort(alarms)
    assert isinstance(exc.value, Exception)
    assert exc.value.alarms is alarms


def test_sync_check_inflight_skips_when_worker_running(tmp_path: Path) -> None:
    """测试名：test_sync_check_inflight_skips_when_worker_running

    测试场景：current_node 仍有 in-flight worker（enter_cnt != exit_cnt）时
    _sync_check_inflight 抛 _TickAbort，skip_count 累加并记 AWAITING_IPORT。
    前置条件：tmp_path；_build_chain_graph(恒 True gate)。
    是否使用 mock：No（用 _RecordingLogger 观测事件）。
    测试步骤：1. 构造 enter_cnt={"a":1}/exit_cnt={} 的 State；2. 调 _sync_check_inflight。
    预期结果：抛 _TickAbort；.alarms is alarms 且 == []；sch._skip_count == 1；
              AWAITING_IPORT 的 fields 为 {current_node:"a", reason:"in_flight_worker",
              skip_count:1, enter:1, exit:0}。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_true_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)
    alarms: list[str] = []
    state = _state(current_node="a", enter_cnt={"a": 1}, exit_cnt={})

    with pytest.raises(_TickAbort) as exc:
        sch._sync_check_inflight(state, alarms)

    assert exc.value.alarms is alarms
    assert alarms == []
    assert sch._skip_count == 1
    assert _fields(logger, "sch_ev_awaiting_iport") == [
        {
            "current_node": "a", "reason": "in_flight_worker",
            "skip_count": 1, "enter": 1, "exit": 0,
        }
    ]


def test_sync_check_inflight_reports_leak_and_resets(tmp_path: Path) -> None:
    """测试名：test_sync_check_inflight_reports_leak_and_resets

    测试场景：连续 in-flight 次数超 MAX_INFLIGHT_SKIPS（严格大于）时报
    WORKER_INFLIGHT_LEAK 并把 skip_count 清零，TIMEOUT 事件在 AWAITING_IPORT 之后。
    前置条件：tmp_path；_build_chain_graph；sch._skip_count 预置为 MAX_INFLIGHT_SKIPS。
    是否使用 mock：No。
    测试步骤：1. 预置 _skip_count = MAX_INFLIGHT_SKIPS；2. 用同样 in-flight 状态调守卫。
    预期结果：抛 _TickAbort 且 .alarms == ["WORKER_INFLIGHT_LEAK"]；_skip_count == 0；
              sch_ev_timeout 的 fields 为 {what:"in_flight_leak", node:"a", skip_count:11}；
              timeout 事件下标大于 awaiting_iport 事件下标。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_true_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)
    sch._skip_count = MAX_INFLIGHT_SKIPS
    alarms: list[str] = []
    state = _state(current_node="a", enter_cnt={"a": 1}, exit_cnt={})

    with pytest.raises(_TickAbort) as exc:
        sch._sync_check_inflight(state, alarms)

    assert exc.value.alarms == ["WORKER_INFLIGHT_LEAK"]
    assert sch._skip_count == 0
    assert _fields(logger, "sch_ev_timeout") == [
        {"what": "in_flight_leak", "node": "a", "skip_count": MAX_INFLIGHT_SKIPS + 1},
    ]
    names = _names(logger)
    assert names.index("sch_ev_timeout") > names.index("sch_ev_awaiting_iport")


def test_sync_check_inflight_resets_skip_count_when_synced(tmp_path: Path) -> None:
    """测试名：test_sync_check_inflight_resets_skip_count_when_synced

    测试场景：两条"同步 OK"路径（current_node 为空 / enter_cnt == exit_cnt）
    都必须把 skip_count 清零——这行容易被误当作冗余而删掉。
    前置条件：tmp_path；_build_chain_graph；两条子用例各预置 _skip_count = 3。
    是否使用 mock：No。
    测试步骤：1. (a) enter==exit 调守卫；2. (b) current_node=None 调守卫。
    预期结果：两次都正常返回 None（不抛），sch._skip_count == 0，无事件。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_true_gate_op())

    sch, logger = _make_scheduler(g, tmp_path)
    sch._skip_count = 3
    assert sch._sync_check_inflight(
        _state(current_node="a", enter_cnt={"a": 2}, exit_cnt={"a": 2}), []
    ) is None
    assert sch._skip_count == 0
    assert logger.events == []

    sch, logger = _make_scheduler(g, tmp_path)
    sch._skip_count = 3
    assert sch._sync_check_inflight(_state(current_node=None), []) is None
    assert sch._skip_count == 0
    assert logger.events == []


def test_select_ready_edges_reports_gate_eval_error(tmp_path: Path) -> None:
    """测试名：test_select_ready_edges_reports_gate_eval_error

    测试场景：gate.op 抛异常时 _select_ready_edges 记 SCH_EV_ERROR 并以
    GATE_EVAL_ERROR 抛 _TickAbort（不把 GateEvalError 外抛给 tick）。
    前置条件：tmp_path；_build_chain_graph(抛异常的 gate op)。
    是否使用 mock：No。
    测试步骤：1. 构造 current_node="a" 的 State；2. 调 _select_ready_edges。
    预期结果：抛 _TickAbort 且 .alarms == ["GATE_EVAL_ERROR"]；唯一事件是
              sch_ev_error 且 where == "evaluate_gates"；.__cause__ 是 GateEvalError。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_raise_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)

    with pytest.raises(_TickAbort) as exc:
        sch._select_ready_edges(_state(current_node="a"), tmp_path)

    assert exc.value.alarms == ["GATE_EVAL_ERROR"]
    assert isinstance(exc.value.__cause__, GateEvalError)
    assert _names(logger) == ["sch_ev_error"]
    assert _fields(logger, "sch_ev_error")[0]["where"] == "evaluate_gates"


def test_select_ready_edges_reports_no_edge_match(tmp_path: Path) -> None:
    """测试名：test_select_ready_edges_reports_no_edge_match

    测试场景：gate 不放行时没有命中边 → 记 SCH_EV_EDGE_REJECTED 并以空告警结束，
    事件里的 current_node 是 State 的当前节点（不是候选 onode）。
    前置条件：tmp_path；_build_chain_graph(恒 False gate op)。
    是否使用 mock：No。
    测试步骤：1. 构造 current_node="a" 的 State；2. 调 _select_ready_edges。
    预期结果：抛 _TickAbort 且 .alarms == []；
              sch_ev_edge_rejected 的 fields == {"current_node": "a"}。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_false_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)

    with pytest.raises(_TickAbort) as exc:
        sch._select_ready_edges(_state(current_node="a"), tmp_path)

    assert exc.value.alarms == []
    assert _fields(logger, "sch_ev_edge_rejected") == [{"current_node": "a"}]


def test_select_ready_edges_returns_primary_edge(tmp_path: Path) -> None:
    """测试名：test_select_ready_edges_returns_primary_edge

    测试场景：命中边时返回非空 list，edges[0] 即原 (edges, primary) 元组里的
    primary 边——tick 用 edges[0].onode 作 transition 目标、edges[0].name 作
    selected_edge，这个契约必须被钉住。
    前置条件：tmp_path；_build_chain_graph(恒 True gate)。
    是否使用 mock：No。
    测试步骤：1. 构造 current_node="a" 的 State；2. 调 _select_ready_edges。
    预期结果：返回长度为 1 的 list；edges[0].name == "ab"、edges[0].onode == "b"；
              无任何事件。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_true_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)

    edges = sch._select_ready_edges(_state(current_node="a"), tmp_path)

    assert len(edges) == 1
    assert edges[0].name == "ab"
    assert edges[0].onode == "b"
    assert logger.events == []


def test_check_iport_aborts_when_next_node_iport_missing(tmp_path: Path) -> None:
    """测试名：test_check_iport_aborts_when_next_node_iport_missing

    测试场景：next_node 的 iport 产物缺失 → 记 AWAITING_IPORT（无 reason /
    skip_count / enter / exit 字段，与 in-flight 版本不同）并以空告警结束；
    产物补齐后同一调用正常返回。
    前置条件：tmp_path；_build_chain_graph（b 的 iport = (("f/a.md",),)）。
    是否使用 mock：No。
    测试步骤：1. iport 未写时调 _check_iport；2. 写 base_dir/f/a.md 后再调一次。
    预期结果：第 1 次抛 _TickAbort 且 .alarms == []，事件为
              {"current_node": "b"}；第 2 次返回 None 且不再有事件。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_true_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)
    base_dir = tmp_path / "sch" / "abc"

    with pytest.raises(_TickAbort) as exc:
        sch._check_iport("b", base_dir)

    assert exc.value.alarms == []
    assert _fields(logger, "sch_ev_awaiting_iport") == [{"current_node": "b"}]

    (base_dir / "f").mkdir(parents=True, exist_ok=True)
    (base_dir / "f" / "a.md").write_text("x", encoding="utf-8")

    assert sch._check_iport("b", base_dir) is None
    assert _fields(logger, "sch_ev_awaiting_iport") == [{"current_node": "b"}]


def test_run_edge_cmds_aborts_on_cmd_error(tmp_path: Path) -> None:
    """测试名：test_run_edge_cmds_aborts_on_cmd_error

    测试场景：edge.cmd 抛异常时 _run_edge_cmds 记 SCH_EV_ERROR 并以
    EDGE_CMD_ERROR 抛 _TickAbort。
    前置条件：tmp_path；_build_chain_graph(恒 True gate, 抛异常的 cmd)。
    是否使用 mock：No。
    测试步骤：1. 取该图的命中边；2. 调 _run_edge_cmds。
    预期结果：抛 _TickAbort 且 .alarms == ["EDGE_CMD_ERROR"]；唯一事件是
              sch_ev_error 且 where == "run_edge_cmd"；.__cause__ 是 EdgeCmdError。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_true_gate_op(), cmd=_raise_cmd_op())
    sch, logger = _make_scheduler(g, tmp_path)
    edges = g.edges

    with pytest.raises(_TickAbort) as exc:
        sch._run_edge_cmds(list(edges), _state(current_node="a"), tmp_path)

    assert exc.value.alarms == ["EDGE_CMD_ERROR"]
    assert isinstance(exc.value.__cause__, EdgeCmdError)
    assert _names(logger) == ["sch_ev_error"]
    assert _fields(logger, "sch_ev_error")[0]["where"] == "run_edge_cmd"


def test_run_edge_cmds_keeps_earlier_cmd_side_effect_on_later_error(tmp_path: Path) -> None:
    """测试名：test_run_edge_cmds_keeps_earlier_cmd_side_effect_on_later_error

    测试场景：parallel 双边中前一条 cmd 写产物、后一条 cmd 抛异常 → 抛
    EDGE_CMD_ERROR，且前一条的副作用保留（不做补偿回滚），调用顺序为 ab、ac。
    前置条件：tmp_path；_build_parallel_graph(恒 True gate, cmd_ab 写 marker, cmd_ac 抛异常)。
    是否使用 mock：No。
    测试步骤：1. 注册 marker cmd；2. 调 _run_edge_cmds(两条边)。
    预期结果：抛 _TickAbort 且 .alarms == ["EDGE_CMD_ERROR"]；
              order == ["ab", "ac"]；marker 文件存在。
    测试后清理：pytest tmp_path 自动清理。
    """
    order: list[str] = []
    marker = tmp_path / "marker.txt"

    def _mark(base_dir: Path, state: State, graph: Graph, edge: Edge) -> None:
        order.append(edge.name)
        marker.write_text("hit", encoding="utf-8")

    def _mark_then_raise(
        base_dir: Path, state: State, graph: Graph, edge: Edge
    ) -> None:
        order.append(edge.name)
        raise RuntimeError("boom")

    cmd_ok = _op_name("cmd_mark")
    register_op(cmd_ok, _mark)
    cmd_bad = _op_name("cmd_mark_raise")
    register_op(cmd_bad, _mark_then_raise)
    g = _build_parallel_graph(_true_gate_op(), cmd_ab=cmd_ok, cmd_ac=cmd_bad)
    sch, _logger = _make_scheduler(g, tmp_path)

    with pytest.raises(_TickAbort) as exc:
        sch._run_edge_cmds(list(g.edges), _state(current_node="a"), tmp_path)

    assert exc.value.alarms == ["EDGE_CMD_ERROR"]
    assert order == ["ab", "ac"]
    assert marker.exists()


def test_dispatch_edges_dispatches_every_onode(tmp_path: Path) -> None:
    """测试名：test_dispatch_edges_dispatches_every_onode

    测试场景：_dispatch_edges 按 edges 顺序对每条 onode 拉 worker（含 secondary
    parallel onode）；无 dispatcher 时整个阶段静默跳过。
    前置条件：tmp_path；_build_parallel_graph；_RecordingDispatcher。
    是否使用 mock：No（自建记录型 dispatcher，非 mock 库）。
    测试步骤：1. 有 dispatcher 时调 _dispatch_edges；2. 无 dispatcher 时再调一次。
    预期结果：(1) dispatched == ["b", "c"]，两条 SCH_EV_NODE_START(pid=4242)；
              (2) 返回 None、不抛、无 node_start 事件。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_parallel_graph(_true_gate_op())
    new_state = _state(current_node="b")

    disp = _RecordingDispatcher()
    sch, logger = _make_scheduler(g, tmp_path, dispatcher=disp)
    assert sch._dispatch_edges(new_state, list(g.edges), "abc", tmp_path) is None
    assert disp.dispatched == ["b", "c"]
    assert _fields(logger, "sch_ev_node_start") == [
        {"node": "b", "pid": 4242}, {"node": "c", "pid": 4242},
    ]

    sch, logger = _make_scheduler(g, tmp_path, dispatcher=None)
    assert sch._dispatch_edges(new_state, list(g.edges), "abc", tmp_path) is None
    assert _fields(logger, "sch_ev_node_start") == []


def test_tick_advances_state_and_emits_events_in_order(tmp_path: Path) -> None:
    """测试名：test_tick_advances_state_and_emits_events_in_order

    测试场景：parallel 双边成功 tick —— cmd → advance → dispatch → write →
    exit 事件的完整顺序，并落盘到 edges[0].onode。
    前置条件：tmp_path；_build_parallel_graph(恒 True gate, 无 cmd)；
              写 current_node="a" 的 state；_RecordingDispatcher。
    是否使用 mock：No。
    测试步骤：1. 调 tick("sch_par", "abc")；2. 读回 state.json。
    预期结果：alarms == []；事件名序列恰为
              [tick, node_start, node_start, tick]；首个 tick 事件 phase=="enter"、
              末个 phase=="exit" 且 current_node=="b"、selected_edge=="ab"；
              落盘 current_node == "b" 且 history 增加一条。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_parallel_graph(_true_gate_op())
    state = _state(sop_name="sch_par", current_node="a")
    state_file = _write_state(tmp_path, state)
    disp = _RecordingDispatcher()
    sch, logger = _make_scheduler(g, tmp_path, dispatcher=disp)

    alarms = sch.tick("sch_par", "abc")

    assert alarms == []
    assert _names(logger) == [
        "sch_ev_tick", "sch_ev_node_start", "sch_ev_node_start", "sch_ev_tick",
    ]
    assert logger.events[0][1]["phase"] == "enter"
    assert logger.events[-1][1] == {
        "phase": "exit", "sop": "sch_par", "instance": "abc",
        "current_node": "b", "selected_edge": "ab",
    }
    new_state = StateManager.read(state_file)
    assert new_state.current_node == "b"
    assert len(new_state.history) == 1


def test_tick_returns_gate_eval_error_without_writing_state(tmp_path: Path) -> None:
    """测试名：test_tick_returns_gate_eval_error_without_writing_state

    测试场景：gate.op 抛异常时 tick 在写盘前终止 —— state.json 字节不变。
    前置条件：tmp_path；_build_chain_graph(抛异常的 gate op)；
              写 current_node="a" 的 state。
    是否使用 mock：No。
    测试步骤：1. 记录 state.json 原始字节；2. 调 tick；3. 比对字节。
    预期结果：alarms == ["GATE_EVAL_ERROR"]；state.json 字节完全相同；
              current_node 仍为 "a"；无 phase=="exit" 的 tick 事件。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_raise_gate_op())
    state_file = _write_state(tmp_path, _state(current_node="a"))
    before = state_file.read_bytes()
    sch, logger = _make_scheduler(g, tmp_path)

    alarms = sch.tick("sch", "abc")

    assert alarms == ["GATE_EVAL_ERROR"]
    assert state_file.read_bytes() == before
    assert StateManager.read(state_file).current_node == "a"
    assert all(f.get("phase") != "exit" for _, f in logger.events)


def test_tick_sop_done_emits_and_resets_skip_count(tmp_path: Path) -> None:
    """测试名：test_tick_sop_done_emits_and_resets_skip_count

    测试场景：current_node 已是 sink（且 sink 无 oport）→ 记 SCH_EV_SOP_DONE、
    无条件把 skip_count 清零并返回空告警。
    前置条件：tmp_path；_build_chain_graph；写 current_node="b" 的 state；
              预置 sch._skip_count = 5。
    是否使用 mock：No。
    测试步骤：1. 调 tick；2. 校验返回值、事件与 skip_count。
    预期结果：alarms == []；sch_ev_sop_done 出现在 enter 事件之后；
              sch._skip_count == 0；state.json 未被改写。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_true_gate_op())
    state_file = _write_state(tmp_path, _state(current_node="b"))
    before = state_file.read_bytes()
    sch, logger = _make_scheduler(g, tmp_path)
    sch._skip_count = 5

    alarms = sch.tick("sch", "abc")

    assert alarms == []
    assert _fields(logger, "sch_ev_sop_done") == [
        {"sop": "sch", "instance": "abc"},
    ]
    names = _names(logger)
    assert names.index("sch_ev_sop_done") > names.index("sch_ev_tick")
    assert sch._skip_count == 0
    assert state_file.read_bytes() == before


# ---------------------------------------------------------------------------
# 以下为公开边选择谓词 find_ready_next_node 与 _dispatch_if_any 悬空 onode 兜底的测试。
# ---------------------------------------------------------------------------


def test_find_ready_next_node_returns_empty_when_current_node_is_none(
    tmp_path: Path,
) -> None:
    """测试名：test_find_ready_next_node_returns_empty_when_current_node_is_none

    测试场景：state.current_node 为 None（SOP 未起跑）时直接返回 ([], None)，
    连 gate.op 都不该被调用——短路发生在求值之前。
    前置条件：tmp_path；_build_chain_graph(gate op 把求值记录到 calls)。
    是否使用 mock：No（自建计数 op，非 mock 库）。
    测试步骤：1. 构造 current_node=None 的 State；2. 调 find_ready_next_node。
    预期结果：返回 ([], None)；calls == []（gate 未求值）；无任何事件。
    测试后清理：pytest tmp_path 自动清理。
    """
    calls: list[str] = []

    def _counting_gate_op(
        base_dir: Path, state: State, graph: Graph, gate: Gate
    ) -> bool:
        calls.append(gate.name)
        return True

    gate_op = _op_name("gate_count")
    register_op(gate_op, _counting_gate_op)
    g = _build_chain_graph(gate_op)
    sch, logger = _make_scheduler(g, tmp_path)

    edges, primary = sch.find_ready_next_node(_state(current_node=None), tmp_path)

    assert (edges, primary) == ([], None)
    assert calls == []
    assert logger.events == []


def test_find_ready_next_node_returns_edges_and_primary(tmp_path: Path) -> None:
    """测试名：test_find_ready_next_node_returns_edges_and_primary

    测试场景：命中边时返回 (edges, primary)，且 primary 必须等于 edges[0].onode
    ——_select_ready_edges / _advance / tick 的 exit 事件都靠这条契约取 transition 目标。
    前置条件：tmp_path；_build_chain_graph(恒 True gate)。
    是否使用 mock：No。
    测试步骤：1. 构造 current_node="a" 的 State；2. 调 find_ready_next_node。
    预期结果：边名序列 == ["ab"]；primary == edges[0].onode == "b"；无任何事件。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_true_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)

    edges, primary = sch.find_ready_next_node(_state(current_node="a"), tmp_path)

    assert [edge.name for edge in edges] == ["ab"]
    assert primary == edges[0].onode == "b"
    assert logger.events == []


def test_find_ready_next_node_returns_empty_when_no_edge_matches(
    tmp_path: Path,
) -> None:
    """测试名：test_find_ready_next_node_returns_empty_when_no_edge_matches

    测试场景：gate 已求值但不命中任何边 → 返回 ([], None)；与 current_node 为空
    走的是同一条 "无候选边" 返回路径（告警由 _select_ready_edges 记，此处不记）。
    前置条件：tmp_path；_build_chain_graph(恒 False gate)。
    是否使用 mock：No。
    测试步骤：1. 构造 current_node="a" 的 State；2. 调 find_ready_next_node。
    预期结果：返回 ([], None)；无任何事件（本函数不记 alarm）。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_false_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)

    edges, primary = sch.find_ready_next_node(_state(current_node="a"), tmp_path)

    assert (edges, primary) == ([], None)
    assert logger.events == []


def test_find_ready_next_node_propagates_gate_eval_error(tmp_path: Path) -> None:
    """测试名：test_find_ready_next_node_propagates_gate_eval_error

    测试场景：gate.op 抛异常时 GateEvalError 必须一路冒到调用方 _select_ready_edges
    （那里负责记 SCH_EV_ERROR + 转成 GATE_EVAL_ERROR 的 _TickAbort）——本函数
    不吞、不包、不改写。若有人在此处加吞异常的处理，此用例会失败。
    前置条件：tmp_path；_build_chain_graph(抛异常的 gate op)。
    是否使用 mock：No。
    测试步骤：1. 构造 current_node="a" 的 State；2. 调 find_ready_next_node。
    预期结果：抛 GateEvalError（.__cause__ 是 gate.op 抛的原始异常）；
              无任何事件（记事件是 _select_ready_edges 的职责）。
    测试后清理：pytest tmp_path 自动清理。
    """
    g = _build_chain_graph(_raise_gate_op())
    sch, logger = _make_scheduler(g, tmp_path)

    with pytest.raises(GateEvalError) as exc:
        sch.find_ready_next_node(_state(current_node="a"), tmp_path)

    assert isinstance(exc.value.__cause__, RuntimeError)
    assert logger.events == []


def test_dispatch_edges_skips_missing_node_and_emits_error(tmp_path: Path) -> None:
    """测试名：test_dispatch_edges_skips_missing_node_and_emits_error

    测试场景：边的 onode 不在 node_index 里（正常构造路径被 E_BAD_EDGE_ENDPOINT
    拦掉，这里直接用 Graph 构造器绕过校验构造该图）——_dispatch_if_any 必须记
    SCH_EV_ERROR 并跳过这条边、继续派发其余边，而不是抛 KeyError 打死整个 tick。
    前置条件：tmp_path；手工构造 nodes=(a, b) 而 edges 含 onode="ghost" 的 Graph；
              _RecordingDispatcher。
    是否使用 mock：No（自建记录型 dispatcher，非 mock 库）。
    测试步骤：1. 构造悬空图；2. 调 _dispatch_edges（边序 a_ghost、a_b）。
    预期结果：不抛异常；dispatched == ["b"]（ghost 被跳过、b 照常派发）；
              恰好一条 sch_ev_error == {where:"dispatch", reason:"node not found",
              node:"ghost"}；node_start 只有 {"node":"b","pid":4242}。
    测试后清理：pytest tmp_path 自动清理。
    """
    n_a = Node(name="a", iport=(), oport=(),
               attr=NodeAttr(is_src=True, role="r"))
    n_b = Node(name="b", iport=(), oport=(),
               attr=NodeAttr(is_sink=True, role="r"))
    # 直接调 Graph 构造器（不走 Graph.build）以绕过 E_BAD_EDGE_ENDPOINT 校验。
    g = Graph(
        name="sch_ghost", graph_mode=GraphMode.DIRECTED_NOCYCLE,
        pre_handle="", post_handle="",
        nodes=(n_a, n_b), gates=(),
        edges=(
            Edge(name="a_ghost", inode="a", onode="ghost", driver="tick",
                 gate="g_absent", gate_value=True, cmd=""),
            Edge(name="a_b", inode="a", onode="b", driver="tick",
                 gate="g_absent", gate_value=True, cmd=""),
        ),
        tick_period_s=1.0,
    )
    disp = _RecordingDispatcher()
    sch, logger = _make_scheduler(g, tmp_path, dispatcher=disp)

    result = sch._dispatch_edges(
        _state(current_node="a"), list(g.edges), "abc", tmp_path
    )

    assert result is None
    assert disp.dispatched == ["b"]
    assert _fields(logger, "sch_ev_error") == [
        {"where": "dispatch", "reason": "node not found", "node": "ghost"},
    ]
    assert _fields(logger, "sch_ev_node_start") == [{"node": "b", "pid": 4242}]
