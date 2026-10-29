"""supervisor 退出约定 fixture：写 error 帧后以 exit 0 退出（违反协议 §3）。

协议 §3 封闭表只允许「exit 2 + 结构化 error」表示业务失败；error + exit 0 是协议错误。
"""

import json
import os
import sys


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 写 error 帧 → exit 0。"""
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
            "worker_id": "error-exit-zero",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    send("error", {"code": "NODE_BUSINESS_ERROR", "message": "boom but exit 0"})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
