"""supervisor 有界读取 fixture：握手后写一条 >8 MiB 且无换行的「帧」。

旧实现 `read_until` 会先分配整行 → OOM；M3 修复后 supervisor 必须立即判
NODE_WORKER_PROTOCOL 并杀进程组（不等待换行）。
"""

import json
import os
import sys
import time

FRAME_LIMIT: int = 8 * 1024 * 1024
OVERSIZED: int = FRAME_LIMIT + 1024


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 写超大无换行字节流 → 长睡（应被 supervisor kill）。"""
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
            "worker_id": "oversized-frame",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    sys.stdout.buffer.write(b"x" * OVERSIZED)
    sys.stdout.buffer.flush()
    time.sleep(60)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
