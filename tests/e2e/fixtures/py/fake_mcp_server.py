#!/usr/bin/env python3
"""V5a e2e fixture：fake MCP stdio server（双 era + 行为注入）。

用法：`fake_mcp_server.py <modern|legacy>`；
- modern：`server/discover` 成功（`2026-07-28` 无状态）；`tools/list`、`tools/call`。
- legacy：`server/discover` 返回 -32601（触发 rmcp Auto 回落 `2025-11-25` initialize）。

环境变量：`ZGRAPH_FAKE_MCP_LOG`（追加请求/响应 JSON 行）、
`ZGRAPH_FAKE_MCP_SPAWN_LOG`（追加 pid，崩溃重启计数用）。

工具：echo / slow / crash / big / needs_input / annotated。
"""

from __future__ import annotations

import json
import os
import sys
import time
from typing import Any

MODE: str = sys.argv[1] if len(sys.argv) > 1 else "modern"
LOG_PATH: str | None = os.environ.get("ZGRAPH_FAKE_MCP_LOG")
SPAWN_LOG: str | None = os.environ.get("ZGRAPH_FAKE_MCP_SPAWN_LOG")
PROBE_FILE: str | None = os.environ.get("ZGRAPH_FAKE_MCP_PROBE_FILE")

TOOLS: list[dict[str, Any]] = [
    {"name": "echo", "description": "Echo back the text argument",
     "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}},
                     "required": ["text"]}},
    {"name": "slow", "description": "Sleep for seconds then return",
     "inputSchema": {"type": "object", "properties": {"seconds": {"type": "number"}}}},
    {"name": "crash", "description": "Crash the server process",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "big", "description": "Return a >32KiB text result",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "needs_input", "description": "Return input_required (MRTR)",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "annotated", "description": "Tool with annotations",
     "inputSchema": {"type": "object", "properties": {}},
     "annotations": {"readOnlyHint": True, "destructiveHint": False,
                     "idempotentHint": True}},
    {"name": "a b", "description": "Name normalizes to a_b",
     "inputSchema": {"type": "object", "properties": {}}},
    {"name": "a_b", "description": "Name collision with 'a b'",
     "inputSchema": {"type": "object", "properties": {}}},
]

SERVER_INFO: dict[str, Any] = {"name": "fake-mcp", "version": "0.3.0"}
MODERN_VERSIONS: list[str] = ["2026-07-28"]
LEGACY_VERSION: str = "2025-11-25"


def log(direction: str, payload: Any) -> None:
    """把请求/响应追加到日志（e2e 断言 ask/list/call 事实）。"""
    if not LOG_PATH:
        return
    with open(LOG_PATH, "a", encoding="utf-8") as handle:
        handle.write(json.dumps({"dir": direction, "payload": payload}) + "\n")


def send(message: dict[str, Any]) -> None:
    """写一行 JSON-RPC 消息。"""
    log("out", message)
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def receive() -> dict[str, Any] | None:
    """读一行 JSON-RPC 消息；EOF → None。"""
    line = sys.stdin.readline()
    if not line:
        return None
    message = json.loads(line)
    log("in", message)
    return message


def result(message: dict[str, Any], payload: dict[str, Any]) -> None:
    """回复成功响应。"""
    send({"jsonrpc": "2.0", "id": message.get("id"), "result": payload})


def error(message: dict[str, Any], code: int, text: str) -> None:
    """回复错误响应。"""
    send({"jsonrpc": "2.0", "id": message.get("id"),
          "error": {"code": code, "message": text}})


def discover_result() -> dict[str, Any]:
    """`2026-07-28` server/discover 结果（camelCase per rmcp）。"""
    return {
        "resultType": "complete",
        "supportedVersions": MODERN_VERSIONS,
        "capabilities": {"tools": {}},
        "ttlMs": 0,
        "cacheScope": "private",
        "_meta": {"io.modelcontextprotocol/serverInfo": SERVER_INFO},
    }


def initialize_result() -> dict[str, Any]:
    """`2025-11-25` legacy initialize 结果。"""
    return {"protocolVersion": LEGACY_VERSION,
            "capabilities": {"tools": {}},
            "serverInfo": SERVER_INFO}


def call_tool(name: str, arguments: dict[str, Any]) -> dict[str, Any]:
    """按工具名返回结果；`crash` 直接退出进程（崩溃注入）。"""
    if name == "echo":
        text = str(arguments.get("text", ""))
        return {"content": [{"type": "text", "text": f"echo:{text}"}],
                "isError": False}
    if name == "slow":
        time.sleep(float(arguments.get("seconds", 5)))
        return {"content": [{"type": "text", "text": "slow-done"}]}
    if name == "crash":
        os._exit(7)
    if name == "big":
        return {"content": [{"type": "text", "text": "B" * 40_000}]}
    if name == "needs_input":
        # state-only `input_required`：客户端带 requestState 重试（rmcp MRTR 轮次，
        # 空 clientCapabilities 无法满足输入 → 最终 input_required_unsupported）。
        return {"resultType": "input_required", "requestState": "fake-state-1"}
    if name == "annotated":
        return {"content": [{"type": "text", "text": "annotated-ok"}],
                "annotations": {"readOnlyHint": True}}
    return {"content": [{"type": "text", "text": f"unknown:{name}"}],
            "isError": True}


def handle(message: dict[str, Any]) -> bool:
    """处理一条消息；返回 False 表示需要退出。"""
    method = message.get("method")
    params = message.get("params") or {}
    if method == "server/discover":
        if MODE == "legacy":
            error(message, -32601, "Method not found (legacy server)")
        else:
            result(message, discover_result())
        return True
    if method == "initialize":
        result(message, initialize_result())
        return True
    if method == "notifications/initialized":
        return True
    if method == "tools/list":
        result(message, {"tools": TOOLS})
        return True
    if method == "tools/call":
        payload = call_tool(str(params.get("name", "")), params.get("arguments") or {})
        result(message, payload)
        return True
    if message.get("id") is not None:
        error(message, -32601, f"Method not found: {method}")
    return True


def main() -> int:
    """stdio 主循环。"""
    if SPAWN_LOG:
        with open(SPAWN_LOG, "a", encoding="utf-8") as spawn_handle:
            spawn_handle.write(f"{os.getpid()}\n")
    if PROBE_FILE:
        # 证明 registry `env` 的 `${ENV}` 展开已传入子进程（值只写入测试侧文件）。
        with open(PROBE_FILE, "w", encoding="utf-8") as probe_handle:
            probe_handle.write(os.environ.get("ZGRAPH_FAKE_PROBE", ""))
    while True:
        message = receive()
        if message is None:
            return 0
        handle(message)


if __name__ == "__main__":
    raise SystemExit(main())
