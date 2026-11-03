"""DSH ACP adapter（`python/zgraph_worker/adapters/dsh_acp.py`）测试（v0.5/F5a）。

分层：
- **默认 CI**：fake ACP server（本文件内嵌）覆盖 transport/update 映射/permission/
  cancel/错误/stopReason/scratch 隔离；
- **真实 DSH 握手**：`ZGRAPH_REAL_DSH=1` 且本机存在 `dsh` 时运行（无 key 的
  initialize/session/new/close + prompt -32603 降级；不发起模型请求）。
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

import pytest

from zgraph_worker.adapters import dsh_acp

REPO_ROOT = Path(__file__).resolve().parents[2]
ADAPTER = REPO_ROOT / "python" / "zgraph_worker" / "adapters" / "dsh_acp.py"
PROTOCOL = "zgraph-harness/1"

FAKE_ACP = '''#!/usr/bin/env python3
import json
import os
import sys

MODE = os.environ.get("FAKE_ACP_MODE", "happy")
LOG = os.environ.get("FAKE_ACP_LOG")
OPTIONS = [
    {"optionId": "allow-once", "name": "Allow once", "kind": "allow_once"},
    {"optionId": "reject-once", "name": "Reject", "kind": "reject_once"},
]


def log(entry):
    if LOG:
        with open(LOG, "a", encoding="utf-8") as handle:
            handle.write(json.dumps(entry, ensure_ascii=False) + "\\n")


def send(payload):
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\\n")
    sys.stdout.flush()


def error(mid, code, message):
    send({"jsonrpc": "2.0", "id": mid, "error": {"code": code, "message": message}})


def send_update(update):
    send({"jsonrpc": "2.0", "method": "session/update",
          "params": {"sessionId": "sess-1", "update": update}})


def read_line():
    line = sys.stdin.readline()
    if not line:
        return None
    try:
        return json.loads(line)
    except json.JSONDecodeError:
        return None


def prompt_cancel(mid):
    send_update({"sessionUpdate": "agent_message_chunk",
                 "content": {"type": "text", "text": "working"}})
    while True:
        message = read_line()
        if message is None:
            return
        log({"client": message})
        if message.get("method") == "session/cancel":
            log({"cancel": True})
            send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "cancelled"}})
            return


def prompt_happy(mid):
    send_update({"sessionUpdate": "agent_message_chunk",
                 "content": {"type": "text", "text": "hello "}})
    send_update({"sessionUpdate": "agent_thought_chunk",
                 "content": {"type": "text", "text": "thinking"}})
    send_update({"sessionUpdate": "tool_call", "toolCallId": "c1", "title": "bash",
                 "kind": "other", "status": "in_progress", "rawInput": {"cmd": "ls"}})
    send({"jsonrpc": "2.0", "id": 999, "method": "session/request_permission",
          "params": {"sessionId": "sess-1", "toolCall": {"toolCallId": "c1"},
                     "options": OPTIONS}})
    while True:
        message = read_line()
        if message is None:
            return
        log({"permission": message.get("result")})
        if message.get("id") == 999:
            break
    send_update({"sessionUpdate": "tool_call_update", "toolCallId": "c1",
                 "status": "completed",
                 "content": [{"type": "content",
                              "content": {"type": "text", "text": "done"}}]})
    send_update({"sessionUpdate": "usage_update", "used": 10, "size": 1000})
    send_update({"sessionUpdate": "agent_message_chunk",
                 "content": {"type": "text", "text": "world"}})
    send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "end_turn"}})


def prompt(mid):
    if MODE == "error":
        error(mid, -32603,
              "Internal error: turn failed: llm-deepseek: no API key for provider route")
    elif MODE == "max_tokens":
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "max_tokens"}})
    elif MODE == "cancelled":
        send({"jsonrpc": "2.0", "id": mid, "result": {"stopReason": "cancelled"}})
    elif MODE == "cancel":
        prompt_cancel(mid)
    else:
        prompt_happy(mid)


def main():
    log({"argv": sys.argv, "cwd": os.getcwd(),
         "dsh_home": os.environ.get("DSH_HOME"), "tmpdir": os.environ.get("TMPDIR")})
    while True:
        line = sys.stdin.readline()
        if not line:
            return 0
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        method = message.get("method")
        mid = message.get("id")
        log({"server": method, "id": mid})
        if method == "initialize":
            version = 2 if MODE == "version" else 1
            name = "other-agent" if MODE == "wrong_agent" else "deepseek-harness-acp"
            send({"jsonrpc": "2.0", "id": mid,
                  "result": {"protocolVersion": version,
                             "agentInfo": {"name": name},
                             "authMethods": []}})
        elif method == "session/new":
            log({"session_new": message.get("params")})
            send({"jsonrpc": "2.0", "id": mid,
                  "result": {"sessionId": "sess-1", "configOptions": []}})
        elif method == "session/close":
            send({"jsonrpc": "2.0", "id": mid, "result": {}})
        elif method == "session/cancel":
            log({"cancel": True})
        elif method == "session/prompt":
            prompt(mid)
        elif "id" in message:
            log({"client": message})


if __name__ == "__main__":
    raise SystemExit(main())
'''


def _fake_acp(tmp_path: Path) -> Path:
    binary = tmp_path / "fake-dsh"
    binary.write_text(FAKE_ACP, encoding="utf-8")
    binary.chmod(binary.stat().st_mode | 0o100)
    return binary


def _env(tmp_path: Path, binary: Path, mode: str = "happy", **extra: str) -> dict[str, str]:
    env = dict(os.environ)
    env["DSH_BIN"] = str(binary)
    env["FAKE_ACP_MODE"] = mode
    env["FAKE_ACP_LOG"] = str(tmp_path / "acp.log")
    env["ZGRAPH_DSH_SCRATCH_DIR"] = str(tmp_path / "scratch")
    env["ZGRAPH_DSH_KEEP_SCRATCH"] = "1"
    env["ZGRAPH_DSH_INIT_TIMEOUT_MS"] = "10000"
    for key in (
        "DEEPSEEK_API_KEY",
        "ZGRAPH_REAL_DSH",
        "ZGRAPH_REAL_DSH_KEY",
    ):
        env.pop(key, None)
    env.update(extra)
    return env


def _run_adapter(
    env: dict[str, str],
    frames: list[dict[str, object]],
    timeout: float = 30.0,
    expect_results: int = 1,
) -> tuple[list[dict[str, object]], str, int]:
    """驱动 adapter：写帧 → 读到终止帧（result/error）→ 关 stdin → 收集退出。

    使用后台线程读 stdout（避免 TextIOWrapper 预读与 select 混用的漏帧问题）；
    保持 stdin 打开直到终止帧，模拟真实 harness。
    """
    import queue
    import threading

    process = subprocess.Popen(
        [sys.executable, str(ADAPTER)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    assert process.stdin is not None and process.stdout is not None
    lines: queue.Queue[str] = queue.Queue()

    def reader() -> None:
        assert process.stdout is not None
        for line in process.stdout:
            lines.put(line)

    thread = threading.Thread(target=reader, daemon=True)
    thread.start()
    for frame in frames:
        process.stdin.write(json.dumps(frame) + "\n")
    process.stdin.flush()
    has_run = any(frame.get("type") == "run" for frame in frames)
    frames_out: list[dict[str, object]] = []
    deadline = time.monotonic() + timeout
    terminal = 0
    while time.monotonic() < deadline:
        try:
            line = lines.get(timeout=0.05)
        except queue.Empty:
            continue
        if not line:
            break
        parsed = json.loads(line)
        frames_out.append(parsed)
        if has_run and parsed.get("type") in ("result", "error"):
            terminal += 1
            if terminal >= expect_results:
                break
        if not has_run and parsed.get("type") == "hello_ok":
            break
    process.stdin.close()
    process.wait(timeout=timeout)
    thread.join(timeout=5)
    while True:
        try:
            line = lines.get_nowait()
        except queue.Empty:
            break
        if line.strip():
            frames_out.append(json.loads(line))
    stderr = process.stderr.read() if process.stderr is not None else ""
    return frames_out, stderr, process.returncode


def _run_frames() -> list[dict[str, object]]:
    return [
        {"v": 1, "type": "hello", "protocol": PROTOCOL},
        {"v": 1, "type": "run", "inputs": {"user_message": "hi"}},
    ]


def _log_entries(path: Path) -> list[dict[str, object]]:
    if not path.exists():
        return []
    return [json.loads(line) for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]


def test_handshake_is_immediate_and_does_not_spawn_dsh(tmp_path: Path) -> None:
    env = _env(tmp_path, tmp_path / "missing-dsh")
    out, _err, code = _run_adapter(env, [{"v": 1, "type": "hello", "protocol": PROTOCOL}])
    assert code == 0
    assert out == [
        {
            "v": 1,
            "type": "hello_ok",
            "protocol": PROTOCOL,
            "provider": "acp",
            "capabilities": {"streaming": True, "reasoning": True},
        }
    ]


def test_happy_path_maps_updates_to_events_and_result(tmp_path: Path) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary)
    out, _err, code = _run_adapter(env, _run_frames())
    assert code == 0
    assert out[0]["type"] == "hello_ok"
    events = [frame for frame in out if frame.get("type") == "event"]
    kinds = [event["kind"] for event in events]
    assert kinds == ["delta", "reasoning", "tool_start", "tool_end", "delta"], kinds
    assert events[0]["text"] == "hello "
    assert events[1]["text"] == "thinking"
    assert events[2]["call_id"] == "c1"
    assert events[2]["tool"] == "bash"
    assert events[2]["args_summary"] == {"cmd": "ls"}
    assert events[3]["call_id"] == "c1"
    assert events[3]["ok"] is True
    assert events[3]["result_summary"] == {"text": "done"}
    assert events[4]["text"] == "world"
    result = out[-1]
    assert result["type"] == "result"
    assert result["content"] == "hello world"
    assert result["iterations"] == 1
    assert result["stop_reason"] == "EndTurn"
    assert "usage" not in result, "usage_update (used/size) must not fabricate token usage"
    payloads = _log_entries(tmp_path / "acp.log")
    permissions = [entry["permission"] for entry in payloads if "permission" in entry]
    assert permissions == [{"outcome": {"outcome": "cancelled"}}], permissions


def test_permission_policy_allow_once_selects_option(tmp_path: Path) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary, ZGRAPH_DSH_PERMISSION="allow_once")
    out, _err, _code = _run_adapter(env, _run_frames())
    assert out[-1]["type"] == "result"
    payloads = _log_entries(tmp_path / "acp.log")
    permissions = [entry["permission"] for entry in payloads if "permission" in entry]
    assert permissions == [{"outcome": {"outcome": "selected", "optionId": "allow-once"}}]


def test_session_reuse_and_reset(tmp_path: Path) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary)
    frames = [
        {"v": 1, "type": "hello", "protocol": PROTOCOL},
        {"v": 1, "type": "run", "inputs": {"m": 1}, "session": {"reset": True}},
        {"v": 1, "type": "run", "inputs": {"m": 2}, "session": {"reset": False}},
        {"v": 1, "type": "run", "inputs": {"m": 3}, "session": {"reset": True}},
    ]
    out, _err, code = _run_adapter(env, frames, expect_results=3)
    assert code == 0
    results = [frame for frame in out if frame.get("type") == "result"]
    assert len(results) == 3
    entries = _log_entries(tmp_path / "acp.log")
    new_sessions = [entry for entry in entries if "session_new" in entry]
    closes = [entry for entry in entries if entry.get("server") == "session/close"]
    assert len(new_sessions) == 2, "reset=true creates new; reset=false reuses"
    assert len(closes) == 2, "reset closes old session; EOF shutdown closes current"


def test_permission_policy_reject_once_selects_reject_option(tmp_path: Path) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary, ZGRAPH_DSH_PERMISSION="reject_once")
    out, _err, _code = _run_adapter(env, _run_frames())
    assert out[-1]["type"] == "result"
    entries = _log_entries(tmp_path / "acp.log")
    permissions = [entry["permission"] for entry in entries if "permission" in entry]
    assert permissions == [{"outcome": {"outcome": "selected", "optionId": "reject-once"}}]


def test_prompt_error_maps_to_plugin_error(tmp_path: Path) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary, mode="error")
    out, _err, code = _run_adapter(env, _run_frames())
    assert code == 0
    assert out[0]["type"] == "hello_ok"
    assert out[-1]["type"] == "error"
    assert out[-1]["code"] == "HARNESS_PLUGIN_ERROR"
    assert out[-1]["reason"] == "protocol"
    assert "no API key" in out[-1]["message"]
    assert not any(frame.get("type") == "result" for frame in out)


@pytest.mark.parametrize(
    ("mode", "expected"),
    [("max_tokens", "MaxTokens"), ("cancelled", "Other:cancelled")],
)
def test_stop_reason_mapping(tmp_path: Path, mode: str, expected: str) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary, mode=mode)
    out, _err, _code = _run_adapter(env, _run_frames())
    assert out[-1]["type"] == "result"
    assert out[-1]["stop_reason"] == expected


def test_version_mismatch_maps_to_version_error(tmp_path: Path) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary, mode="version")
    out, _err, code = _run_adapter(env, _run_frames())
    assert code == 0
    assert out[-1]["type"] == "error"
    assert out[-1]["reason"] == "version"
    assert not any(frame.get("type") == "result" for frame in out)


def test_wrong_agent_name_maps_to_version_error(tmp_path: Path) -> None:
    """REV-128 补充：agentInfo.name 非 deepseek-harness 前缀 → 拒绝错误二进制。"""
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary, mode="wrong_agent")
    out, _err, code = _run_adapter(env, _run_frames())
    assert code == 0
    assert out[-1]["type"] == "error"
    assert out[-1]["reason"] == "version"
    assert "agentInfo.name" in out[-1]["message"]
    assert not any(frame.get("type") == "result" for frame in out)
    entries = _log_entries(tmp_path / "acp.log")
    assert not any("session_new" in entry for entry in entries), "must not create session"


def test_spawn_failure_maps_to_spawn_failed(tmp_path: Path) -> None:
    missing = tmp_path / "missing-dsh"
    env = _env(tmp_path, missing)
    out, _err, code = _run_adapter(env, _run_frames())
    assert code == 0
    assert out[-1]["type"] == "error"
    assert out[-1]["reason"] == "spawn_failed"


def test_scratch_isolates_dsh_home_tmpdir_and_cwd(tmp_path: Path) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary)
    out, _err, _code = _run_adapter(env, _run_frames())
    assert out[-1]["type"] == "result"
    entries = _log_entries(tmp_path / "acp.log")
    startup = entries[0]
    scratch = str((tmp_path / "scratch").resolve())
    assert startup["argv"][1:] == ["--profile", "acp"]
    assert str(startup["dsh_home"]).startswith(scratch)
    assert str(startup["tmpdir"]).startswith(os.path.join(scratch, "tmp"))
    assert str(startup["cwd"]).startswith(scratch)
    session_new = [entry["session_new"] for entry in entries if "session_new" in entry]
    assert len(session_new) == 1, entries
    assert str(session_new[0]["cwd"]).startswith(os.path.join(scratch, "workspaces"))
    assert session_new[0]["mcpServers"] == []


def test_cancel_sends_session_cancel_without_result(tmp_path: Path) -> None:
    binary = _fake_acp(tmp_path)
    env = _env(tmp_path, binary, mode="cancel")
    process = subprocess.Popen(
        [sys.executable, str(ADAPTER)],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        env=env,
    )
    assert process.stdin is not None and process.stdout is not None
    process.stdin.write(json.dumps({"v": 1, "type": "hello", "protocol": PROTOCOL}) + "\n")
    process.stdin.write(json.dumps({"v": 1, "type": "run", "inputs": {"m": 1}}) + "\n")
    process.stdin.flush()
    first = process.stdout.readline()
    assert json.loads(first)["type"] == "hello_ok"
    time.sleep(0.4)
    process.stdin.write(json.dumps({"v": 1, "type": "cancel"}) + "\n")
    process.stdin.flush()
    process.stdin.close()
    rest = process.stdout.read()
    process.wait(timeout=5)
    out = [first] + [line for line in rest.splitlines() if line.strip()]
    frames_out = [json.loads(line) for line in out]
    assert process.returncode == 0
    assert not any(frame.get("type") == "result" for frame in frames_out)
    entries = _log_entries(tmp_path / "acp.log")
    assert any(entry.get("cancel") is True for entry in entries), entries


def test_wrong_protocol_and_invalid_hello_are_ignored() -> None:
    wrong = subprocess.run(
        [sys.executable, str(ADAPTER)],
        input=json.dumps({"v": 1, "type": "hello", "protocol": "zgraph-harness/2"}) + "\n",
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    assert wrong.stdout.strip() == ""
    invalid_limits = subprocess.run(
        [sys.executable, str(ADAPTER)],
        input=json.dumps({"v": 1, "type": "hello", "protocol": PROTOCOL, "limits": "bad"}) + "\n",
        capture_output=True,
        text=True,
        timeout=15,
        check=False,
    )
    assert invalid_limits.stdout.strip() == ""


def test_helpers_map_content_and_permission() -> None:
    assert dsh_acp.extract_text({"type": "text", "text": "hi"}) == "hi"
    assert dsh_acp.extract_text([{"content": {"type": "text", "text": "a"}}, "b"]) == "ab"
    assert dsh_acp.map_stop_reason("end_turn") == "EndTurn"
    assert dsh_acp.map_stop_reason("cancelled") == "Other:cancelled"
    selected = dsh_acp.choose_permission(
        "allow_once", [{"optionId": "a1", "kind": "allow_once"}]
    )
    assert selected == {"outcome": {"outcome": "selected", "optionId": "a1"}}
    assert dsh_acp.choose_permission("cancel", []) == {"outcome": {"outcome": "cancelled"}}


# ---------------------------------------------------------------------------
# 真实 DSH（F5b；默认 skip；无 key 只做握手 + prompt 降级，不发起模型请求）
# ---------------------------------------------------------------------------


def _real_dsh() -> str | None:
    configured = os.environ.get("DSH_BIN", "").strip()
    if configured:
        return configured
    found = shutil.which("dsh")
    if found:
        return found
    for candidate in dsh_acp.DSH_COMMON_PATHS:
        expanded = os.path.expanduser(candidate)
        if os.path.isfile(expanded) and os.access(expanded, os.X_OK):
            return expanded
    return None


REAL_DSH = os.environ.get("ZGRAPH_REAL_DSH") == "1"


@pytest.mark.skipif(not REAL_DSH, reason="set ZGRAPH_REAL_DSH=1 to run real DSH handshake")
def test_real_dsh_initialize_session_close(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    binary = _real_dsh()
    if binary is None:
        pytest.skip("dsh binary not found (set DSH_BIN)")
    env_backup = os.environ.get("DSH_BIN")
    os.environ["DSH_BIN"] = binary
    try:
        scratch = dsh_acp.Scratch(str(tmp_path / "scratch"))
        client = dsh_acp.AcpClient(scratch)
        client.initialize(30.0)
        assert client.initialized is True
        assert client.agent_info.get("name") == "deepseek-harness-acp", client.agent_info
        session_id = client.new_session(scratch.new_workspace(), 30.0)
        assert isinstance(session_id, str) and session_id
        client.close_session(session_id, 30.0)
        client.shutdown()
        captured = capsys.readouterr()
        assert "initialize ok" in captured.err
        assert "session/new ok" in captured.err
    finally:
        if env_backup is None:
            os.environ.pop("DSH_BIN", None)
        else:
            os.environ["DSH_BIN"] = env_backup


@pytest.mark.skipif(not REAL_DSH, reason="set ZGRAPH_REAL_DSH=1 to run real DSH handshake")
def test_real_dsh_no_key_prompt_degrades_to_plugin_error(tmp_path: Path) -> None:
    binary = _real_dsh()
    if binary is None:
        pytest.skip("dsh binary not found (set DSH_BIN)")
    if os.environ.get("DEEPSEEK_API_KEY"):
        pytest.skip("DEEPSEEK_API_KEY present; no-key degradation test would issue a model request")
    env = dict(os.environ)
    env["DSH_BIN"] = binary
    env["ZGRAPH_DSH_SCRATCH_DIR"] = str(tmp_path / "scratch")
    env["ZGRAPH_DSH_KEEP_SCRATCH"] = "1"
    env.pop("DEEPSEEK_API_KEY", None)
    out, err, code = _run_adapter(env, _run_frames(), timeout=90.0)
    assert code == 0
    assert out[0]["type"] == "hello_ok"
    assert "initialize ok" in err
    assert "session/new ok" in err
    assert out[-1]["type"] == "error"
    assert out[-1]["reason"] == "protocol"
    assert "no API key" in out[-1]["message"]
    assert not any(frame.get("type") == "result" for frame in out)
