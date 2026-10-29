"""进程组 kill fixture：worker 自己再派一个孙进程，两者都长睡且不写 result。

用于验证超时/Drop 路径对**进程组**发 SIGKILL（D1：孙进程不残留）。
pid 文件：``ZGRAPH_FIXTURE_PID_FILE``（自身）、``ZGRAPH_FIXTURE_CHILD_PID_FILE``（孙）。
"""

import json
import os
import subprocess
import sys
import time


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 写自身 pid → 派孙进程 → 长睡。"""
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
            "worker_id": "grandchild",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    child = subprocess.Popen(  # noqa: S603 - fixture 只在自己的进程组里派 sleep
        [sys.executable, "-c", "import time\ntime.sleep(60)"]
    )
    child_file = os.environ.get("ZGRAPH_FIXTURE_CHILD_PID_FILE")
    if child_file:
        with open(child_file, "w", encoding="utf-8") as handle:
            handle.write(str(child.pid))
    time.sleep(60)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
