"""supervisor 信号 fixture：握手 + 等 run 后 `os.kill(getpid(), SIGKILL)`。

等价 shell 的 `kill -9 $$`：进程被 SIGKILL 终止、没有退出码，进入协议 §3 的
崩溃分支（`WorkerFailure::Exit { code: None, signal: Some(9) }`）。
"""

import json
import os
import signal
import sys


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 自杀（SIGKILL）。"""
    pid_file = os.environ.get("ZGRAPH_FIXTURE_PID_FILE")
    if pid_file:
        with open(pid_file, "w", encoding="utf-8") as handle:
            handle.write(str(os.getpid()))
    send(
        "hello",
        {
            "protocol_version": 1,
            "python_version": sys.version.split()[0],
            "sdk_version": "fixture",
            "worker_id": "signal-exit",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 0
        if json.loads(line).get("kind") == "run":
            break
    os.kill(os.getpid(), signal.SIGKILL)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
