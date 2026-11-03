#!/usr/bin/env python3
"""F5d e2e fixture：本地 OpenAI Responses API mock（SSE），供真实 codex-cli 零密钥使用。

用法：`mock_codex_responses.py <port-file>`；绑定 `127.0.0.1:0` 后把端口写入 port-file。
环境：
- `MOCK_CODEX_MODE`：`text`（默认）| `tool`（首请求返回 command_execution）|
  `slow`（SSE 长连接 + keepalive，供取消用例）；
- `MOCK_CODEX_LOG`：追加请求/客户端断开事件（JSON 行，e2e 断言用）。

响应口径（与 DR-020 spike 一致）：usage input=11（cached=4）、output=7、
reasoning=2、total=18；文本 `hello from mock`。
"""

from __future__ import annotations

import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

PORT_FILE: str = sys.argv[1]
MODE: str = os.environ.get("MOCK_CODEX_MODE", "text")
LOG_PATH: str | None = os.environ.get("MOCK_CODEX_LOG")
REPLY = "hello from mock"
REQUEST_COUNT = 0


def log(entry: dict[str, Any]) -> None:
    """追加审计行（e2e 断言请求已到达 / 客户端已断开）。"""
    if not LOG_PATH:
        return
    with open(LOG_PATH, "a", encoding="utf-8") as handle:
        handle.write(json.dumps(entry) + "\n")


def sse(event: dict[str, Any]) -> bytes:
    """编码一帧 SSE。"""
    return f"event: {event.get('type', 'message')}\ndata: {json.dumps(event)}\n\n".encode()


def usage(input_tokens: int = 11, output_tokens: int = 7, reasoning: int = 2) -> dict[str, Any]:
    """Responses API usage（cached=4；codex 会映射到 cached_input_tokens）。"""
    return {
        "input_tokens": input_tokens,
        "input_tokens_details": {"cached_tokens": 4},
        "output_tokens": output_tokens,
        "output_tokens_details": {"reasoning_tokens": reasoning},
        "total_tokens": input_tokens + output_tokens,
    }


def response_envelope(output: list[dict[str, Any]], status: str = "completed") -> dict[str, Any]:
    """Responses API response 对象。"""
    return {
        "id": "resp_mock_1",
        "object": "response",
        "created_at": 0,
        "status": status,
        "model": "mock-model",
        "output": output,
        "parallel_tool_calls": False,
        "tool_choice": "auto",
        "tools": [],
        "usage": usage() if status == "completed" else None,
    }


def message_item(text: str) -> dict[str, Any]:
    """assistant message item。"""
    return {
        "id": "msg_mock_1",
        "type": "message",
        "role": "assistant",
        "status": "completed",
        "content": [{"type": "output_text", "text": text, "annotations": []}],
    }


def tool_call_item() -> dict[str, Any]:
    """function_call item（触发 codex 执行 shell tool；仅 F5f 非嵌套手动路径使用）。"""
    return {
        "id": "fc_mock_1",
        "type": "function_call",
        "status": "completed",
        "call_id": "call_mock_1",
        "name": "shell",
        "arguments": json.dumps({"command": ["bash", "-lc", "echo hi-from-tool"]}),
    }


def text_body() -> bytes:
    """正常文本回复 SSE 帧。"""
    item = message_item(REPLY)
    frames = [
        {"type": "response.created", "response": response_envelope([], "in_progress")},
        {"type": "response.output_item.done", "output_index": 0, "item": item},
        {"type": "response.completed", "response": response_envelope([item])},
    ]
    return b"".join(sse(frame) for frame in frames)


def tool_body() -> bytes:
    """首请求返回 function_call；后续请求返回最终文本（tool 场景）。"""
    if REQUEST_COUNT == 1:
        call = tool_call_item()
        frames = [
            {"type": "response.created", "response": response_envelope([], "in_progress")},
            {
                "type": "response.output_item.added",
                "output_index": 0,
                "item": {**call, "status": "in_progress", "arguments": ""},
            },
            {"type": "response.output_item.done", "output_index": 0, "item": call},
            {"type": "response.completed", "response": response_envelope([call])},
        ]
        return b"".join(sse(frame) for frame in frames)
    item = message_item(REPLY)
    frames = [
        {"type": "response.created", "response": response_envelope([], "in_progress")},
        {"type": "response.output_item.done", "output_index": 0, "item": item},
        {"type": "response.completed", "response": response_envelope([item])},
    ]
    return b"".join(sse(frame) for frame in frames)


class Handler(BaseHTTPRequestHandler):
    """`/v1/responses`（SSE）；`/health` 返回 ok。"""

    protocol_version = "HTTP/1.1"

    def log_message(self, _format: str, *args: Any) -> None:
        """静默访问日志。"""

    def _start_sse(self) -> None:
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "keep-alive")
        self.send_header("Transfer-Encoding", "chunked")
        self.end_headers()

    def _chunk(self, payload: bytes) -> None:
        self.wfile.write(f"{len(payload):x}\r\n".encode() + payload + b"\r\n")
        self.wfile.flush()

    def do_POST(self) -> None:  # noqa: N802（http.server 固定命名）
        """处理 Responses 请求。"""
        global REQUEST_COUNT
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        REQUEST_COUNT += 1
        log({"event": "request", "path": self.path, "count": REQUEST_COUNT,
             "body_bytes": len(body)})
        if MODE == "slow":
            self._serve_slow()
            return
        payload = tool_body() if MODE == "tool" else text_body()
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def _serve_slow(self) -> None:
        """SSE 长连接 + keepalive；客户端（codex）被杀时记录 client_disconnected。"""
        self._start_sse()
        try:
            for _ in range(300):
                self._chunk(b": keepalive\n\n")
                time.sleep(0.1)
        except (BrokenPipeError, ConnectionResetError, OSError):
            log({"event": "client_disconnected"})

    def do_GET(self) -> None:  # noqa: N802
        """健康检查。"""
        payload = b"ok"
        self.send_response(200)
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)


def main() -> int:
    """启动 mock server 并写端口文件。"""
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    with open(PORT_FILE, "w", encoding="utf-8") as handle:
        handle.write(str(server.server_address[1]))
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
