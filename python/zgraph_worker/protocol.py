"""worker 协议 v1 的 Python 实现（帧编解码 + 常量）。

与 Rust 侧 ``crates/zgraph-worker/src/protocol.rs`` 保持同源；任何不兼容修改必须升版本。
"""

from __future__ import annotations

import json
from typing import Any, BinaryIO

PROTOCOL_VERSION: int = 1
SDK_VERSION: str = "0.1.0"
MAX_FRAME_BYTES: int = 8 * 1024 * 1024

ALLOWED_KINDS: frozenset[str] = frozenset(
    {"hello", "hello_ack", "run", "emit", "log", "result", "error", "cancel", "ping", "pong"}
)
ALLOWED_EMIT_TYPES: frozenset[str] = frozenset(
    {"token", "reasoning", "tool_call_start", "tool_call_end", "progress"}
)


class ProtocolError(Exception):
    """协议错误：帧非法、超限、版本不符。"""


def encode_frame(kind: str, payload: dict[str, Any], frame_id: str | None = None) -> bytes:
    """编码为单行 NDJSON（含结尾换行）。"""
    if kind not in ALLOWED_KINDS:
        raise ProtocolError(f"unknown kind: {kind}")
    frame: dict[str, Any] = {"v": PROTOCOL_VERSION, "kind": kind, "payload": payload}
    if frame_id is not None:
        frame["id"] = frame_id
    line: str = json.dumps(frame, ensure_ascii=False, separators=(",", ":"))
    data: bytes = line.encode("utf-8")
    if len(data) > MAX_FRAME_BYTES:
        raise ProtocolError(f"frame too large: {len(data)} bytes")
    return data + b"\n"


def decode_frame(line: bytes) -> dict[str, Any]:
    """解析单行 NDJSON 帧并校验版本/类型。"""
    if len(line) > MAX_FRAME_BYTES:
        raise ProtocolError(f"frame too large: {len(line)} bytes")
    try:
        frame: dict[str, Any] = json.loads(line)
    except json.JSONDecodeError as exc:
        raise ProtocolError(f"invalid json frame: {exc}") from exc
    if frame.get("v") != PROTOCOL_VERSION:
        raise ProtocolError(f"unsupported protocol version: {frame.get('v')}")
    kind: Any = frame.get("kind")
    if kind not in ALLOWED_KINDS:
        raise ProtocolError(f"unknown kind: {kind}")
    if not isinstance(frame.get("payload"), dict):
        raise ProtocolError("payload must be an object")
    return frame


def write_frame(fd: int, kind: str, payload: dict[str, Any]) -> None:
    """通过文件描述符写一帧（原子写一行）。"""
    data: bytes = encode_frame(kind, payload)
    _write_all(fd, data)


def read_frame(stream: BinaryIO) -> dict[str, Any]:
    """从二进制流读一帧；EOF 抛 ``EOFError``。"""
    line: bytes = stream.readline()
    if not line:
        raise EOFError("worker stdin closed")
    if not line.endswith(b"\n"):
        raise ProtocolError("partial frame at EOF (missing trailing newline)")
    return decode_frame(line[:-1])


def _write_all(fd: int, data: bytes) -> None:
    """循环写，直到写完（避免短写）。"""
    import os

    view: memoryview = memoryview(data)
    while view:
        written: int = os.write(fd, view)
        view = view[written:]
