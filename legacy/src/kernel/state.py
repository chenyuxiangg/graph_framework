"""状态原子读写 + 多实例路径生成。

锁粒度 = 单实例（不是单 SOP）；state schema 由组件层决定，内核只管原子性。
"""

from __future__ import annotations

import errno
import fcntl
import json
import os
import tempfile
import time
import uuid
from pathlib import Path

DEFAULT_ROOT = Path("/var/lib/graph_base")
ENV_ROOT = "GRAPH_BASE_STATE_ROOT"
DEFAULT_STALE_AFTER = 300.0
DEFAULT_TIMEOUT = 5.0


def state_dir(root: Path | None = None, sop_name: str = "") -> Path:
    """计算 <root>/<sop_name>/ 路径，并自动 mkdir(parents=True, exist_ok=True)。

    root 缺省时读 GRAPH_BASE_STATE_ROOT 环境变量，默认 /var/lib/graph_base。
    """
    base = _effective_root(root)
    p = base / sop_name
    p.mkdir(parents=True, exist_ok=True)
    return p


def state_path(
    root: Path | None = None, sop_name: str = "", instance_id: str = ""
) -> Path:
    """<state_dir>/<instance_id>.json；不创建文件。"""
    return state_dir(root, sop_name) / f"{instance_id}.json"


def new_instance_id() -> str:
    """uuid.uuid4().hex[:12]，12 字符 hex。"""
    return uuid.uuid4().hex[:12]


def _effective_root(root: Path | None) -> Path:
    if root is not None:
        return Path(root)
    env = os.environ.get(ENV_ROOT)
    if env:
        return Path(env)
    return DEFAULT_ROOT


class StateLock:
    """基于 fcntl.flock 的互斥锁；with 语法使用。

    超时（timeout 秒）内拿不到锁抛 TimeoutError。
    stale_after 用于 claim 有效性判断（仅 claim 函数使用，锁本身不感知）。
    """

    def __init__(
        self,
        state_file: Path,
        timeout: float = DEFAULT_TIMEOUT,
        stale_after: float = DEFAULT_STALE_AFTER,
    ) -> None:
        self.state_file = Path(state_file)
        self.timeout = timeout
        self.stale_after = stale_after
        self._fd: int | None = None
        self._lock_path: Path | None = None

    def __enter__(self) -> "StateLock":
        # 锁放在 state_file 同目录，名为 <state_file.stem>.lock
        self._lock_path = self.state_file.with_suffix(self.state_file.suffix + ".lock")
        self._lock_path.parent.mkdir(parents=True, exist_ok=True)
        fd = os.open(str(self._lock_path), os.O_CREAT | os.O_RDWR, 0o644)
        self._fd = fd
        deadline = time.monotonic() + self.timeout
        while True:
            try:
                fcntl.flock(fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
                return self
            except OSError as exc:
                if exc.errno not in (errno.EWOULDBLOCK, errno.EAGAIN):
                    raise
                if time.monotonic() >= deadline:
                    os.close(fd)
                    self._fd = None
                    raise TimeoutError(
                        f"{self.timeout}s 内未拿到锁 {self._lock_path}"
                    ) from exc
                time.sleep(0.05)

    def __exit__(self, exc_type: type[BaseException] | None, exc: BaseException | None, tb: object) -> None:
        if self._fd is not None:
            try:
                fcntl.flock(self._fd, fcntl.LOCK_UN)
            finally:
                os.close(self._fd)
                self._fd = None


def read_state(state_file: Path) -> dict:
    """读 state JSON；文件不存在返回 {}。"""
    p = Path(state_file)
    if not p.exists():
        return {}
    with p.open("r", encoding="utf-8") as f:
        try:
            data = json.load(f)
        except json.JSONDecodeError:
            return {}
    if not isinstance(data, dict):
        return {}
    return data


def write_state(state_file: Path, payload: dict) -> None:
    """原子写入：临时文件 + os.replace。调用方应自行持锁。"""
    p = Path(state_file)
    p.parent.mkdir(parents=True, exist_ok=True)
    fd, tmp_path = tempfile.mkstemp(
        prefix=f".{p.name}.", suffix=".tmp", dir=str(p.parent)
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as f:
            json.dump(payload, f, ensure_ascii=False, indent=2)
        os.replace(tmp_path, p)
    except Exception:
        # 失败时清理临时文件，避免遗留
        try:
            os.unlink(tmp_path)
        except OSError:
            pass
        raise


def claim(state_file: Path, pid: int, owner: str, stale_after: float = DEFAULT_STALE_AFTER) -> bool:
    """认领 state_file：写入 {claim_pid, claim_owner, claim_ts}。

    返回是否成功。失败原因：
    - 文件已存在有效 claim（pid 存活且未超时）
    - 写盘异常

    成功时仍由调用方负责持锁；本函数只覆盖 payload 字段，不改 lock 状态。
    stale_after 用于判断旧 claim 是否过期；默认 300s。
    """
    p = Path(state_file)
    with StateLock(p, stale_after=stale_after) as lock:
        _ = lock  # lock 持有仅用于读-改-写原子性
        current = read_state(p)
        existing_pid = current.get("claim_pid")
        existing_ts = current.get("claim_ts")
        if existing_pid is not None and existing_ts is not None:
            if pid_alive(int(existing_pid)):
                age = time.time() - float(existing_ts)
                if age <= stale_after:
                    return False
        payload = dict(current)
        payload["claim_pid"] = pid
        payload["claim_owner"] = owner
        payload["claim_ts"] = time.time()
        write_state(p, payload)
        return True


def clear_claim(state_file: Path) -> None:
    """清除 claim 字段，保留其他字段。"""
    p = Path(state_file)
    with StateLock(p) as lock:
        _ = lock
        current = read_state(p)
        current.pop("claim_pid", None)
        current.pop("claim_owner", None)
        current.pop("claim_ts", None)
        write_state(p, current)


def pid_alive(pid: int) -> bool:
    """检查 pid 是否存活。pid <= 0 返回 False。"""
    if pid <= 0:
        return False
    try:
        os.kill(pid, 0)
        return True
    except OSError as exc:
        if exc.errno == errno.ESRCH:
            return False
        if exc.errno == errno.EPERM:
            # 进程存在但没权限发信号
            return True
        return False