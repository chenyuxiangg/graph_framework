"""supervisor 取消升级 fixture：忽略 cancel 帧与 SIGTERM，只能被 SIGKILL 终止。

启动后写 pid 文件（供断言 reap），完成 hello/hello_ack/run 三次往返后进入长 sleep；
stdin 不再读取，因此 supervisor 的 cancel 帧不会被协作消费 → 必须升级到 SIGKILL。
"""

import json
import os
import signal
import sys
import time


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 长睡（忽略 SIGTERM）。"""
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
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
            "worker_id": "cancel-ignore",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    time.sleep(60)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
