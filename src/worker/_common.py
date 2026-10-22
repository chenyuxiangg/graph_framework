"""Worker 进程共用 helper。

record_lifecycle：串行 read → record_enter → proc → record_completion → record_exit
整段持 StateLock 防并行 worker 互踩 state.json。

parallel fan-out 场景下，多个 worker 子进程同时跑；不带锁的话 read-modify-write
竞态会导致某个 worker 的 enter/exit 计数丢失（被另一个 worker 覆盖）。
"""

from __future__ import annotations

import sys
from pathlib import Path
from typing import Callable

from src.components.state_manager import StateManager
from src.kernel import StateLock, load_graph, resolve_op, state_path
from src.worker import PROC_BYPASS_NAME


def record_lifecycle(
    sop_name: str,
    instance_id: str,
    node_name: str,
    proc_name: str,
    base_dir: Path,
    extra_pre_record: Callable | None = None,
) -> int:
    """worker 进程的标准生命周期（带锁）。

    1. 解析 config + 读 state（持 StateLock）。
    2. record_enter（持锁）。
    3. （释放锁跑 proc —— proc 可能耗时长，阻塞其他 worker 不合理）
    4. record_completion + record_exit（持锁）。
    5. 返回 exit code（0 成功 / 1 proc 失败）。

    锁范围刻意避开 proc 调用本身——proc 内部通常写文件（产物），
    不动 state；锁只保护 enter/exit 计数与 last_output 这种共享 metadata。
    """
    graph = load_graph(_find_config(sop_name))
    sf = state_path(sop_name=sop_name, instance_id=instance_id)
    proc_name = proc_name or PROC_BYPASS_NAME

    # Phase 1：持锁读 state + record_enter + 写回
    with StateLock(sf):
        state = StateManager.read(sf)
        state = StateManager.record_enter(state, node_name)
        StateManager.write(sf, state)

    # Phase 2：跑 proc（不持锁 —— proc 可能耗时长）
    proc_fn = resolve_op(proc_name)
    try:
        output_files = proc_fn(
            base_dir=base_dir, state=state, graph=graph, node_name=node_name,
        )
    except Exception as exc:
        print(f"proc {proc_name} failed: {exc}", file=sys.stderr)
        return 1

    # Phase 3：持锁 record_completion + record_exit + 写回
    with StateLock(sf):
        # 重读最新 state（其他 worker 可能已更新）
        latest = StateManager.read(sf)
        new_state = StateManager.record_completion(
            latest, node_name, output_files=tuple(output_files or ())
        )
        new_state = StateManager.record_exit(new_state, node_name)
        StateManager.write(sf, new_state)
    return 0


def _find_config(sop_name: str) -> Path:
    base = Path(__file__).resolve().parents[2]
    return base / "config" / f"{sop_name}_graph.json"