"""Scheduler 主循环：find_ready_next_node / trigger_edge / tick。"""

from __future__ import annotations

from pathlib import Path

from src.kernel import Edge, Graph, StateLock

from ..dispatcher import Dispatcher
from ..monitor import Event, Monitor, get_monitor
from ..state_manager import State, StateManager, is_sop_done
from .cycle import detect_cycle_progress
from .evaluator import (
    EdgeCmdError,
    GateEvalError,
    evaluate_gates,
    run_edge_cmd,
    select_next_edges,
)


MAX_INFLIGHT_SKIPS = 10


class _TickAbort(Exception):
    """tick 中途终止：本 tick 到此为止，alarms 原样返回给调用方。

    阶段方法用它表达"提前返回"——正常返回 = 继续，抛出 = 终止。调用方不可能
    漏检（不存在 (value, error) 二元组被当成 value 误用的空间）。
    alarms 为空 list 表示本次终止不产生告警。
    """

    def __init__(self, alarms: list[str]) -> None:
        super().__init__("tick aborted")
        self.alarms = alarms


class Scheduler:
    """单实例调度器。

    每 instance 维护一个 _skip_count：当前节点 enter_cnt != exit_cnt（worker in-flight）
    时的连续跳过次数。超 MAX_INFLIGHT_SKIPS 报警（worker 疑似 leak）。
    """

    def __init__(
        self,
        graph: Graph,
        root: Path,
        dispatcher: Dispatcher | None = None,
        monitor: Monitor | None = None,
    ) -> None:
        self._graph = graph
        self._root = Path(root)
        self._dispatcher = dispatcher
        self._monitor = monitor or Monitor(get_monitor())
        self._skip_count: int = 0

    @property
    def graph(self) -> Graph:
        return self._graph

    @property
    def monitor(self) -> Monitor:
        return self._monitor

    def find_ready_next_node(
        self, state: State, base_dir: Path
    ) -> tuple[list[Edge], str | None]:
        """基于 state.current_node 选下一个候选节点（edges, primary）。

        返回 (edges, primary)：
        - edges：所有 match_gate + should_run_cmd 命中的边列表
          （parallel 时多条；非 parallel 时单条首条命中）。
        - primary = edges[0].onode，作为 transition 目标（parallel 时第一个 onode
          是状态机"主线"，其余 onode 由 dispatch 启动但不改变 current_node）。

        返回 ([], None)：current_node 为空 / 无匹配边 / 全部 manual 未 trigger。
        gate.op 异常 → 抛 GateEvalError（由调用方 _select_ready_edges 记
        SCH_EV_ERROR + GATE_EVAL_ERROR）。

        注意：本函数不判断"节点的进入条件"（iport）——iport 是下一节点的进入条件，
        由 tick 阶段 4 _check_iport（内部调 _iport_ready）检查，且只检查 primary。
        """
        if state.current_node is None:
            return [], None
        gate_values = evaluate_gates(
            self._graph, state.current_node, base_dir, state
        )
        edges = select_next_edges(
            self._graph, state.current_node, gate_values, state
        )
        if not edges:
            return [], None
        return edges, edges[0].onode

    def trigger_edge(self, state_file: Path, edge_name: str, approver: str) -> State:
        """manual 边外部触发。"""
        new_state = StateManager.trigger_edge(state_file, edge_name, approver)
        self._monitor.emit(
            Event.SCH_EV_EDGE_ADMITTED, edge=edge_name, approver=approver
        )
        return new_state

    def tick(self, sop_name: str, instance_id: str) -> list[str]:
        """单实例 tick。

        顺序（阶段 0 在取锁前执行，阶段 1 之后全程持有 StateLock）：
        0. locate       —— state.json 不存在 → SCH_EV_ERROR + STATE_NOT_FOUND（不取锁）
        1. read         —— 取 StateLock 读 state + 记 tick enter；is_sop_done → SCH_EV_SOP_DONE + 清零 skip_count + 返回
        2. sync_check   —— enter_cnt == exit_cnt 才往下走；否则 skip（in-flight）+ 超 MAX_INFLIGHT_SKIPS 报警
        3. select_edges —— 边放行条件（find_ready_next_node）
        4. iport_ready  —— 节点进入条件（仅 primary）
        5. run_cmd      —— 副作用动作
        6. advance      —— 转移 current_node + cycle 检测
        7. dispatch     —— 拉 worker 跑各 onode 的 proc（异步 spawn）
        8. write        —— 写盘 + 记 tick exit

        阶段 2/3/4/5 的提前终止由 _TickAbort 承载（携带本 tick 的 alarms），
        在 StateLock 内捕获后原样返回。阶段方法均假定锁已由本函数持有，不再取锁。
        """
        alarms: list[str] = []
        state_file = self._state_path(sop_name, instance_id)
        base_dir = state_file.parent / instance_id
        if not state_file.exists():
            self._monitor.emit(
                Event.SCH_EV_ERROR, where="tick", reason="state not found",
                sop=sop_name, instance=instance_id,
            )
            return ["STATE_NOT_FOUND"]

        with StateLock(state_file):
            state = StateManager.read(state_file)
            self._monitor.emit(
                Event.SCH_EV_TICK, phase="enter", sop=sop_name, instance=instance_id,
                current_node=state.current_node,
            )
            if is_sop_done(self._graph, state):
                self._monitor.emit(Event.SCH_EV_SOP_DONE, sop=sop_name, instance=instance_id)
                self._skip_count = 0
                return alarms

            try:
                self._sync_check_inflight(state, alarms)          # 阶段 2
                edges = self._select_ready_edges(state, base_dir)  # 阶段 3
                self._check_iport(edges[0].onode, base_dir)        # 阶段 4
                self._run_edge_cmds(edges, state, base_dir)        # 阶段 5
            except _TickAbort as abort:
                return abort.alarms

            new_state, alarms = self._advance(
                state, transition_edge=edges[0], alarms=alarms,
            )
            self._dispatch_edges(new_state, edges, instance_id, base_dir)
            StateManager.write(state_file, new_state)
            self._monitor.emit(
                Event.SCH_EV_TICK, phase="exit", sop=sop_name, instance=instance_id,
                current_node=new_state.current_node, selected_edge=edges[0].name,
            )
        return alarms

    def _sync_check_inflight(self, state: State, alarms: list[str]) -> None:
        """阶段 2 守卫：current_node 仍有 in-flight worker（enter_cnt != exit_cnt）
        → 本 tick 不推进。

        同步通过（current_node 为空，或 enter_cnt == exit_cnt）时把 self._skip_count
        清零；不同步时自增并发 SCH_EV_AWAITING_IPORT，超 MAX_INFLIGHT_SKIPS 追加
        WORKER_INFLIGHT_LEAK 并清零计数。alarms 就地追加。不同步一律以 _TickAbort
        结束本 tick。

        调用方已持 StateLock，本方法不再取锁。
        """
        cur = state.current_node
        if cur is None:
            self._skip_count = 0
            return
        enter = state.enter_cnt.get(cur, 0)
        exit_n = state.exit_cnt.get(cur, 0)
        if enter == exit_n:
            self._skip_count = 0
            return
        self._skip_count += 1
        self._monitor.emit(
            Event.SCH_EV_AWAITING_IPORT,
            current_node=cur, reason="in_flight_worker",
            skip_count=self._skip_count,
            enter=enter, exit=exit_n,
        )
        if self._skip_count > MAX_INFLIGHT_SKIPS:
            self._monitor.emit(
                Event.SCH_EV_TIMEOUT,
                what="in_flight_leak", node=cur,
                skip_count=self._skip_count,
            )
            alarms.append("WORKER_INFLIGHT_LEAK")
            self._skip_count = 0  # reset 避免连续累积
        raise _TickAbort(alarms)

    def _select_ready_edges(self, state: State, base_dir: Path) -> list[Edge]:
        """阶段 3 守卫：选本 tick 可走的边（match_gate + should_run_cmd 命中）。

        返回非空 list，edges[0] 即 find_ready_next_node 的 primary 边（transition
        目标与 tick exit 事件的 selected_edge 都取它）。
        gate.op 异常 → 记 SCH_EV_ERROR 并以 GATE_EVAL_ERROR 结束本 tick（不外抛）；
        无命中边 → 记 SCH_EV_EDGE_REJECTED 并结束本 tick，不产生告警。

        调用方已持 StateLock，本方法不再取锁。
        """
        try:
            edges, _primary = self.find_ready_next_node(state, base_dir)
        except GateEvalError as exc:
            self._monitor.emit(Event.SCH_EV_ERROR, where="evaluate_gates", msg=str(exc))
            raise _TickAbort(["GATE_EVAL_ERROR"]) from exc
        if not edges:
            self._monitor.emit(
                Event.SCH_EV_EDGE_REJECTED,
                current_node=state.current_node,
            )
            raise _TickAbort([])
        return edges

    def _check_iport(self, next_node: str, base_dir: Path) -> None:
        """阶段 4 守卫：next_node 的进入条件（iport）不满足 → 本 tick 不推进。

        不满足时记 SCH_EV_AWAITING_IPORT（current_node=next_node），不产生告警。
        调用方已持 StateLock，本方法不再取锁。
        """
        if self._iport_ready(next_node, base_dir):
            return
        self._monitor.emit(Event.SCH_EV_AWAITING_IPORT, current_node=next_node)
        raise _TickAbort([])

    def _run_edge_cmds(self, edges: list[Edge], state: State, base_dir: Path) -> None:
        """阶段 5 守卫：按 edges 顺序跑每条命中边的 edge.cmd（parallel 时多条）。

        任一条 cmd 抛 EdgeCmdError → 记 SCH_EV_ERROR 并结束本 tick；此时不
        transition / 不 dispatch / 不写盘。已跑过的 cmd 副作用保留，不回滚。
        调用方已持 StateLock，本方法不再取锁。
        """
        for edge in edges:
            try:
                self._run_edge_cmd(edge, state, base_dir)
            except EdgeCmdError as exc:
                self._monitor.emit(
                    Event.SCH_EV_ERROR, where="run_edge_cmd", msg=str(exc),
                )
                raise _TickAbort(["EDGE_CMD_ERROR"]) from exc

    def _dispatch_edges(
        self, new_state: State, edges: list[Edge], instance_id: str, base_dir: Path
    ) -> None:
        """阶段 7：拉起每条命中边 onode 的 worker（primary 由 _advance 接管）。"""
        for edge in edges:
            self._dispatch_if_any(new_state, edge.onode, instance_id, base_dir)

    def _run_edge_cmd(self, edge: Edge, state: State, base_dir: Path) -> None:
        """跑 edge.cmd；异常 → EdgeCmdError。"""
        run_edge_cmd(self._graph, edge, state, base_dir)

    def _iport_ready(self, node_name: str, base_dir: Path) -> bool:
        """判断 node_name 是否满足进入条件（iport = 节点开始工作所需的输入文件）。

        空 iport（节点无输入要求）直接视为满足；非空 iport 要求所有组至少一组
        文件全部存在（OR-of-AND）。

        本函数判断的是"节点的进入条件"——与 find_ready_next_node 中 gate 判断的
        "边的放行条件"是两件事：
          - iport 满足：节点可以开始干活
          - gate 通过：以 current_node 为起点的某条出边可以走
        两者都满足才会触发 cmd + transition。
        """
        from src.kernel import iport_satisfied

        node = self._graph.node_index.get(node_name)
        if node is None:
            return False
        if not node.iport:
            return True
        return bool(iport_satisfied(node, base_dir))

    def _advance(
        self, state: State, transition_edge: Edge, alarms: list[str]
    ) -> tuple[State, list[str]]:
        """transition + cycle 检测（按 exit_cnt）；返回 (new_state, alarms)。

        transition_edge：在 parallel 多边情况下，指定用哪条边的 onode 作 transition
        目标（通常 edges[0]，即 primary 边）。其他 parallel 边只走 dispatch，
        不改变 current_node。

        enter_cnt / exit_cnt 由 worker 维护（proc 入口 +1 / proc 出口 +1），
        scheduler 只读不写——见 tick() 的同步检查。
        """
        new_state = StateManager.transition(
            state, from_node=state.current_node, to_node=transition_edge.onode,
        )
        prog = detect_cycle_progress(self._graph, new_state)
        if prog.over_threshold:
            self._monitor.emit(
                Event.SCH_EV_TIMEOUT, what="cycle_threshold",
                max_count=prog.max_count,
            )
            alarms.append("CYCLE_OVER_THRESHOLD")
        return new_state, alarms

    def _dispatch_if_any(
        self,
        new_state: State,
        next_node: str,
        instance_id: str,
        base_dir: Path,
    ) -> None:
        """拉 worker 跑 next_node 的 proc（无 dispatcher 时跳过）。

        next_node 不在 node_index 里时：记 SCH_EV_ERROR(where="dispatch") 并跳过
        这条边，继续派发其余边。这条分支是防御性的——Graph.build 的校验器对
        悬空 onode 抛 E_BAD_EDGE_ENDPOINT，正常构造路径到不了这里。

        这里不新增 alarm 串：本分支不终止 tick（dispatch 在 _advance 之后、
        StateManager.write 之前，跳过它不影响 state 落盘），返回的 alarms 与
        正常运行完全一致；而 tick 级的 5 个 alarm 串是 Orchestrator.run / CLI 的
        契约，混入不可达的防御分支只会让契约变糊。
        """
        if self._dispatcher is None:
            return
        node = self._graph.node_index.get(next_node)
        if node is None:
            self._monitor.emit(
                Event.SCH_EV_ERROR,
                where="dispatch", reason="node not found", node=next_node,
            )
            return
        dr = self._dispatcher.dispatch(
            self._graph, new_state, node, instance_id, base_dir,
        )
        self._monitor.emit(
            Event.SCH_EV_NODE_START, node=next_node, pid=dr.pid if dr else 0,
        )

    def _state_path(self, sop_name: str, instance_id: str) -> Path:
        from src.kernel import state_path

        return state_path(root=self._root, sop_name=sop_name, instance_id=instance_id)