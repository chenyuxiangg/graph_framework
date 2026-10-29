"""worker 运行时端到端单测：真实子进程 + NDJSON 握手/执行/错误。"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
from pathlib import Path
from typing import Any

from zgraph_worker.protocol import encode_frame

SDK_DIR: Path = Path(__file__).resolve().parents[1]
REPO_ROOT: Path = SDK_DIR.parent
WORKERS_DIR: Path = REPO_ROOT / "tests" / "fixtures" / "workers"


def _env() -> dict[str, str]:
    env: dict[str, str] = dict(os.environ)
    env["PYTHONPATH"] = os.pathsep.join([str(SDK_DIR), str(WORKERS_DIR)])
    env["PYTHONHASHSEED"] = "0"
    return env


def _spawn() -> subprocess.Popen[bytes]:
    proc: subprocess.Popen[bytes] = subprocess.Popen(
        [sys.executable, "-m", "zgraph_worker.runtime"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=REPO_ROOT,
        env=_env(),
    )

    def _drain() -> None:
        # 必须持续排空 stderr，否则 worker 写满 pipe 会阻塞（协议检查单）。
        assert proc.stderr is not None
        while proc.stderr.readline():
            continue

    threading.Thread(target=_drain, daemon=True).start()
    return proc


def _readline(proc: subprocess.Popen[bytes]) -> dict[str, Any]:
    assert proc.stdout is not None
    line: bytes = proc.stdout.readline()
    assert line, "worker closed stdout"
    return json.loads(line)


def _run_frame(module: str, function: str, inputs: dict[str, Any]) -> dict[str, Any]:
    return {
        "v": 1,
        "kind": "run",
        "payload": {
            "run_id": "r",
            "instance_id": "i",
            "node_id": "n",
            "attempt": 0,
            "module": module,
            "function": function,
            "inputs": inputs,
            "params": {},
            "user_id": "u",
            "global_vars": {},
            "seed": 0,
            "deadline_ms": 0,
            "trace_id": "t",
        },
    }


def _handshake(proc: subprocess.Popen[bytes], run: dict[str, Any]) -> None:
    hello: dict[str, Any] = _readline(proc)
    assert hello["kind"] == "hello"
    assert hello["payload"]["protocol_version"] == 1
    assert proc.stdin is not None
    proc.stdin.write(encode_frame("hello_ack", {"accepted": True}))
    proc.stdin.flush()
    proc.stdin.write(json.dumps(run).encode("utf-8") + b"\n")
    proc.stdin.flush()


def test_echo_runtime_roundtrip() -> None:
    proc = _spawn()
    try:
        _handshake(proc, _run_frame("zgraph_worker.echo", "run", {"message": "hi"}))
        frames: list[dict[str, Any]] = []
        while True:
            frame = _readline(proc)
            frames.append(frame)
            if frame["kind"] in {"result", "error"}:
                break
        assert frames[-1]["kind"] == "result"
        assert frames[-1]["payload"]["output"]["echo"] == "hi"
        assert any(frame["kind"] == "emit" for frame in frames)
        assert proc.wait(timeout=5) == 0
    finally:
        proc.kill()


def test_import_error_is_reported() -> None:
    proc = _spawn()
    try:
        _handshake(proc, _run_frame("zgraph_worker.missing_module", "run", {}))
        frame: dict[str, Any] = _readline(proc)
        assert frame["kind"] == "error"
        assert frame["payload"]["code"] == "NODE_WORKER_IMPORT"
        assert proc.wait(timeout=5) == 2
    finally:
        proc.kill()


def test_return_not_dict_is_reported() -> None:
    proc = _spawn()
    try:
        _handshake(proc, _run_frame("bad_return", "run", {}))
        frame: dict[str, Any] = _readline(proc)
        assert frame["kind"] == "error"
        assert frame["payload"]["code"] == "NODE_RETURN_NOT_DICT"
    finally:
        proc.kill()


def test_print_and_stderr_do_not_pollute_protocol() -> None:
    proc = _spawn()
    try:
        _handshake(proc, _run_frame("zgraph_worker.noisy", "run", {}))
        frames: list[dict[str, Any]] = []
        while True:
            frame = _readline(proc)
            frames.append(frame)
            if frame["kind"] in {"result", "error"}:
                break
        assert frames[-1]["kind"] == "result"
        assert frames[-1]["payload"]["output"]["ok"] is True
    finally:
        proc.kill()


def test_input_is_disabled() -> None:
    proc = _spawn()
    try:
        _handshake(proc, _run_frame("zgraph_worker.inputter", "run", {}))
        frame: dict[str, Any] = _readline(proc)
        assert frame["kind"] == "error"
        assert frame["payload"]["code"] == "NODE_PYTHON_EXCEPTION"
        assert "input" in frame["payload"]["message"].lower()
    finally:
        proc.kill()


def test_cancel_is_cooperative() -> None:
    proc = _spawn()
    try:
        _handshake(proc, _run_frame("zgraph_worker.slow", "run", {"seconds": 5}))
        assert proc.stdin is not None
        proc.stdin.write(encode_frame("cancel", {}))
        proc.stdin.flush()
        frame: dict[str, Any] = _readline(proc)
        while frame["kind"] not in {"result", "error"}:
            frame = _readline(proc)
        assert frame["kind"] == "error"
        assert frame["payload"]["code"] == "NODE_CANCELLED"
        assert proc.wait(timeout=5) == 2
    finally:
        proc.kill()
