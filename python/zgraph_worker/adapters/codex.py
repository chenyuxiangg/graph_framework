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
- `StopReason` 不新增枚举：`turn.completed`→`EndTurn`、`turn.failed`→error；
- REV-099 C3：安装 SIGTERM/SIGINT handler，父进程取消时先 `killpg` Codex
  进程组再退出（`start_new_session=True` 的 Codex 子进程不残留）；
- REV-099 N5：Codex `stderr` 由后台线程持续排空（head/tail 各 4 KiB，防管道写满死锁）。

v0.5/F5d（DR-020 Codex FINAL）：
- `--ephemeral`：不落 sessions/rollout/sqlite（CODEX_HOME 保持干净，避免输入持久层）；
- **tool 事件**：`-s danger-full-access`（或容器外非嵌套）下 Codex 会发
  `item.started/item.completed[type=command_execution]`，映射为
  `tool_start{call_id=item.id, tool="command_execution", args_summary:{command}}` +
  `tool_end{call_id, ok=(exit_code==0), result_summary:{exit_code,status,output_tail}}`；
  **无 command_execution 时不发任何 tool 事件**（默认 sandbox 在嵌套环境静默无事件，
  不得据此伪造）；
- 顶层 `error` 事件置 failed；**`turn.completed` 清除 failed**（Codex 重连后成功
  不应误报）；
- `iterations` 固定 **1**（codex exec 内部可能多轮，无对应字段；**usage 是
  turn.completed 的累计值**，v0.3 usage 字段口径不丢信息）；`cached_input_tokens`
  丢弃（协议无缓存维度）；
- 版本 pin **`@openai/codex@0.166.0`**；运行前 `codex --version` 写 stderr 诊断，
  版本不符仅 warn（不改变协议/不阻断）；
- **ARG_MAX**：prompt 经 argv 传递（`codex exec ... <prompt>`）；macOS ARG_MAX
  ≈1 MiB，**超大 prompt 可能 E2BIG**（本适配器按 DR-020 不做临时文件回退）；
- **认证分层**：默认 provider 需 `codex login` / `OPENAI_API_KEY`；自定义
  mock provider（CODEX_HOME/config.toml `model_providers.<name>.env_key`）只需
  环境变量有值即可零密钥（CI mock 用）；
- **节点超时**：codex 冷启动+重试可能分钟级，建议 `node_timeout_ms > 60000`
  （e2e/F5d 用 120000）；取消宽限沿用 2s→SIGKILL（DR-011/DR-020）。

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
import threading
import time
from typing import Any, Dict, IO, List, Optional, Tuple

PROTOCOL = "zgraph-harness/1"
PROTOCOL_VERSION = 1
PROVIDER = "codex_exec"
CANCEL_GRACE_SECONDS = 2.0
READ_POLL_SECONDS = 0.05
PINNED_CODEX_VERSION = "0.166.0"
TOOL_OUTPUT_PREVIEW_CHARS = 512
MAX_PREVIEW_CHARS = 512
STDERR_CAPTURE_BYTES = 4096

ACTIVE_PROCESS: Optional[subprocess.Popen[str]] = None
STOP_REQUESTED = False
DIAGNOSED_VERSION = False


class StderrCapture:
    """Codex stderr 的有界捕获（head/tail 各 4 KiB，避免管道写满与泄漏全文）。"""

    def __init__(self) -> None:
        self.head: List[str] = []
        self.tail: List[str] = []
        self.head_chars = 0
        self.tail_chars = 0
        self.total = 0
        self.lock = threading.Lock()

    def push(self, chunk: str) -> None:
        """追加一块 stderr 文本（维护 head/tail 上限）。"""
        with self.lock:
            self.total += len(chunk)
            if self.head_chars < STDERR_CAPTURE_BYTES:
                room = STDERR_CAPTURE_BYTES - self.head_chars
                piece = chunk[:room]
                self.head.append(piece)
                self.head_chars += len(piece)
            piece = chunk[-STDERR_CAPTURE_BYTES:]
            self.tail.append(piece)
            self.tail_chars += len(piece)
            while self.tail_chars > STDERR_CAPTURE_BYTES and self.tail:
                removed = self.tail.pop(0)
                self.tail_chars -= len(removed)

    def head_text(self) -> str:
        """head 文本（≤4 KiB）。"""
        return "".join(self.head)

    def tail_text(self) -> str:
        """tail 文本（≤4 KiB）。"""
        return "".join(self.tail)[-STDERR_CAPTURE_BYTES:]


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


