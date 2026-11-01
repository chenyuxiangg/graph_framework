"""Codex（`codex exec --json`）到 `zgraph-harness/1` 的薄适配器（v0.3/V4）。

协议（DR-011 §2）：`hello`/`hello_ok` → `run` → `result`/`error`；本适配器的
能力为 **streaming=false**（Codex 只在 item started/updated/completed 给整段文本），
因此**只发 `result`**、不发任何 `event` delta（DR-011 §3.1）。

实现边界（DR-011 §3.1）：
- 退出码只有 0/1，成功与否以 `turn.completed`/`turn.failed` 为准；
- 最终消息优先读 `--output-last-message` 文件，缺失时回退 `item.completed`
  的 `agent_message` 文本；
- `iterations` 无对应物，固定 **1**；usage 映射 input/output/reasoning，
  `cached_input_tokens`/`cache_write_input_tokens` 丢弃；
- `StopReason` 不新增枚举：`turn.completed`→`EndTurn`、`turn.failed`→`Other:turn_failed`。

用法（由 AgentConfig.harness 配置）：

    {"kind":"subprocess","command":"python3",
     "args":["-m","zgraph_worker.adapters.codex"],
     "protocol":"zgraph-harness/1"}

Codex 二进制由用户自备（`CODEX_BIN` 或 PATH 中的 `codex`）。
"""

from __future__ import annotations

import json
import os
import select
import shutil
import signal
import subprocess
import sys
import tempfile
import time
from typing import Any, Dict, IO, List, Optional, Tuple

PROTOCOL = "zgraph-harness/1"
PROTOCOL_VERSION = 1
PROVIDER = "codex_exec"
CANCEL_GRACE_SECONDS = 2.0
READ_POLL_SECONDS = 0.05
MAX_PREVIEW_CHARS = 512


def send(payload: Dict[str, Any]) -> None:
    """向父进程写一帧 NDJSON（stdout）。"""
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def read_frame(stream: IO[str]) -> Optional[Dict[str, Any]]:
    """从 stdin 读一帧；EOF/非法 JSON 返回 `None`。"""
    line = stream.readline()
    if not line:
        return None
    try:
        frame = json.loads(line)
    except json.JSONDecodeError:
        return None
    return frame if isinstance(frame, dict) else None


def codex_binary() -> str:
    """返回 Codex 可执行文件路径（`CODEX_BIN` 覆盖；缺失返回配置值用于报错）。"""
    configured = os.environ.get("CODEX_BIN", "codex")
    resolved = shutil.which(configured)
    return resolved if resolved is not None else configured


def build_prompt(inputs: Dict[str, Any]) -> str:
    """把 agent 输入（已映射字段）序列化为 Codex prompt（确定性键序）。"""
    return json.dumps(inputs, ensure_ascii=False, sort_keys=True)


def build_command(binary: str, prompt: str, output_file: str) -> List[str]:
    """构造 `codex exec --json ... <prompt>` 命令（不经 shell）。"""
    return [
        binary,
        "exec",
        "--json",
        "--skip-git-repo-check",
        "--output-last-message",
        output_file,
        prompt,
    ]


def terminate(process: subprocess.Popen[str]) -> None:
    """取消升级：SIGTERM 进程组 → 宽限 → SIGKILL（无孤儿）。"""
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except (ProcessLookupError, PermissionError):
        return
    deadline = time.monotonic() + CANCEL_GRACE_SECONDS
    while time.monotonic() < deadline:
        if process.poll() is not None:
            return
        time.sleep(READ_POLL_SECONDS)
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except (ProcessLookupError, PermissionError):
        return


def parse_usage(raw: Optional[Dict[str, Any]]) -> Dict[str, Any]:
    """Codex usage → 协议 usage（cached 字段丢弃；缺省全 0）。"""
    usage = raw if isinstance(raw, dict) else {}
    prompt = int(usage.get("input_tokens", 0) or 0)
    completion = int(usage.get("output_tokens", 0) or 0)
    reasoning = int(usage.get("reasoning_output_tokens", 0) or 0)
    return {
        "input_tokens": prompt,
        "output_tokens": completion,
        "total_tokens": prompt + completion,
        "reasoning_output_tokens": reasoning,
    }


def parse_thread_events(lines: List[str]) -> Dict[str, Any]:
    """解析 Codex JSONL `ThreadEvent`（8 类；未知类型忽略，向前兼容）。"""
    outcome: Dict[str, Any] = {
        "content": "",
        "usage": None,
        "completed": False,
        "failed": False,
        "error": "",
        "schema_violation": False,
    }
    for line in lines:
        line = line.strip()
        if not line:
            continue
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            outcome["schema_violation"] = True
            continue
        if not isinstance(event, dict):
            outcome["schema_violation"] = True
            continue
        kind = event.get("type")
        if kind == "turn.completed":
            outcome["completed"] = True
            outcome["usage"] = event.get("usage")
        elif kind == "turn.failed":
            outcome["failed"] = True
            outcome["error"] = _error_message(event)
        elif kind == "error":
            outcome["failed"] = True
            outcome["error"] = _error_message(event)
        elif kind == "item.completed":
            item = event.get("item")
            if isinstance(item, dict) and item.get("type") == "agent_message":
                text = item.get("text")
                if isinstance(text, str):
                    outcome["content"] = text
    return outcome


