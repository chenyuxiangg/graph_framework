"""Codex adapter（`python/zgraph_worker/adapters/codex.py`）单元测试。"""

from __future__ import annotations

import json
import os
import signal
import stat
import subprocess
import sys
import time
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


VERSION_REPLY = '''#!/usr/bin/env python3
import sys
print("codex-cli 0.166.0")
raise SystemExit(0)
'''


def _fake_codex(tmp_path: Path, script: str = FAKE_CODEX) -> Path:
    """写 fake codex；自动处理 `--version`（adapter 的 F5d 版本诊断会先探测）。"""
    binary = tmp_path / "codex"
    handling = '''#!/usr/bin/env python3
import sys
if "--version" in sys.argv:
    print("codex-cli 0.166.0")
    raise SystemExit(0)
'''
    if script.startswith("#!/usr/bin/env python3\n"):
        body = script[len("#!/usr/bin/env python3\n"):]
    else:
        body = script
    binary.write_text(handling + body, encoding="utf-8")
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


def test_build_command_includes_ephemeral() -> None:
    command = codex.build_command("codex", "hello", "/tmp/out.txt")
    assert "--ephemeral" in command, "F5d #2：必须 --ephemeral，避免 CODEX_HOME 写 session/sqlite"
    assert command.index("--ephemeral") < command.index("--skip-git-repo-check")


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


def test_error_event_then_turn_completed_clears_failed() -> None:
    lines = [
        json.dumps({"type": "error", "message": "Reconnecting ... 1/5"}),
        json.dumps(
            {
                "type": "item.completed",
                "item": {"id": "i1", "type": "agent_message", "text": "recovered"},
            }
        ),
        json.dumps(
            {"type": "turn.completed", "usage": {"input_tokens": 11, "output_tokens": 7}}
        ),
    ]
    outcome = codex.parse_thread_events(lines)
    assert outcome["completed"] is True
    assert outcome["failed"] is False, "F5d #1：turn.completed 必须清除顶层 error 的 failed"
    assert outcome["error"] == ""
    assert outcome["content"] == "recovered"


def test_command_execution_maps_tool_start_and_end() -> None:
    started = {
        "type": "item.started",
        "item": {
            "id": "item_1",
            "type": "command_execution",
            "command": "/bin/bash -lc 'echo hi'",
            "aggregated_output": "",
            "exit_code": None,
            "status": "in_progress",
        },
    }
    completed = {
        "type": "item.completed",
        "item": {
            "id": "item_1",
            "type": "command_execution",
            "command": "/bin/bash -lc 'echo hi'",
            "aggregated_output": "hi\n",
            "exit_code": 0,
            "status": "completed",
        },
    }
    lines = [json.dumps(started), json.dumps(completed), json.dumps({"type": "turn.completed"})]
    outcome = codex.parse_thread_events(lines)
    events = outcome["tool_events"]
    assert [event["kind"] for event in events] == ["tool_start", "tool_end"]
    assert events[0]["call_id"] == "item_1"
    assert events[0]["tool"] == "command_execution"
    assert events[0]["args_summary"]["command"] == "/bin/bash -lc 'echo hi'"
    assert events[1]["call_id"] == "item_1"
    assert events[1]["ok"] is True, "ok == exit_code==0"
    assert events[1]["result_summary"]["exit_code"] == 0
    assert events[1]["result_summary"]["output_tail"].endswith("hi\n")


def test_command_execution_nonzero_exit_is_not_ok() -> None:
    item = {
        "type": "item.completed",
        "item": {
            "id": "item_2",
            "type": "command_execution",
            "command": "/bin/false",
            "aggregated_output": "boom\n",
            "exit_code": 7,
            "status": "completed",
        },
    }
    outcome = codex.parse_thread_events([json.dumps(item)])
    events = outcome["tool_events"]
    # completed 无 started → 用真实 completed 数据补配对（tool_start + tool_end）。
    assert [event["kind"] for event in events] == ["tool_start", "tool_end"]
    assert events[1]["ok"] is False, "exit_code=7 → ok=false"
    assert events[1]["result_summary"]["exit_code"] == 7


def test_no_tool_events_without_command_execution() -> None:
    lines = [
        json.dumps({"type": "item.completed", "item": {"type": "agent_message", "text": "hi"}}),
        json.dumps({"type": "turn.completed"}),
    ]
    outcome = codex.parse_thread_events(lines)
    assert outcome["tool_events"] == [], "无 command_execution 时不得伪造 tool 事件"


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


def test_validate_hello_checks_protocol_and_limits() -> None:
    assert codex.validate_hello({"type": "hello", "protocol": "zgraph-harness/1", "v": 1})
    assert codex.validate_hello(
        {"type": "hello", "protocol": "zgraph-harness/1", "v": 1, "limits": {"max_events": 1}}
    )
    assert not codex.validate_hello(
        {"type": "hello", "protocol": "zgraph-harness/1", "v": 1, "limits": "bad"}
    )
    assert not codex.validate_hello({"type": "hello", "protocol": "zgraph-harness/2", "v": 1})
    assert not codex.validate_hello(None)


def test_stderr_capture_keeps_head_and_tail_bounded() -> None:
    capture = codex.StderrCapture()
    capture.push("HEAD-" + ("a" * 6000) + "-MID-" + ("b" * 6000) + "-TAILEND")
    assert capture.head_text().startswith("HEAD-")
    assert capture.tail_text().endswith("TAILEND")
    assert len(capture.head_text()) <= codex.STDERR_CAPTURE_BYTES
    assert len(capture.tail_text()) <= codex.STDERR_CAPTURE_BYTES


