"""无 result fixture（raw NDJSON）：hello → 收到 run → exit 0，但不写 result。

协议 §3 封闭表：exit 0 但没有 `result` → 协议错误（`NODE_WORKER_PROTOCOL`）。
"""

import json
import sys


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 直接 exit 0（无 result）。"""
    send(
        "hello",
        {
            "protocol_version": 1,
            "python_version": sys.version.split()[0],
            "sdk_version": "fixture",
            "worker_id": "no-result",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