def _error_message(event: Dict[str, Any]) -> str:
    error = event.get("error")
    if isinstance(error, dict) and isinstance(error.get("message"), str):
        return error["message"]
    if isinstance(event.get("message"), str):
        return event["message"]
    return "codex reported a failed turn"


def read_output_file(path: str) -> str:
    """读取 `--output-last-message` 文件（缺失/空 → 空串）。"""
    try:
        with open(path, "r", encoding="utf-8") as handle:
            return handle.read()
    except OSError:
        return ""


def collect_events_with_cancel(
    process: subprocess.Popen[str], stdin: IO[str]
) -> Tuple[List[str], bool]:
    """读取 Codex stdout，同时监听 stdin 的 `cancel` 帧（返回 lines, cancelled）。"""
    lines: List[str] = []
    cancelled = False
    assert process.stdout is not None
    stream = process.stdout
    while True:
        readable, _, _ = select.select([stream, stdin], [], [], READ_POLL_SECONDS)
        if stream in readable:
            line = stream.readline()
            if not line:
                break
            lines.append(line)
        if stdin in readable:
            frame = read_frame(stdin)
            if frame is not None and frame.get("type") == "cancel":
                cancelled = True
                terminate(process)
                break
    try:
        process.wait(timeout=CANCEL_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        terminate(process)
        process.wait()
    return lines, cancelled


def execute_run(run_frame: Dict[str, Any]) -> None:
    """执行一次 Codex run 并回写 `result` / `error`（协议帧）。"""
    inputs = run_frame.get("inputs")
    if not isinstance(inputs, dict):
        inputs = {}
    binary = codex_binary()
    if shutil.which(binary) is None:
        send(
            {
                "v": PROTOCOL_VERSION,
                "type": "error",
                "code": "HARNESS_PLUGIN_ERROR",
                "reason": "spawn_failed",
                "message": "codex binary not found (user-provided; set CODEX_BIN or PATH)",
            }
        )
        return
    with tempfile.NamedTemporaryFile(prefix="zgraph-codex-", suffix=".txt") as output:
        command = build_command(binary, build_prompt(inputs), output.name)
        try:
            process = subprocess.Popen(
                command,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                start_new_session=True,
            )
        except OSError as error:
            send(
                {
                    "v": PROTOCOL_VERSION,
                    "type": "error",
                    "code": "HARNESS_PLUGIN_ERROR",
                    "reason": "spawn_failed",
                    "message": f"failed to spawn codex: {error}",
                }
            )
            return
        lines, _cancelled = collect_events_with_cancel(process, sys.stdin)
        outcome = parse_thread_events(lines)
        if outcome["failed"]:
            send(
                {
                    "v": PROTOCOL_VERSION,
                    "type": "error",
                    "code": "HARNESS_PLUGIN_ERROR",
                    "reason": "protocol",
                    "message": outcome["error"],
                }
            )
            return
        if not outcome["completed"]:
            stderr = ""
            if process.stderr is not None:
                stderr = process.stderr.read()[:MAX_PREVIEW_CHARS]
            send(
                {
                    "v": PROTOCOL_VERSION,
                    "type": "error",
                    "code": "HARNESS_PLUGIN_ERROR",
                    "reason": "exit_nonzero",
                    "message": stderr or "codex exited without turn.completed",
                }
            )
            return
        content = read_output_file(output.name) or outcome["content"]
        send(
            {
                "v": PROTOCOL_VERSION,
                "type": "result",
                "content": content,
                "iterations": 1,
                # turn.failed 已在上面走 error 分支；成功路径固定 EndTurn（不新增枚举）。
                "stop_reason": "EndTurn",
                "usage": parse_usage(outcome["usage"]),
            }
        )


def handshake() -> bool:
    """握手：读 `hello`、校验协议，回 `hello_ok`（失败返回 False）。"""
    hello = read_frame(sys.stdin)
    if hello is None or hello.get("type") != "hello":
        return False
    if hello.get("protocol") != PROTOCOL or hello.get("v") != PROTOCOL_VERSION:
        return False
    send(
        {
            "v": PROTOCOL_VERSION,
            "type": "hello_ok",
            "protocol": PROTOCOL,
            "provider": PROVIDER,
            "capabilities": {"streaming": False, "reasoning": False},
        }
    )
    return True


def main() -> int:
    """协议主循环：握手 → 逐 `run` 执行（`cancel` 帧在 run 中处理）。"""
    if not handshake():
        return 0
    while True:
        frame = read_frame(sys.stdin)
        if frame is None:
            return 0
        kind = frame.get("type")
        if kind == "cancel":
            return 0
        if kind == "run":
            execute_run(frame)


if __name__ == "__main__":
    raise SystemExit(main())
