"""Codex adapter（`python/zgraph_worker/adapters/codex.py`）单元测试。"""

from __future__ import annotations

import json
import os
import stat
import subprocess
import sys
from pathlib import Path

import pytest

from zgraph_worker.adapters import codex

REPO_ROOT = Path(__file__).resolve().parents[2]
ADAPTER = REPO_ROOT / "python" / "zgraph_worker" / "adapters" / "codex.py"

FAKE_CODEX = """#!/usr/bin/env python3
import json
import sys

args = sys.argv[1:]
output = None
if "--output-last-message" in args:
    output = args[args.index("--output-last-message") + 1]
for event in [
    {"type": "thread.started", "thread_id": "t1"},
    {"type": "item.completed", "item": {"id": "i1", "type": "agent_message", "text": "codex-final"}},
    {"type": "turn.completed", "usage": {"input_tokens": 10, "output_tokens": 5,
                                        "reasoning_output_tokens": 2, "cached_input_tokens": 99}},
]:
    print(json.dumps(event), flush=True)
if output:
    with open(output, "w", encoding="utf-8") as handle:
        handle.write("codex-final")
"""


def _fake_codex(tmp_path: Path, script: str = FAKE_CODEX) -> Path:
    binary = tmp_path / "codex"
    binary.write_text(script, encoding="utf-8")
    binary.chmod(binary.stat().st_mode | stat.S_IXUSR)
    return binary


def test_build_prompt_is_deterministic() -> None:
    assert codex.build_prompt({"b": 1, "a": "x"}) == '{"a": "x", "b": 1}'


def test_build_command_passes_json_and_output_file() -> None:
    command = codex.build_command("codex", "hello", "/tmp/out.txt")
    assert command[:2] == ["codex", "exec"]
    assert "--json" in command
    assert command[command.index("--output-last-message") + 1] == "/tmp/out.txt"
    assert command[-1] == "hello"


def test_parse_usage_drops_cached_tokens() -> None:
    usage = codex.parse_usage(
        {"input_tokens": 10, "output_tokens": 5, "reasoning_output_tokens": 2,
         "cached_input_tokens": 99}
    )
    assert usage["input_tokens"] == 10
    assert usage["output_tokens"] == 5
    assert usage["total_tokens"] == 15
    assert usage["reasoning_output_tokens"] == 2
    assert "cached_input_tokens" not in usage


def test_parse_thread_events_maps_completed_turn() -> None:
    lines = [
        json.dumps({"type": "thread.started"}),
        json.dumps({"type": "item.completed", "item": {"type": "agent_message", "text": "hi"}}),
        json.dumps({"type": "turn.completed", "usage": {"input_tokens": 1, "output_tokens": 1}}),
        "not json",
    ]
    outcome = codex.parse_thread_events(lines)
    assert outcome["content"] == "hi"
    assert outcome["completed"] is True
    assert outcome["failed"] is False
    assert outcome["schema_violation"] is True


def test_parse_thread_events_maps_failed_turn() -> None:
    lines = [json.dumps({"type": "turn.failed", "error": {"message": "boom"}})]
    outcome = codex.parse_thread_events(lines)
    assert outcome["failed"] is True
    assert outcome["error"] == "boom"


def _run_adapter(env: dict[str, str], frames: list[dict[str, object]]) -> list[dict[str, object]]:
    payload = "".join(json.dumps(frame) + "\n" for frame in frames)
    result = subprocess.run(
        [sys.executable, str(ADAPTER)],
        input=payload,
        capture_output=True,
        text=True,
        env=env,
        timeout=30,
        check=False,
    )
    return [json.loads(line) for line in result.stdout.splitlines() if line.strip()]


def test_adapter_handshake_and_result_with_fake_codex(tmp_path: Path) -> None:
    fake = _fake_codex(tmp_path)
    env = dict(os.environ)
    env["CODEX_BIN"] = str(fake)
    frames = [
        {"v": 1, "type": "hello", "protocol": "zgraph-harness/1"},
        {"v": 1, "type": "run", "inputs": {"user_message": "hi"}},
    ]
    out = _run_adapter(env, frames)
    assert out[0]["type"] == "hello_ok"
    assert out[0]["provider"] == "codex_exec"
    assert out[0]["capabilities"]["streaming"] is False
    assert out[1]["type"] == "result"
    assert out[1]["content"] == "codex-final"
    assert out[1]["iterations"] == 1
    assert out[1]["stop_reason"] == "EndTurn"
    assert out[1]["usage"]["total_tokens"] == 15


def test_adapter_missing_codex_returns_structured_error(tmp_path: Path) -> None:
    env = dict(os.environ)
    env["CODEX_BIN"] = str(tmp_path / "missing-codex")
    frames = [
        {"v": 1, "type": "hello", "protocol": "zgraph-harness/1"},
        {"v": 1, "type": "run", "inputs": {}},
    ]
    out = _run_adapter(env, frames)
    assert out[1]["type"] == "error"
    assert out[1]["reason"] == "spawn_failed"
