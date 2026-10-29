"""StateManager：State 读写、迁移、rollback、trigger_edge、claim。"""

from __future__ import annotations

import os
import time
import uuid
from pathlib import Path

from src.kernel import (
    Graph,
    StateLock,
    claim as kernel_claim,
    clear_claim as kernel_clear_claim,
    new_instance_id,
    read_state,
    write_state,
)
from src.kernel.ports import expand_oport_paths

from .schema import State


def _initial_state(sop_name: str, instance_id: str, graph: Graph) -> State:
    src_node = next((n for n in graph.nodes if n.attr.is_src), None)
    return State(
        sop_name=sop_name,
        instance_id=instance_id,
        graph_name=graph.name,
        current_node=src_node.name if src_node else None,
    )


class StateManager:
    """State 读写与迁移。对外公开方法均返回新 State / 写盘。"""

    @staticmethod
    def read(state_file: Path) -> State:
        """读 state.json；不存在返回空 State（current_node=None）。"""
        data = read_state(Path(state_file))
        if not data:
            return State(sop_name="", instance_id="", graph_name="")
        return State(
            sop_name=data.get("sop_name", ""),
            instance_id=data.get("instance_id", ""),
            graph_name=data.get("graph_name", ""),
            current_node=data.get("current_node"),
            history=tuple(tuple(h) for h in data.get("history", [])),
            triggered_edges=dict(data.get("triggered_edges", {})),
            enter_cnt=dict(data.get("enter_cnt", {})),
            exit_cnt=dict(data.get("exit_cnt", {})),
            last_output={k: tuple(v) for k, v in data.get("last_output", {}).items()},
        )

    @staticmethod
    def write(state_file: Path, state: State) -> None:
        """原子写（用 kernel write_state）。"""
        payload = {
            "sop_name": state.sop_name,
            "instance_id": state.instance_id,
            "graph_name": state.graph_name,
            "current_node": state.current_node,
            "history": [list(h) for h in state.history],
            "triggered_edges": dict(state.triggered_edges),
            "enter_cnt": dict(state.enter_cnt),
            "exit_cnt": dict(state.exit_cnt),
            "last_output": {k: list(v) for k, v in state.last_output.items()},
        }
        write_state(Path(state_file), payload)

    @staticmethod
    def create(state_file: Path, sop_name: str, graph: Graph, instance_id: str | None = None) -> State:
        """新建实例 State（current_node=src）并落盘。返回新 State。"""
        iid = instance_id or new_instance_id()
        state = _initial_state(sop_name, iid, graph)
        StateManager.write(Path(state_file), state)
        return state

    @staticmethod
    def transition(state: State, from_node: str, to_node: str) -> State:
        """校验 onode 在 from_node 的 successors 中后追加 history。"""
        return state.evolve(
            current_node=to_node,
            history=state.history + ((from_node, _iso_now()),),
        )

    @staticmethod
    def rollback(state: State, node_name: str, reason: str, base_dir: Path) -> State:
        """回退 current_node；清 triggered_edges / claim / last_output 文件；保留 enter_cnt / exit_cnt。"""
        # 1. 找 node_name 的前驱
        prev_idx = -1
        for i, (n, _) in enumerate(state.history):
            if n == node_name:
                prev_idx = i - 1
                break
        if prev_idx >= 0:
            current = state.history[prev_idx][0]
        else:
            current = None
        # 2. 删产物文件
        files = state.last_output.get(node_name, ())
        for f in files:
            try:
                p = Path(f)
                if p.is_absolute():
                    p.unlink(missing_ok=True)
            except OSError:
                pass
        # 3. 清 last_output
        new_last_output = {k: v for k, v in state.last_output.items() if k != node_name}
        # 4. 清 triggered_edges
        # 5. 清 claim
        try:
            kernel_clear_claim(_state_path_from_state(state))
        except Exception:
            pass
        return state.evolve(
            current_node=current,
            history=state.history + ((node_name, _iso_now()),),
            triggered_edges={},
            last_output=new_last_output,
        )

    @staticmethod
    def record_completion(
        state: State, node_name: str, output_files: tuple[str, ...] = ()
    ) -> State:
        """worker 完成上报：写 last_output + 追加 history (但 current_node 不变；由 scheduler 选边后再 transition)。"""
        new_last = dict(state.last_output)
        if output_files:
            new_last[node_name] = output_files
        return state.evolve(last_output=new_last)

    @staticmethod
    def trigger_edge(state_file: Path, edge_name: str, approver: str) -> State:
        """manual 边外部触发：写 triggered_edges[edge_name] = approver。"""
        state = StateManager.read(Path(state_file))
        new_triggered = dict(state.triggered_edges)
        new_triggered[edge_name] = approver or "anonymous"
        new_state = state.evolve(triggered_edges=new_triggered)
        StateManager.write(Path(state_file), new_state)
        return new_state

    @staticmethod
    def claim(state_file: Path, owner: str) -> bool:
        """认领 state_file。返回是否成功。"""
        return kernel_claim(Path(state_file), pid=os.getpid(), owner=owner)

    @staticmethod
    def record_enter(state: State, node_name: str) -> State:
        """worker 进入 node.proc 时调用：enter_cnt[node] += 1。"""
        new_enter = dict(state.enter_cnt)
        new_enter[node_name] = new_enter.get(node_name, 0) + 1
        return state.evolve(enter_cnt=new_enter)

    @staticmethod
    def record_exit(state: State, node_name: str) -> State:
        """worker 完成 node.proc 时调用（record_completion 之后）：exit_cnt[node] += 1。"""
        new_exit = dict(state.exit_cnt)
        new_exit[node_name] = new_exit.get(node_name, 0) + 1
        return state.evolve(exit_cnt=new_exit)


def _iso_now() -> str:
    return time.strftime("%Y-%m-%dT%H:%M:%S", time.gmtime())


def _state_path_from_state(state: State) -> Path:
    """从 state 字段推导 state.json 路径（仅 rollback 内辅助使用）。"""
    from src.kernel import state_path

    return state_path(sop_name=state.sop_name, instance_id=state.instance_id)