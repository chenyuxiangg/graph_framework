#!/usr/bin/env python3
"""V5a e2e fixture：fake MCP Streamable HTTP server（modern era，单 endpoint）。

用法：`fake_mcp_http.py <port-file>`；绑定 `127.0.0.1:0` 后把实际端口写入 port-file。
环境：`ZGRAPH_FAKE_MCP_HTTP_LOG`（追加请求 JSON 行，含 method/headers）。
支持 `server/discover` / `tools/list` / `tools/call`（echo/slow/annotated）。
"""

from __future__ import annotations

import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from typing import Any

PORT_FILE: str = sys.argv[1]
LOG_PATH: str | None = os.environ.get("ZGRAPH_FAKE_MCP_HTTP_LOG")
SERVER_INFO: dict[str, Any] = {"name": "fake-mcp-http", "version": "0.3.0"}
TOOLS: list[dict[str, Any]] = [
    {"name": "echo", "description": "Echo back the text argument",
     "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}},
                     "required": ["text"]}},
    {"name": "slow", "description": "Sleep then return",
     "inputSchema": {"type": "object", "properties": {"seconds": {"type": "number"}}}},
]


def log(payload: Any) -> None:
    """追加请求审计行。"""
    if not LOG_PATH:
        return
    with open(LOG_PATH, "a", encoding="utf-8") as handle:
        handle.write(json.dumps(payload) + "\n")


def discover_result() -> dict[str, Any]:
    """modern `server/discover` 结果。"""
    return {"resultType": "complete", "supportedVersions": ["2026-07-28"],
            "capabilities": {"tools": {}}, "ttlMs": 0, "cacheScope": "private",
            "_meta": {"io.modelcontextprotocol/serverInfo": SERVER_INFO}}


def call_tool(name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    """工具调用（fake 行为）。"""
    if name == "echo":
        return {"content": [{"type": "text",
                             "text": f"http-echo:{arguments.get('text', '')}"}]}
    if name == "slow":
        time.sleep(float(arguments.get("seconds", 3)))
        return {"content": [{"type": "text", "text": "http-slow-done"}]}
    return {"content": [{"type": "text", "text": f"unknown:{name}"}], "isError": True}


class Handler(BaseHTTPRequestHandler):
    """JSON-RPC over Streamable HTTP（application/json 响应）。"""

    protocol_version = "HTTP/1.1"

    def log_message(self, _format: str, *args: Any) -> None:
        """静默访问日志。"""

    def _send_json(self, message: dict[str, Any]) -> None:
        data = json.dumps(message).encode("utf-8")
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_POST(self) -> None:  # noqa: N802（http.server 固定命名）
        """处理一条 JSON-RPC 请求。"""
        length = int(self.headers.get("Content-Length", "0"))
        body = self.rfile.read(length)
        message = json.loads(body)
        log({"method": message.get("method"),
             "headers": {key: value for key, value in self.headers.items()},
             "body": message})
        request_id = message.get("id")
        method = message.get("method")
        params = message.get("params") or {}
        if method == "server/discover":
            payload = discover_result()
        elif method == "tools/list":
            payload = {"tools": TOOLS}
        elif method == "tools/call":
            payload = call_tool(str(params.get("name", "")),
                                params.get("arguments") or {})
        else:
            self._send_json({"jsonrpc": "2.0", "id": request_id,
                             "error": {"code": -32601, "message": "Method not found"}})
            return
        self._send_json({"jsonrpc": "2.0", "id": request_id, "result": payload})

    def do_GET(self) -> None:  # noqa: N802
        """modern era 无 SSE 通知流：405（客户端应容忍）。"""
        self.send_response(405)
        self.send_header("Content-Length", "0")
        self.end_headers()


def main() -> int:
    """启动 server 并写 port 文件。"""
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    with open(PORT_FILE, "w", encoding="utf-8") as handle:
        handle.write(str(server.server_address[1]))
    server.serve_forever()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
