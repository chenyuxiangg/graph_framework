"""supervisor 退出码 fixture：握手 + 等 run 后以约定退出码退出（无 result）。

默认退出码 7（非约定码 → `NODE_WORKER_CRASH`）；设置
`ZGRAPH_FIXTURE_EXIT_CODE=3` 可复现 SDK 自检失败（协议 §3 的协议错误分支）。
"""

import json
import os
import sys

DEFAULT_EXIT_CODE = 7


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 直接退出（不写 result/error）。"""
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
            "worker_id": "exit-code",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 0
        if json.loads(line).get("kind") == "run":
            break
    return int(os.environ.get("ZGRAPH_FIXTURE_EXIT_CODE", str(DEFAULT_EXIT_CODE)))


if __name__ == "__main__":
    raise SystemExit(main())
