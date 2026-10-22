"""Monitor：emit / reap_stale / detect_progress。"""

from __future__ import annotations

import os
import time
from pathlib import Path
from typing import Any

from src.kernel import Graph, StateLock, claim as kernel_claim, pid_alive, state_path

from .events import Event, _Event


class Monitor:
    """事件发出器 + stale 回收 + 进度兜底。"""

    def __init__(self, logger: Any) -> None:
        self._logger = logger

    def emit(self, event: _Event, **fields: Any) -> None:
        """单事件：按 event.level 调对应 log 方法。"""
        method = _level_method(self._logger, event.level)
        method(event.name, **fields)

    def reap_stale_claims(self, root: Path, sop_name: str, stale_after: float = 300.0) -> list[str]:
        """扫 sop 目录下所有 state.json 的 claim；pid_alive=false 或 age>stale_after → 告警。"""
        from src.kernel import read_state

        alarms: list[str] = []
        sop_dir = root / sop_name
        if not sop_dir.is_dir():
            return alarms
        for state_file in sop_dir.glob("*.json"):
            data = read_state(state_file)
            pid = data.get("claim_pid")
            ts = data.get("claim_ts")
            if pid is None or ts is None:
                continue
            age = time.time() - float(ts)
            if (not pid_alive(int(pid))) or age > stale_after:
                self.emit(
                    Event.SCH_EV_CLAIM_RECLAIM,
                    instance=state_file.stem, age=round(age, 1),
                )
                alarms.append(state_file.stem)
        return alarms

    def detect_progress(self, graph: Graph, state_file: Path, base_dir: Path) -> list[str]:
        """进度兜底：manual 边长时间未 trigger / 异常产物 / etc。"""
        from src.kernel import read_state

        alarms: list[str] = []
        data = read_state(state_file)
        history_len = len(data.get("history", []))
        triggered = data.get("triggered_edges", {})
        # 简单检查：state 文件存在很久但 history 没增长且有 manual pending edge
        ts = data.get("claim_ts")
        if ts is not None and history_len == 0 and triggered:
            age = time.time() - float(ts)
            if age > 60.0:
                self.emit(
                    Event.SCH_EV_MANUAL_PENDING,
                    instance=state_file.stem, age=round(age, 1),
                )
                alarms.append("MANUAL_PENDING")
        return alarms


def _level_method(logger: Any, level: str):
    """按 level 字符串取 logger 的对应方法。"""
    name_to_method = {
        "debug": logger.debug,
        "info": logger.info,
        "warning": logger.warn,
        "error": logger.error,
    }
    return name_to_method.get(level, logger.info)