def codex_version(binary: str) -> str:
    """执行 `codex --version` 取版本（失败/超时 → 空串；不阻断 run）。"""
    try:
        result = subprocess.run(
            [binary, "--version"],
            capture_output=True,
            text=True,
            timeout=5,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    text = (result.stdout or "").strip() or (result.stderr or "").strip()
    return text.splitlines()[0] if text else ""


def diagnose_version(binary: str) -> None:
    """把 codex 版本写到 stderr（F5d 诊断；不改变协议输出）。"""
    version = codex_version(binary)
    sys.stderr.write(
        f"zgraph codex adapter: version={version or 'unknown'} "
        f"(pinned {PINNED_CODEX_VERSION})\n"
    )
    sys.stderr.flush()
    if version and PINNED_CODEX_VERSION not in version:
        sys.stderr.write(
            "zgraph codex adapter: warn: codex version differs from pinned "
            f"{PINNED_CODEX_VERSION}; event shape may drift (re-run F5d)\n"
        )
        sys.stderr.flush()


def build_prompt(inputs: Dict[str, Any]) -> str:
    """把 agent 输入（已映射字段）序列化为 Codex prompt（确定性键序）。"""
    return json.dumps(inputs, ensure_ascii=False, sort_keys=True)


def build_command(binary: str, prompt: str, output_file: str) -> List[str]:
    """构造 `codex exec --json ... <prompt>` 命令（不经 shell）。"""
    return [
        binary,
        "exec",
        "--json",
        # v0.5/F5d：不落 session/rollout/sqlite（DR-020 #2）。
        "--ephemeral",
        "--skip-git-repo-check",
        "--output-last-message",
        output_file,
        prompt,
    ]


def validate_hello(hello: Optional[Dict[str, Any]]) -> bool:
    """校验父进程 `hello` 帧（协议/v；`limits` 若存在必须是对象）。"""
    if hello is None or hello.get("type") != "hello":
        return False
    if hello.get("protocol") != PROTOCOL or hello.get("v") != PROTOCOL_VERSION:
        return False
    limits = hello.get("limits")
    return limits is None or isinstance(limits, dict)


def _handle_signal(_signum: int, _frame: Any) -> None:
    """SIGTERM/SIGINT：置停止标志并立即向 Codex 进程组发 SIGTERM（REV-099 C3）。"""
    global STOP_REQUESTED
    STOP_REQUESTED = True
    process = ACTIVE_PROCESS
    if process is not None:
        try:
            os.killpg(process.pid, signal.SIGTERM)
        except (ProcessLookupError, PermissionError):
            pass


def install_signal_handlers() -> None:
    """安装 SIGTERM/SIGINT handler（父进程取消时先回收 Codex 子进程组）。"""
    signal.signal(signal.SIGTERM, _handle_signal)
    signal.signal(signal.SIGINT, _handle_signal)


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


def drain_stderr(stream: IO[str], capture: StderrCapture) -> None:
    """后台线程持续排空 Codex stderr（防管道写满；head/tail 有界）。"""
    try:
        for chunk in iter(lambda: stream.read(STDERR_CAPTURE_BYTES), ""):
            if not chunk:
                return
            capture.push(chunk)
    except (OSError, ValueError):
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


def _error_message(event: Dict[str, Any]) -> str:
    """从 `turn.failed`/`error` 事件提取错误文本。"""
    error = event.get("error")
    if isinstance(error, dict) and isinstance(error.get("message"), str):
        return error["message"]
    if isinstance(event.get("message"), str):
        return event["message"]
    return "codex reported a failed turn"


def parse_thread_events(lines: List[str]) -> Dict[str, Any]:
    """解析 Codex JSONL `ThreadEvent`（8 类；未知类型忽略，向前兼容）。"""
    outcome: Dict[str, Any] = {
        "content": "",
        "usage": None,
        "completed": False,
        "failed": False,
        "error": "",
        "schema_violation": False,
        # v0.5/F5d（DR-020 #3）：command_execution 的 tool_start/tool_end（按出现顺序）。
        "tool_events": [],
        # 已发过 tool_start 的 item.id（completed 不重复发 start；无 started 时补配对）。
        "tool_started": set(),
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
            # DR-020 #1：重连后成功（turn.completed）必须清除先前顶层 error 的 failed。
            outcome["completed"] = True
            outcome["usage"] = event.get("usage")
            outcome["failed"] = False
            outcome["error"] = ""
        elif kind in ("turn.failed", "error"):
            outcome["failed"] = True
            outcome["error"] = _error_message(event)
        elif kind == "item.started":
            item = event.get("item")
            if isinstance(item, dict) and item.get("type") == "command_execution":
                _record_tool_start(item, outcome)
        elif kind == "item.completed":
            item = event.get("item")
            if not isinstance(item, dict):
                continue
            if item.get("type") == "agent_message":
                text = item.get("text")
                if isinstance(text, str):
                    outcome["content"] = text
            elif item.get("type") == "command_execution":
                _record_tool_end(item, outcome)
    outcome.pop("tool_started", None)
    return outcome


def _tool_command(item: Dict[str, Any]) -> str:
    """`command_execution.command` → str（非字符串 → 空串）。"""
    command = item.get("command")
    return command if isinstance(command, str) else ""


def _record_tool_start(item: Dict[str, Any], outcome: Dict[str, Any]) -> None:
    """`command_execution` started → tool_start（无 id 则不伪造）。"""
    call_id = item.get("id")
    if not isinstance(call_id, str) or not call_id:
        return
    started = outcome["tool_started"]
    if call_id in started:
        return
    started.add(call_id)
    outcome["tool_events"].append(
        {
            "kind": "tool_start",
            "call_id": call_id,
            "tool": "command_execution",
            "args_summary": {"command": _tool_command(item)},
        }
    )


def _record_tool_end(item: Dict[str, Any], outcome: Dict[str, Any]) -> None:
    """`command_execution` completed → tool_end（ok=(exit_code==0)）；无 started 时补 start。"""
    call_id = item.get("id")
    if not isinstance(call_id, str) or not call_id:
        return
    # Codex 可能只给 completed（不同版本/中断路径）：用真实 completed 数据补配对，
    # 不编造 command/exit_code。
    _record_tool_start(item, outcome)
    exit_code = item.get("exit_code")
    ok = isinstance(exit_code, int) and exit_code == 0
    output = item.get("aggregated_output")
    output_text = output if isinstance(output, str) else ""
    outcome["tool_events"].append(
        {
            "kind": "tool_end",
            "call_id": call_id,
            "ok": ok,
            "result_summary": {
                "exit_code": exit_code,
                "status": item.get("status"),
                "output_tail": output_text[-TOOL_OUTPUT_PREVIEW_CHARS:],
            },
        }
    )


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
    """读取 Codex stdout，同时监听 stdin `cancel` 帧 / SIGTERM 标志（返回 lines, cancelled）。"""
    lines: List[str] = []
    cancelled = False
    stdin_open = True
    assert process.stdout is not None
    stream = process.stdout
    while True:
        if STOP_REQUESTED:
            cancelled = True
            terminate(process)
            break
        watched = [stream, stdin] if stdin_open else [stream]
        try:
            readable, _, _ = select.select(watched, [], [], READ_POLL_SECONDS)
        except InterruptedError:
            continue
        if stream in readable:
            line = stream.readline()
            if not line:
                break
            lines.append(line)
        if stdin_open and stdin in readable:
            frame = read_frame(stdin)
            if frame is None:
                stdin_open = False
                continue
            if frame.get("type") == "cancel":
                cancelled = True
                terminate(process)
                break
    try:
        process.wait(timeout=CANCEL_GRACE_SECONDS)
    except subprocess.TimeoutExpired:
        terminate(process)
        process.wait()
    return lines, cancelled


def _send_error(reason: str, message: str) -> None:
    """发送结构化 error 帧（`HARNESS_PLUGIN_ERROR{reason}`）。"""
    send(
        {
            "v": PROTOCOL_VERSION,
            "type": "error",
            "code": "HARNESS_PLUGIN_ERROR",
            "reason": reason,
            "message": message[:MAX_PREVIEW_CHARS],
        }
    )


def _start_codex(
    binary: str, prompt: str, output_file: str, capture: StderrCapture
) -> subprocess.Popen[str]:
    """spawn Codex（独立 session/进程组；stderr 后台排空）。`OSError` 由调用方处理。"""
    process = subprocess.Popen(
        build_command(binary, prompt, output_file),
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        start_new_session=True,
    )
    if process.stderr is not None:
        threading.Thread(
            target=drain_stderr, args=(process.stderr, capture), daemon=True
        ).start()
    return process


def execute_run(run_frame: Dict[str, Any]) -> None:
    """执行一次 Codex run 并回写 `result` / `error`（协议帧）。"""
    global ACTIVE_PROCESS
    inputs = run_frame.get("inputs")
    if not isinstance(inputs, dict):
        inputs = {}
    binary = codex_binary()
    if shutil.which(binary) is None:
        _send_error("spawn_failed", "codex binary not found (user-provided; set CODEX_BIN or PATH)")
        return
    if not DIAGNOSED_VERSION:
        diagnose_version(binary)
        globals()["DIAGNOSED_VERSION"] = True
    capture = StderrCapture()
    with tempfile.NamedTemporaryFile(prefix="zgraph-codex-", suffix=".txt") as output:
        try:
            process = _start_codex(binary, build_prompt(inputs), output.name, capture)
        except OSError as error:
            _send_error("spawn_failed", f"failed to spawn codex: {error}")
            return
        ACTIVE_PROCESS = process
        try:
            lines, cancelled = collect_events_with_cancel(process, sys.stdin)
        finally:
            ACTIVE_PROCESS = None
        if cancelled or STOP_REQUESTED:
            # 父进程正在取消：静默退出（父侧已按 Cancelled 收口），不再写 error 帧。
            return
        outcome = parse_thread_events(lines)
        if outcome["failed"]:
            _send_error("protocol", outcome["error"])
            return
        if not outcome["completed"]:
            detail = capture.tail_text() or "codex exited without turn.completed"
            _send_error("exit_nonzero", detail)
            return
        content = read_output_file(output.name) or outcome["content"]
        # v0.5/F5d：先发真实 tool 事件（若有），再发 result（协议要求事件在 result 前）。
        for event in outcome["tool_events"]:
            send({"v": PROTOCOL_VERSION, "type": "event", **event})
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
    """握手：读 `hello`、校验协议与 limits，回 `hello_ok`（失败返回 False）。"""
    hello = read_frame(sys.stdin)
    if not validate_hello(hello):
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
    install_signal_handlers()
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
            if STOP_REQUESTED:
                # v0.5/F5d（DR-020 #8）：SIGTERM 已置停止标志 → 不再阻塞等下一帧。
                return 0


if __name__ == "__main__":
    raise SystemExit(main())
