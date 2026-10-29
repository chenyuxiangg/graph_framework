"""协议编解码单测（对应 ``schemas/worker-protocol-v1.md``）。"""

from __future__ import annotations

import io
import json

import pytest

from zgraph_worker.protocol import (
    MAX_FRAME_BYTES,
    PROTOCOL_VERSION,
    ProtocolError,
    decode_frame,
    encode_frame,
    read_frame,
)


def test_encode_decode_roundtrip() -> None:
    data: bytes = encode_frame("run", {"a": 1})
    frame: dict[str, object] = decode_frame(data)
    assert frame["v"] == PROTOCOL_VERSION
    assert frame["kind"] == "run"
    assert frame["payload"] == {"a": 1}


def test_unknown_kind_rejected() -> None:
    with pytest.raises(ProtocolError):
        encode_frame("bogus", {})


def test_wrong_version_rejected() -> None:
    line: bytes = json.dumps({"v": 9, "kind": "hello", "payload": {}}).encode("utf-8")
    with pytest.raises(ProtocolError):
        decode_frame(line)


def test_oversized_frame_rejected() -> None:
    with pytest.raises(ProtocolError):
        decode_frame(b"x" * (MAX_FRAME_BYTES + 1))


def test_payload_must_be_object() -> None:
    line: bytes = json.dumps({"v": 1, "kind": "hello", "payload": []}).encode("utf-8")
    with pytest.raises(ProtocolError):
        decode_frame(line)


def test_partial_frame_at_eof_rejected() -> None:
    with pytest.raises(ProtocolError):
        read_frame(io.BytesIO(b'{"v":1,"kind":"run","payload":{}}'))
