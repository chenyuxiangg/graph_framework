"""ProcessDispatcher：按 worker_cmd 模板拉子进程。"""

from __future__ import annotations

import os
import shlex
import subprocess
import sys
import time
import uuid
from pathlib import Path
from typing import Any

from src.kernel import Graph, Node

from ..state_manager import State
from .base import DispatchResult


DEFAULT_WORKER_CMD: str = (
    "{python} -m src.worker.{sop_name}.{node_name}_worker "
    "--sop {sop_name} --instance {instance_id} "
    "--node {node_name} --proc {proc} --base-dir {base_dir}"
)


class ProcessDispatcher:
    """setsid 拉子进程，按 worker_cmd 模板替换占位符。"""

    def __init__(
        self,
        worker_cmd: str | None = None,
        timeout: float = 5.0,
        env_extra: dict[str, str] | None = None,
    ) -> None:
        self._worker_cmd = worker_cmd or DEFAULT_WORKER_CMD
        self._timeout = timeout
        self._env_extra = env_extra or {}

    def dispatch(
        self,
        graph: Graph,
        state: State,
        node: Node,
        instance_id: str,
        work_dir: Path,
    ) -> DispatchResult:
        """拉子进程跑 node.proc。返回 DispatchResult。"""
        cmd_str = self._worker_cmd.format_map(_SafeFormat(
            python=sys.executable,
            sop_name=graph.name,
            instance_id=instance_id,
            node_name=node.name,
            proc=node.proc or "",
            base_dir=str(work_dir),
        ))
        argv = shlex.split(cmd_str)
        env = os.environ.copy()
        env.update(self._env_extra)
        env["GRAPH_BASE_STATE_ROOT"] = str(work_dir.parent.parent)
        env["GRAPH_BASE_INSTANCE_ID"] = instance_id
        env["GRAPH_BASE_SOP_NAME"] = graph.name
        # 把 cwd 上的 src 与 scripts/ 注入 PYTHONPATH；保证子进程可 import src / zlog
        if "PYTHONPATH" not in env:
            env["PYTHONPATH"] = os.getcwd()

        pid = os.fork()
        if pid == 0:
            os.setsid()
            try:
                os.execvpe(argv[0], argv, env)
            except Exception:
                os._exit(1)
        # 父进程：等 pid_alive（最长 timeout 秒）
        deadline = time.monotonic() + self._timeout
        while time.monotonic() < deadline:
            try:
                os.kill(pid, 0)
                return DispatchResult(pid=pid, dispatch_id=uuid.uuid4().hex[:12], backend="process")
            except OSError:
                time.sleep(0.05)
        return DispatchResult(pid=pid, dispatch_id=uuid.uuid4().hex[:12], backend="process")

    def alive(self, pid: int) -> bool:
        if pid <= 0:
            return False
        try:
            os.kill(pid, 0)
            return True
        except OSError:
            return False

    def wait(self, pid: int, timeout: float) -> int:
        """waitpid with timeout (秒)。返回 exit code；超时返回 -1。"""
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                _pid, status = os.waitpid(pid, os.WNOHANG)
                if _pid == pid:
                    return os.waitstatus_to_exitcode(status)
            except ChildProcessError:
                return -1
            time.sleep(0.05)
        return -1


class _SafeFormat(dict):
    """str.format_map 用：缺 key 时原样保留。"""

    def __missing__(self, key: str) -> str:
        return "{" + key + "}"