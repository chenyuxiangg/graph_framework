"""重复 result fixture（raw NDJSON）：一帧 result 后又写一帧 result。

协议 §3：`result` 必须**恰好一帧**；重复帧 = 协议错误（`NODE_WORKER_PROTOCOL`），
supervisor 在退出后仍会读残余帧并判错。
"""

import json
import sys


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 连写两帧 result → exit 0。"""
    send(
        "hello",
        {
            "protocol_version": 1,
            "python_version": sys.version.split()[0],
            "sdk_version": "fixture",
            "worker_id": "duplicate-result",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    send("result", {"output": {"done": 1}})
    send("result", {"output": {"done": 2}})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
