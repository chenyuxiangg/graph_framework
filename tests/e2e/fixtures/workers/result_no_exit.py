"""result 后不退出 fixture（raw NDJSON）：写 result 后继续 sleep。

协议 §3：`result` 后必须在 `exit_grace`（默认 1s）内退出，否则协议错误
（`NODE_WORKER_PROTOCOL`）并杀进程组。
"""

import json
import sys
import time


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 写 result → 不退出，等待被 supervisor kill。"""
    send(
        "hello",
        {
            "protocol_version": 1,
            "python_version": sys.version.split()[0],
            "sdk_version": "fixture",
            "worker_id": "result-no-exit",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    send("result", {"output": {"ok": True}})
    time.sleep(60)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
