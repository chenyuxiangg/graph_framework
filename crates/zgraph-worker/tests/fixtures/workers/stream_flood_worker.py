"""流式上限 fixture：连续写 ``ZGRAPH_FIXTURE_COUNT`` 个 emit 帧后写 result。

每个 emit 帧的 ``text`` 长度由 ``ZGRAPH_FIXTURE_TEXT_BYTES`` 控制，用于验证
supervisor 的 emit 条数/字节上限（超限丢弃计数，不 OOM）。
"""

import json
import os
import sys


def send(kind: str, payload: dict) -> None:
    """写一帧 NDJSON 到协议 stdout。"""
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    """握手 → 等 run → 洪泛 emit → result。"""
    count = int(os.environ.get("ZGRAPH_FIXTURE_COUNT", "50"))
    text_bytes = int(os.environ.get("ZGRAPH_FIXTURE_TEXT_BYTES", "200"))
    send(
        "hello",
        {
            "protocol_version": 1,
            "python_version": sys.version.split()[0],
            "sdk_version": "fixture",
            "worker_id": "stream-flood",
        },
    )
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    for index in range(count):
        send("emit", {"type": "token", "text": f"{index:04d}" + "x" * text_bytes})
    send("result", {"output": {"emitted": count}})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
