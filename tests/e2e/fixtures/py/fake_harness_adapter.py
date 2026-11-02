#!/usr/bin/env python3
"""V5a e2e fixture：fake external harness adapter（`zgraph-harness/1` NDJSON over stdio）。

用法：`fake_harness_adapter.py <mode>`；mode：echo / no_stream / version_bad /
bad_json / big_line / bad_result / error / exit_nonzero / silent / stderr_big。

环境：`ZGRAPH_FAKE_ADAPTER_PID_FILE`（写 pid，取消/孤儿断言）、
`ZGRAPH_FAKE_ADAPTER_LOG`（追加收到/发出的 JSON 帧，L2 不 spawn/协议断言）。
"""

from __future__ import annotations

import json
import os
import sys
import time
from typing import Any

MODE: str = sys.argv[1] if len(sys.argv) > 1 else "echo"
PID_FILE: str | None = os.environ.get("ZGRAPH_FAKE_ADAPTER_PID_FILE")
LOG_PATH: str | None = os.environ.get("ZGRAPH_FAKE_ADAPTER_LOG")
PROTOCOL: str = "zgraph-harness/1"
STREAMING: bool = MODE != "no_stream"


def log(direction: str, payload: Any) -> None:
    """记录一帧（L2 断言不 spawn：日志行数不变）。"""
    if not LOG_PATH:
        return
    with open(LOG_PATH, "a", encoding="utf-8") as handle:
        handle.write(json.dumps({"dir": direction, "payload": payload}) + "\n")


def send(payload: dict[str, Any]) -> None:
    """写一帧并 flush。"""
    log("out", payload)
    sys.stdout.write(json.dumps(payload) + "\n")
    sys.stdout.flush()


def read() -> dict[str, Any] | None:
    """读一帧；EOF → None。"""
    line = sys.stdin.readline()
    if not line:
        return None
    frame = json.loads(line)
    log("in", frame)
    return frame


def send_result(content: str = "final-text") -> None:
    """发送成功 result（四字段 + usage 映射断言值）。"""
    send({"v": 1, "type": "result", "content": content,
          "stop_reason": "EndTurn", "iterations": 1,
          "usage": {"input_tokens": 11, "output_tokens": 7,
                    "reasoning_output_tokens": 3}})


def main() -> int:
    """协议主循环。"""
    if PID_FILE:
        with open(PID_FILE, "w", encoding="utf-8") as pid_handle:
            pid_handle.write(str(os.getpid()))
    hello = read()
    if hello is None:
        return 0
    if MODE == "version_bad":
        send({"v": 1, "type": "hello_ok", "protocol": "zgraph-harness/2",
              "provider": "fake", "capabilities": {"streaming": False}})
        return 0
    if MODE == "bad_json":
        sys.stdout.write("not-json\n")
        sys.stdout.flush()
        return 0
    send({"v": 1, "type": "hello_ok", "protocol": PROTOCOL,
          "provider": "fake", "capabilities": {"streaming": STREAMING}})
    while True:
        frame = read()
        if frame is None:
            return 0
        if frame.get("type") == "cancel":
            return 0
        if frame.get("type") != "run":
            continue
        if MODE == "echo":
            send({"v": 1, "type": "event", "kind": "delta", "text": "hel"})
            send({"v": 1, "type": "event", "kind": "delta", "text": "lo"})
            send({"v": 1, "type": "event", "kind": "reasoning", "text": "think"})
            send({"v": 1, "type": "event", "kind": "tool_start", "call_id": "c1",
                  "tool": "echo", "args_summary": {"a": 1}})
            send({"v": 1, "type": "event", "kind": "tool_end", "call_id": "c1",
                  "ok": True, "result_summary": {"b": 2}})
            send({"v": 1, "type": "event", "kind": "progress",
                  "percent": 40, "message": "half"})
            send_result()
        elif MODE == "no_stream":
            send({"v": 1, "type": "event", "kind": "delta", "text": "ignored"})
            send_result()
        elif MODE == "bad_result":
            send({"v": 1, "type": "result", "content": "x"})
            return 1
        elif MODE == "big_line":
            sys.stdout.write("x" * (2 * 1024 * 1024) + "\n")
            sys.stdout.flush()
            time.sleep(0.2)
            return 0
        elif MODE == "error":
            send({"v": 1, "type": "error", "code": "HARNESS_PLUGIN_ERROR",
                  "message": "adapter boom", "reason": "protocol"})
            return 0
        elif MODE == "exit_nonzero":
            return 3
        elif MODE == "silent":
            time.sleep(60)
            return 0
        elif MODE == "stderr_big":
            sys.stderr.write("HEAD-" + ("z" * 6000) + "-TAILEND")
            sys.stderr.flush()
            return 5
        else:
            send_result()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