def test_adapter_turn_failed_returns_structured_error(tmp_path: Path) -> None:
    failed_codex = """#!/usr/bin/env python3
import json
import sys
print(json.dumps({"type": "turn.failed", "error": {"message": "boom"}}), flush=True)
"""
    fake = _fake_codex(tmp_path, failed_codex)
    env = dict(os.environ)
    env["CODEX_BIN"] = str(fake)
    frames = [
        {"v": 1, "type": "hello", "protocol": "zgraph-harness/1"},
        {"v": 1, "type": "run", "inputs": {}},
    ]
    out = _run_adapter(env, frames)
    assert out[1]["type"] == "error"
    assert out[1]["reason"] == "protocol"
    assert "boom" in out[1]["message"]


def test_adapter_emits_real_tool_events_before_result(tmp_path: Path) -> None:
    tool_codex = """#!/usr/bin/env python3
import json
import sys

args = sys.argv[1:]
output = None
if "--output-last-message" in args:
    output = args[args.index("--output-last-message") + 1]
events = [
    {"type": "thread.started", "thread_id": "t1"},
    {"type": "item.started", "item": {"id": "item_1", "type": "command_execution",
        "command": "echo hi", "aggregated_output": "", "exit_code": None,
        "status": "in_progress"}},
    {"type": "item.completed", "item": {"id": "item_1", "type": "command_execution",
        "command": "echo hi", "aggregated_output": "hi\\n", "exit_code": 0,
        "status": "completed"}},
    {"type": "item.completed", "item": {"id": "i1", "type": "agent_message", "text": "done"}},
    {"type": "turn.completed", "usage": {"input_tokens": 11, "output_tokens": 7,
                                        "reasoning_output_tokens": 2}},
]
for event in events:
    print(json.dumps(event), flush=True)
if output:
    with open(output, "w", encoding="utf-8") as handle:
        handle.write("done")
"""
    fake = _fake_codex(tmp_path, tool_codex)
    env = dict(os.environ)
    env["CODEX_BIN"] = str(fake)
    frames = [
        {"v": 1, "type": "hello", "protocol": "zgraph-harness/1"},
        {"v": 1, "type": "run", "inputs": {"user_message": "hi"}},
    ]
    out = _run_adapter(env, frames)
    assert [frame["type"] for frame in out] == ["hello_ok", "event", "event", "result"]
    assert out[1]["kind"] == "tool_start" and out[1]["call_id"] == "item_1"
    assert out[2]["kind"] == "tool_end" and out[2]["ok"] is True
    assert out[3]["content"] == "done"
    assert out[3]["usage"]["total_tokens"] == 18


def test_adapter_stdin_eof_exits_zero(tmp_path: Path) -> None:
    fake = _fake_codex(tmp_path)
    env = dict(os.environ)
    env["CODEX_BIN"] = str(fake)
    result = subprocess.run(
        [sys.executable, str(ADAPTER)],
        input=json.dumps({"v": 1, "type": "hello", "protocol": "zgraph-harness/1"}) + "\n",
        capture_output=True,
        text=True,
        env=env,
        timeout=30,
        check=False,
    )
    assert result.returncode == 0, "F5d #8：stdin close（hello 后 EOF）→ exit 0"
    frames = [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    assert frames[0]["type"] == "hello_ok"
    assert len(frames) == 1


def test_cancel_sigterm_kills_codex_process_group_within_three_seconds(tmp_path: Path) -> None:
    pid_file = tmp_path / "codex.pid"
    sleeping_codex = f"""#!/usr/bin/env python3
import os
import signal
import sys
import time

open({str(pid_file)!r}, "w").write(str(os.getpid()))
# 模拟 codex 长会话：忽略 SIGTERM 第一跳，等待进程组 SIGKILL/父进程回收。
while True:
    time.sleep(0.05)
"""
    fake = _fake_codex(tmp_path, sleeping_codex)
    env = dict(os.environ)
    env["CODEX_BIN"] = str(fake)
    adapter = subprocess.Popen(
        [sys.executable, str(ADAPTER)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    assert adapter.stdin is not None
    adapter.stdin.write(json.dumps({"v": 1, "type": "hello", "protocol": "zgraph-harness/1"}) + "\n")
    adapter.stdin.write(json.dumps({"v": 1, "type": "run", "inputs": {}}) + "\n")
    adapter.stdin.flush()
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline and not pid_file.exists():
        time.sleep(0.02)
    assert pid_file.exists(), "fake codex must start (bounded wait)"
    codex_pid = int(pid_file.read_text().strip())
    started = time.monotonic()
    adapter.send_signal(signal.SIGTERM)
    try:
        adapter.wait(timeout=3)
    finally:
        if adapter.stdin is not None:
            adapter.stdin.close()
    assert time.monotonic() - started < 3.0, "F5d #8：取消后 3s 内适配器退出"
    gone_deadline = time.monotonic() + 3.0
    while time.monotonic() < gone_deadline:
        try:
            os.kill(codex_pid, 0)
        except ProcessLookupError:
            break
        except PermissionError:
            break
        time.sleep(0.02)
    try:
        os.kill(codex_pid, 0)
    except (ProcessLookupError, PermissionError):
        pass
    else:
        raise AssertionError(f"F5d #8：codex 进程 {codex_pid} 取消后仍存活（孤儿）")
