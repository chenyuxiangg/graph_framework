"""DSH ACP 到 `zgraph-harness/1` 的适配器（v0.5/F5a；DR-020 DSH FINAL）。

调用链：zgraph 父进程（`zgraph-harness/1` NDJSON）→ 本 adapter（ACP client）
→ `dsh --profile acp`（ACP v1 JSON-RPC 2.0 NDJSON，无 Content-Length）。

硬约束（DR-011 / PROTOCOL.md）：
- **不写** trace/状态机/`llm_calls`；只经协议帧返回四字段输出。
- `hello_ok` **立即**回（ACP `initialize` 懒执行，避开 5s 握手超时）。
- 事件只映射既有 5 类：`delta`/`reasoning`/`tool_start`/`tool_end`
  （`progress` 无对应，ACP 不暴露 plan/percent；`usage_update` 仅上下文占用，
  **不能**填 input/output tokens → 不伪造 `result.usage`）。
- `stop_reason`：`end_turn`→`EndTurn`、`max_tokens`→`MaxTokens`、
  `cancelled`→`Other:cancelled`（不新增枚举）。
- 取消：父 `cancel` 帧 → ACP `session/cancel` notification → 等 prompt 返回
  `cancelled`（≤2s 宽限）→ **不发 result**、清理退出。
- 安全：DSH 是完整 coding agent（bash/fs 工具）→ **一次性 scratch cwd**
  （`session/new` 绝对路径、`mcpServers=[]`），`DSH_HOME`/`TMPDIR` 默认隔离到
  scratch；`session/request_permission` 默认 **取消并上报**（不应答会挂起），可用
  `ZGRAPH_DSH_PERMISSION=allow_once|reject_once|cancel` 显式覆盖；不写 token/key。
- DSH 二进制：`DSH_BIN` > `PATH` 中的 `dsh` > 常见安装位置；**不硬编码**为唯一路径。

环境变量（诊断/测试/部署）：
- `DSH_BIN`：dsh 可执行文件；缺省自动查找。
- `DSH_HOME`：**总是隔离**到 scratch（不继承用户 `~/.dsh` 凭据）；模型调用只需
  父进程显式提供 `DEEPSEEK_API_KEY`。
- `ZGRAPH_DSH_SCRATCH_DIR`：scratch 根（默认 `tempfile.mkdtemp`）。
- `ZGRAPH_DSH_KEEP_SCRATCH=1`：退出时保留 scratch（诊断用）。
- `ZGRAPH_DSH_PERMISSION`：`cancel`（默认）| `reject_once` | `allow_once`。
- `ZGRAPH_DSH_INIT_TIMEOUT_MS`：initialize/session/new 超时（默认 30000）。
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
from typing import Any, Dict, Optional

PROTOCOL = "zgraph-harness/1"
PROTOCOL_VERSION = 1
PROVIDER = "acp"
ACP_PROTOCOL_VERSION = 1
CLIENT_INFO = {"name": "zgraph-harness-dsh-acp", "version": "0.5.0"}
CLIENT_CAPABILITIES = {
    "fs": {"readTextFile": False, "writeTextFile": False},
    "terminal": False,
}
CANCEL_GRACE_SECONDS = 2.0
READ_POLL_SECONDS = 0.05
DEFAULT_INIT_TIMEOUT_MS = 30_000
DEFAULT_PROMPT_TIMEOUT_MS = 300_000
MAX_RESULT_CHARS = 1_048_576
MAX_EVENTS = 20_000
MAX_INVALID_LINES = 2
MAX_TOOL_SUMMARY_CHARS = 512
STDERR_CAPTURE_BYTES = 4096

DSH_COMMON_PATHS = (
    "/Applications/DeepSeek Harness.app/Contents/Resources/runtime/cli/bin/dsh",
    "~/.local/bin/dsh",
    "/usr/local/bin/dsh",
    "/opt/homebrew/bin/dsh",
)

ACTIVE_PROCESS: Optional[subprocess.Popen[str]] = None


class AcpError(Exception):
    """ACP/传输错误（映射为 `HARNESS_PLUGIN_ERROR{reason}`）。"""

    def __init__(self, reason: str, message: str) -> None:
        super().__init__(message)
        self.reason = reason
        self.message = message


class AdapterCancelled(Exception):
    """父进程取消（不发 result，清理后退出）。"""


def send(payload: Dict[str, Any]) -> None:
    """向父进程写一帧 NDJSON（stdout）。"""
    sys.stdout.write(json.dumps(payload, ensure_ascii=False) + "\n")
    sys.stdout.flush()


def log(message: str) -> None:
    """诊断日志走 stderr（不污染 stdout 协议流；不含 token/key）。"""
    sys.stderr.write(f"[dsh_acp] {message}\n")
    sys.stderr.flush()


class ParentChannel:
    """父进程 stdin 的**无 read-ahead** 行通道（select + os.read）。

    TextIOWrapper 的预读与 `select` 混用会漏掉已缓冲的 `cancel` 帧，因此这里直接
    用 fd 读并自行维护 buffer（主循环阻塞读、prompt 期间 select 轮询 cancel）。
    """

    def __init__(self, fd: int = 0, buffer: str = "") -> None:
        self.fd = fd
        self.buffer = buffer
        self.eof = False
        self.deferred: list[Dict[str, Any]] = []

    def fileno(self) -> int:
        """供 select 使用。"""
        return self.fd

    @staticmethod
    def _parse(line: str) -> Optional[Dict[str, Any]]:
        """解析一帧（非法 JSON/非对象 → None）。"""
        try:
            frame = json.loads(line)
        except json.JSONDecodeError:
            return None
        return frame if isinstance(frame, dict) else None

    def _read_more(self) -> bool:
        """读一块数据；EOF 返回 False。"""
        chunk = os.read(self.fd, 65536)
        if not chunk:
            self.eof = True
            return False
        self.buffer += chunk.decode("utf-8", errors="replace")
        return True

    def read_blocking(self) -> Optional[Dict[str, Any]]:
        """阻塞读一帧；EOF 返回 None。"""
        while True:
            message, eof, had_line = self.pop_frame()
            if eof:
                return None
            if had_line:
                if message is not None:
                    return message
                continue
            if not self.read_more():
                return None

    def read_more(self) -> bool:
        """读一块到 buffer；EOF → False。"""
        return self._read_more()

    def defer(self, frame: Dict[str, Any]) -> None:
        """prompt 期间读到的非 cancel 帧暂存，交给主循环稍后处理（不丢帧）。"""
        self.deferred.append(frame)

    def pop_frame(self) -> tuple[Optional[Dict[str, Any]], bool, bool]:
        """弹出一帧（含 prompt 期间暂存的帧）：返回（frame, eof, 是否消费了一行）。"""
        if self.deferred:
            return (self.deferred.pop(0), False, True)
        return self.pop_live_frame()

    def pop_live_frame(self) -> tuple[Optional[Dict[str, Any]], bool, bool]:
        """只从 stdin buffer 弹出新帧（不碰 deferred；prompt 轮询用）。"""
        if "\n" not in self.buffer:
            return (None, self.eof, False)
        line, self.buffer = self.buffer.split("\n", 1)
        line = line.strip()
        if not line:
            return (None, False, True)
        return (self._parse(line), False, True)


class AcpChannel:
    """DSH stdout 的无 read-ahead 行通道（select + os.read；NDJSON framed）。"""

    def __init__(self, fd: int) -> None:
        self.fd = fd
        self.buffer = b""
        self.eof = False

    def fileno(self) -> int:
        """供 select 使用。"""
        return self.fd

    def _read_more(self) -> bool:
        """读一块数据；EOF → False。"""
        chunk = os.read(self.fd, 65536)
        if not chunk:
            self.eof = True
            return False
        self.buffer += chunk
        return True

    def read_more(self) -> bool:
        """读一块到 buffer；EOF → False。"""
        return self._read_more()

    def pop_frame(self) -> tuple[Optional[Dict[str, Any]], bool, bool]:
        """从 buffer 弹出一帧：返回（frame, eof, 是否消费了一行）。"""
        if b"\n" not in self.buffer:
            return (None, self.eof, False)
        raw, self.buffer = self.buffer.split(b"\n", 1)
        line = raw.decode("utf-8", errors="replace").strip()
        if not line:
            return (None, False, True)
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            return (None, False, True)
        return (message if isinstance(message, dict) else None, False, True)


class Scratch:
    """一次性 scratch（DSH_HOME/TMPDIR/workspace；默认退出清理）。"""

    def __init__(self, root: Optional[str] = None) -> None:
        override = (root or os.environ.get("ZGRAPH_DSH_SCRATCH_DIR", "")).strip()
        self.root = override or tempfile.mkdtemp(prefix="zgraph-dsh-")
        os.makedirs(self.root, exist_ok=True)
        self.dsh_home = os.path.join(self.root, "dsh-home")
        self.tmpdir = os.path.join(self.root, "tmp")
        self.workspaces = os.path.join(self.root, "workspaces")
        for path in (self.dsh_home, self.tmpdir, self.workspaces):
            os.makedirs(path, exist_ok=True)
        self.session_counter = 0
        self.created = override == ""

    def new_workspace(self) -> str:
        """为一次 `session/new` 创建一次性绝对 cwd 目录。"""
        self.session_counter += 1
        path = os.path.join(self.workspaces, f"session-{self.session_counter}")
        os.makedirs(path, exist_ok=True)
        return os.path.abspath(path)

    def cleanup(self) -> None:
        """清理 scratch（`ZGRAPH_DSH_KEEP_SCRATCH=1` 或显式目录时保留）。"""
        if self.created and os.environ.get("ZGRAPH_DSH_KEEP_SCRATCH") != "1":
            shutil.rmtree(self.root, ignore_errors=True)


def env_for_dsh(scratch: Scratch) -> Dict[str, str]:
    """构造 DSH 子进程环境：**强制隔离** DSH_HOME/TMPDIR 到 scratch。

    不继承用户 `~/.dsh`（凭据/会话/状态）；需要模型调用时由父进程显式提供
    `DEEPSEEK_API_KEY`（DR-020：不使用用户凭据）。
    """
    env = dict(os.environ)
    # 安全：不继承用户 ~/.dsh 凭据/状态；DSH_HOME 强制隔离到 scratch。
    env["DSH_HOME"] = scratch.dsh_home
    env["TMPDIR"] = scratch.tmpdir
    return env


class StderrCapture:
    """DSH stderr 有界捕获（head/tail 各 4 KiB，防管道写满/泄漏全文）。"""

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.head = ""
        self.tail = ""
        self.total = 0

    def push(self, chunk: str) -> None:
        """追加一块 stderr 文本（维护 head/tail 上限）。"""
        with self.lock:
            self.total += len(chunk)
            if len(self.head) < STDERR_CAPTURE_BYTES:
                room = STDERR_CAPTURE_BYTES - len(self.head)
                self.head += chunk[:room]
            self.tail = (self.tail + chunk)[-STDERR_CAPTURE_BYTES:]

    def tail_text(self) -> str:
        """tail 文本（≤4 KiB）。"""
        with self.lock:
            return self.tail


def validate_hello(hello: Optional[Dict[str, Any]]) -> bool:
    """校验父 `hello` 帧（协议/v；`limits` 若存在必须是对象）。"""
    if hello is None or hello.get("type") != "hello":
        return False
    if hello.get("protocol") != PROTOCOL or hello.get("v") != PROTOCOL_VERSION:
        return False
    limits = hello.get("limits")
    return limits is None or isinstance(limits, dict)


def hello_limits(hello: Dict[str, Any]) -> Dict[str, Any]:
    """取 `hello.limits`（缺失/非法 → 空 dict）。"""
    limits = hello.get("limits")
    return limits if isinstance(limits, dict) else {}


def node_timeout_ms(limits: Dict[str, Any]) -> int:
    """本次实例超时（毫秒；缺失/非法回退 300s）。"""
    value = limits.get("node_timeout_ms")
    if isinstance(value, int) and value > 0:
        return int(value)
    return DEFAULT_PROMPT_TIMEOUT_MS


def dsh_binary() -> str:
    """返回 dsh 可执行文件路径（`DSH_BIN` → PATH → 常见安装；仍缺失时返回原值用于报错）。"""
    configured = os.environ.get("DSH_BIN", "").strip()
    if configured:
        resolved = shutil.which(configured) if os.path.sep not in configured else configured
        return resolved or configured
    for name in ("dsh", "dsh-cli"):
        resolved = shutil.which(name)
        if resolved:
            return resolved
    for candidate in DSH_COMMON_PATHS:
        expanded = os.path.expanduser(candidate)
        if os.path.isfile(expanded) and os.access(expanded, os.X_OK):
            return expanded
    return "dsh"


def permission_policy() -> str:
    """权限默认策略（`cancel` 默认；避免静默放行 DSH 工具副作用）。"""
    value = os.environ.get("ZGRAPH_DSH_PERMISSION", "cancel").strip().lower()
    if value in ("allow_once", "reject_once", "cancel"):
        return value
    return "cancel"


def extract_text(value: Any) -> str:
    """从 ACP content 结构提取文本（object/array 常见形状；否则空串）。"""
    if isinstance(value, dict):
        kind = value.get("type")
        if kind == "text" and isinstance(value.get("text"), str):
            return value["text"]
        nested = value.get("content")
        if nested is not None:
            return extract_text(nested)
        return ""
    if isinstance(value, list):
        return "".join(extract_text(item) for item in value)
    if isinstance(value, str):
        return value
    return ""


def summarize_tool_content(value: Any) -> Dict[str, Any]:
    """tool_end `result_summary`：文本（≤512 chars）或结构化摘要（无全文）。"""
    text = extract_text(value)
    if text:
        return {"text": text[:MAX_TOOL_SUMMARY_CHARS]}
    if value is None:
        return {}
    return {"kind": type(value).__name__}


def args_summary(raw_input: Any) -> Dict[str, Any]:
    """tool_start `args_summary`：仅保留对象形状（不展开大对象）。"""
    if isinstance(raw_input, dict):
        return raw_input
    if raw_input is None:
        return {}
    return {"value": str(raw_input)[:MAX_TOOL_SUMMARY_CHARS]}


def map_stop_reason(raw: str) -> str:
    """ACP stopReason → zgraph 既有字面量（不新增枚举）。"""
    mapping = {
        "end_turn": "EndTurn",
        "max_tokens": "MaxTokens",
        "cancelled": "Other:cancelled",
        "refusal": "Other:refusal",
        "error": "Other:error",
    }
    return mapping.get(raw, f"Other:{raw}")


def choose_permission(policy: str, options: Any) -> Dict[str, Any]:
    """权限应答（默认 cancel；显式 allow/reject 时按 option kind 选择）。"""
    entries = options if isinstance(options, list) else []
    if policy != "cancel":
        wanted = "allow_once" if policy == "allow_once" else "reject_once"
        for option in entries:
            if not isinstance(option, dict):
                continue
            kind = option.get("kind")
            if kind == wanted or (isinstance(kind, str) and kind.startswith(wanted.split("_")[0])):
                option_id = option.get("optionId")
                if isinstance(option_id, str):
                    return {"outcome": {"outcome": "selected", "optionId": option_id}}
    return {"outcome": {"outcome": "cancelled"}}


class AcpClient:
    """最小 ACP v1 JSON-RPC 2.0 NDJSON 客户端（stdio；懒启动）。"""

    def __init__(self, scratch: Scratch) -> None:
        self.scratch = scratch
        self.binary = dsh_binary()
        self.env = env_for_dsh(scratch)
        self.proc: Optional[subprocess.Popen[str]] = None
        self.stderr = StderrCapture()
        self.channel: Optional[AcpChannel] = None
        self.next_id = 1
        self.cancel_requested = False
        self.initialized = False
        self.agent_info: Dict[str, Any] = {}

    def start(self) -> None:
        """启动 `dsh --profile acp`（stdin/stdout/stderr 全部由本 adapter 持有）。"""
        global ACTIVE_PROCESS
        try:
            self.proc = subprocess.Popen(
                [self.binary, "--profile", "acp"],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                bufsize=1,
                cwd=self.scratch.root,
                env=self.env,
                start_new_session=True,
            )
        except OSError as error:
            raise AcpError("spawn_failed", f"failed to start dsh: {error}") from error
        ACTIVE_PROCESS = self.proc
        assert self.proc.stdout is not None
        self.channel = AcpChannel(self.proc.stdout.fileno())
        thread = threading.Thread(target=self._drain_stderr, daemon=True)
        thread.start()

    def _drain_stderr(self) -> None:
        """持续排空 DSH stderr（有界捕获，防管道写满死锁）。"""
        proc = self.proc
        if proc is None or proc.stderr is None:
            return
        while True:
            chunk = proc.stderr.read(4096)
            if not chunk:
                return
            self.stderr.push(chunk)

    def send_message(self, message: Dict[str, Any]) -> None:
        """写一条 JSON-RPC 消息（NDJSON）。"""
        if self.proc is None or self.proc.stdin is None:
            raise AcpError("protocol", "dsh process is not running")
        try:
            self.proc.stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
            self.proc.stdin.flush()
        except (BrokenPipeError, OSError) as error:
            raise AcpError("exit_nonzero", f"dsh stdin closed: {error}") from error

    def request(self, method: str, params: Dict[str, Any], timeout: float) -> Dict[str, Any]:
        """发送 request 并等待同 id 响应（期间应答 permission/忽略 update）。"""
        request_id = self.next_id
        self.next_id += 1
        self.send_message({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + timeout
        invalid = 0
        while True:
            message, eof, had_line = self._stdout().pop_frame()
            if eof:
                raise AcpError("exit_nonzero", self._exit_message(f"waiting for '{method}'"))
            if had_line:
                if message is None:
                    invalid += 1
                    if invalid >= MAX_INVALID_LINES:
                        raise AcpError("protocol", "too many invalid JSON lines from dsh")
                    continue
                if message.get("id") == request_id:
                    if "error" in message:
                        raise AcpError("protocol", self._rpc_error_text(method, message["error"]))
                    result = message.get("result")
                    return result if isinstance(result, dict) else {}
                self._dispatch_server_message(message)
                continue
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise AcpError("timeout", f"ACP method '{method}' timed out")
            ready, _, _ = select.select([self._stdout()], [], [], min(remaining, READ_POLL_SECONDS))
            if ready:
                self._stdout().read_more()

    def notify(self, method: str, params: Dict[str, Any]) -> None:
        """发送 JSON-RPC notification（无 id）。"""
        self.send_message({"jsonrpc": "2.0", "method": method, "params": params})

    def initialize(self, timeout: float) -> None:
        """`initialize`（懒执行；断言 protocolVersion=1）。"""
        if self.initialized:
            return
        if self.proc is None:
            self.start()
        result = self.request(
            "initialize",
            {
                "protocolVersion": ACP_PROTOCOL_VERSION,
                "clientCapabilities": CLIENT_CAPABILITIES,
                "clientInfo": CLIENT_INFO,
            },
            timeout,
        )
        version = result.get("protocolVersion")
        if version != ACP_PROTOCOL_VERSION:
            raise AcpError(
                "version",
                f"unsupported ACP protocolVersion {version!r} (expected {ACP_PROTOCOL_VERSION})",
            )
        self.agent_info = result.get("agentInfo") if isinstance(result.get("agentInfo"), dict) else {}
        auth_methods = result.get("authMethods")
        log(f"initialize ok protocolVersion={version} authMethods={auth_methods!r}")
        self.initialized = True

    def new_session(self, cwd: str, timeout: float) -> str:
        """`session/new`（绝对 cwd、mcpServers=[]）→ sessionId。"""
        result = self.request(
            "session/new", {"cwd": os.path.abspath(cwd), "mcpServers": []}, timeout
        )
        session_id = result.get("sessionId")
        if not isinstance(session_id, str) or not session_id:
            raise AcpError("protocol", "session/new result has no string sessionId")
        log(f"session/new ok sessionId={session_id}")
        return session_id

    def close_session(self, session_id: str, timeout: float) -> None:
        """`session/close`（best-effort；错误仅记 stderr）。"""
        try:
            self.request("session/close", {"sessionId": session_id}, timeout)
        except AcpError as error:
            log(f"session/close ignored: {error.reason}: {error.message}")

    def prompt(
        self,
        session_id: str,
        text: str,
        timeout: float,
        on_update: Any,
        parent_stream: Optional["ParentChannel"] = None,
    ) -> Dict[str, Any]:
        """`session/prompt`：转发 updates、应答 permission、处理父 cancel；返回 result。"""
        request_id = self._send_prompt_request(session_id, text)
        state = {"cancel_sent": False, "cancel_deadline": None, "invalid": 0}
        deadline = time.monotonic() + timeout
        while True:
            self._check_prompt_deadlines(deadline, state)
            message = self._pump_prompt_message(deadline, parent_stream, session_id, state)
            resolved = self._resolve_prompt_message(request_id, message, on_update)
            if resolved is not None:
                if self.cancel_requested:
                    raise AdapterCancelled()
                return resolved

    def _send_prompt_request(self, session_id: str, text: str) -> int:
        """发送 `session/prompt` request，返回 id。"""
        self.cancel_requested = False
        request_id = self.next_id
        self.next_id += 1
        self.send_message(
            {
                "jsonrpc": "2.0",
                "id": request_id,
                "method": "session/prompt",
                "params": {"sessionId": session_id, "prompt": [{"type": "text", "text": text}]},
            }
        )
        return request_id

    @staticmethod
    def _check_prompt_deadlines(deadline: float, state: Dict[str, Any]) -> None:
        """取消宽限/整体超时检查。"""
        now = time.monotonic()
        cancel_deadline = state.get("cancel_deadline")
        if cancel_deadline is not None and now >= cancel_deadline:
            raise AdapterCancelled()
        if now >= deadline:
            raise AcpError("timeout", "session/prompt timed out")

    def _pump_prompt_message(
        self,
        deadline: float,
        parent_stream: Optional["ParentChannel"],
        session_id: str,
        state: Dict[str, Any],
    ) -> Dict[str, Any]:
        """等下一帧（parent cancel 或 ACP 输出），返回 ACP 消息（无递归）。"""
        while True:
            self._check_prompt_deadlines(deadline, state)
            if parent_stream is not None and not state["cancel_sent"]:
                parent_frame, parent_eof, parent_line = parent_stream.pop_live_frame()
                if parent_eof or (parent_line and (parent_frame or {}).get("type") == "cancel"):
                    self._handle_parent_cancel(parent_stream, session_id, state)
                    continue
                if parent_line and parent_frame is not None:
                    parent_stream.defer(parent_frame)
                    continue
            message, eof, had_line = self._stdout().pop_frame()
            if eof:
                if state["cancel_sent"]:
                    raise AdapterCancelled()
                raise AcpError("exit_nonzero", self._exit_message("session/prompt"))
            if had_line:
                if message is None:
                    state["invalid"] += 1
                    if state["invalid"] >= MAX_INVALID_LINES:
                        raise AcpError("protocol", "too many invalid JSON lines from dsh")
                    continue
                return message
            streams = [self._stdout()]
            if parent_stream is not None:
                streams.append(parent_stream)
            remaining = max(0.0, deadline - time.monotonic())
            ready, _, _ = select.select(streams, [], [], min(READ_POLL_SECONDS, remaining))
            if parent_stream is not None and parent_stream in ready:
                parent_stream.read_more()
            if self._stdout() in ready:
                self._stdout().read_more()

    def _handle_parent_cancel(
        self,
        _parent_stream: "ParentChannel",
        session_id: str,
        state: Dict[str, Any],
    ) -> None:
        """父 `cancel`/EOF → 发一次 ACP `session/cancel`，进入 2s 宽限。"""
        self.cancel_requested = True
        if not state["cancel_sent"]:
            self.notify("session/cancel", {"sessionId": session_id})
            state["cancel_sent"] = True
            log("session/cancel sent")
            state["cancel_deadline"] = time.monotonic() + CANCEL_GRACE_SECONDS

    def _resolve_prompt_message(
        self,
        request_id: int,
        message: Dict[str, Any],
        on_update: Any,
    ) -> Optional[Dict[str, Any]]:
        """prompt 响应 → result；其它消息分派后返回 None。"""
        if "id" in message and message.get("id") == request_id:
            if "error" in message:
                raise AcpError("protocol", self._rpc_error_text("session/prompt", message["error"]))
            result = message.get("result")
            return result if isinstance(result, dict) else {}
        self._dispatch_server_message(message, on_update=on_update)
        return None

    def _dispatch_server_message(
        self, message: Dict[str, Any], on_update: Any = None
    ) -> None:
        """处理服务端 notification/request：update 映射或 permission 应答。"""
        method = message.get("method")
        if method == "session/update":
            params = message.get("params")
            update = params.get("update") if isinstance(params, dict) else None
            if isinstance(update, dict) and on_update is not None:
                on_update(update)
            return
        if method == "session/request_permission":
            self._answer_permission(message)
            return
        if "id" in message and "method" in message:
            self.send_message(
                {
                    "jsonrpc": "2.0",
                    "id": message.get("id"),
                    "error": {"code": -32601, "message": f"unsupported method {method}"},
                }
            )

    def _answer_permission(self, message: Dict[str, Any]) -> None:
        """应答 `session/request_permission`（默认 cancel，避免工具悬挂）。"""
        params = message.get("params")
        options = params.get("options") if isinstance(params, dict) else None
        policy = permission_policy()
        outcome = choose_permission(policy, options)
        tool_call = params.get("toolCall") if isinstance(params, dict) else None
        tool_id = tool_call.get("toolCallId") if isinstance(tool_call, dict) else None
        log(f"request_permission toolCallId={tool_id!r} policy={policy} -> {outcome['outcome']}")
        self.send_message({"jsonrpc": "2.0", "id": message.get("id"), "result": outcome})

    def _stdout(self) -> AcpChannel:
        """DSH stdout 通道（必须已启动；无 read-ahead 以便 select 轮询）。"""
        if self.channel is None:
            raise AcpError("protocol", "dsh process is not running")
        return self.channel

    @staticmethod
    def _rpc_error_text(method: str, error: Any) -> str:
        """JSON-RPC error → 可读文本（避免把超大错误体塞进帧）。"""
        if isinstance(error, dict):
            code = error.get("code")
            message = error.get("message")
            text = f"ACP {method} failed: code={code} message={message}"
        else:
            text = f"ACP {method} failed: {error!r}"
        return text[:2048]

    def _exit_message(self, context: str) -> str:
        """子进程退出诊断（含 stderr tail 摘要）。"""
        tail = self.stderr.tail_text().strip()
        suffix = f"; stderr={tail[-1024:]}" if tail else ""
        return f"dsh exited while {context}{suffix}"

    def shutdown(self) -> None:
        """关闭 DSH 子进程（stdin EOF → 宽限 → TERM → KILL 进程组）。"""
        global ACTIVE_PROCESS
        proc = self.proc
        self.proc = None
        ACTIVE_PROCESS = None
        if proc is None:
            return
        try:
            if proc.stdin is not None:
                proc.stdin.close()
        except OSError:
            pass
        try:
            proc.wait(timeout=CANCEL_GRACE_SECONDS)
        except subprocess.TimeoutExpired:
            self._signal_group(proc, signal.SIGTERM)
            try:
                proc.wait(timeout=1.0)
            except subprocess.TimeoutExpired:
                self._signal_group(proc, signal.SIGKILL)
                proc.wait(timeout=1.0)
        except OSError:
            pass
        self._signal_group(proc, signal.SIGKILL)

    @staticmethod
    def _signal_group(proc: subprocess.Popen[str], sig: int) -> None:
        """向 DSH 进程组发信号（start_new_session=True；非 Unix 回退单进程）。"""
        try:
            if hasattr(os, "killpg"):
                os.killpg(proc.pid, sig)
            else:
                proc.send_signal(sig)
        except (ProcessLookupError, PermissionError, OSError):
            pass


class AcpAdapter:
    """`zgraph-harness/1` ↔ ACP 桥（session 复用/reset、事件映射、result/error）。"""

    def __init__(self, client: AcpClient, scratch: Scratch) -> None:
        self.client = client
        self.scratch = scratch
        self.session_id: Optional[str] = None
        self.session_workspace: Optional[str] = None
        self.events_sent = 0
        self.content = ""
        self.prompt_timeout_ms = DEFAULT_PROMPT_TIMEOUT_MS
        self.channel: Optional["ParentChannel"] = None

    def configure_limits(self, limits: Dict[str, Any], channel: "ParentChannel") -> None:
        """应用 `hello.limits`（node_timeout_ms → prompt/init 超时）与父通道。"""
        self.prompt_timeout_ms = node_timeout_ms(limits)
        self.channel = channel

    def execute_run(self, frame: Dict[str, Any]) -> None:
        """处理一个 `run` 帧：建/复用 session → prompt → result/error。"""
        self.events_sent = 0
        self.content = ""
        try:
            self._ensure_session(frame)
            prompt_text = json.dumps(frame.get("inputs") or {}, ensure_ascii=False, sort_keys=True)
            result = self.client.prompt(
                self.session_id or "",
                prompt_text,
                self._prompt_timeout_seconds(),
                self._on_update,
                parent_stream=self.channel,
            )
        except AdapterCancelled:
            self._close_session_best_effort()
            raise
        except AcpError as error:
            self._send_error(error.reason, error.message)
            return
        stop_reason = result.get("stopReason")
        stop_literal = map_stop_reason(stop_reason if isinstance(stop_reason, str) else "end_turn")
        send(
            {
                "v": PROTOCOL_VERSION,
                "type": "result",
                "content": self._bounded_content(),
                "iterations": 1,
                "stop_reason": stop_literal,
            }
        )

    def _ensure_session(self, frame: Dict[str, Any]) -> None:
        """按 `session.reset/reuse_key` 建/复用 ACP session（懒 initialize）。"""
        session = frame.get("session") if isinstance(frame.get("session"), dict) else {}
        reset = bool(session.get("reset", False))
        init_timeout = self._init_timeout_seconds()
        self.client.initialize(init_timeout)
        if self.session_id is not None and reset:
            old = self.session_id
            self.session_id = None
            self.client.close_session(old, min(init_timeout, 2.0))
        if self.session_id is None:
            self.session_workspace = self.scratch.new_workspace()
            self.session_id = self.client.new_session(self.session_workspace, init_timeout)

    def _init_timeout_seconds(self) -> float:
        """initialize/session/* 超时（env 可调；默认 30s）。"""
        raw = os.environ.get("ZGRAPH_DSH_INIT_TIMEOUT_MS", "").strip()
        try:
            value = int(raw)
        except ValueError:
            value = DEFAULT_INIT_TIMEOUT_MS
        return max(1.0, min(value, 120_000) / 1000.0)

    def _prompt_timeout_seconds(self) -> float:
        """prompt 等待上限（node_timeout_ms - 1s 余量，至少 1s）。"""
        return max(1.0, self.prompt_timeout_ms / 1000.0 - 1.0)

    def _on_update(self, update: Dict[str, Any]) -> None:
        """ACP `session/update` → zgraph 事件/正文（未知类型忽略）。"""
        kind = update.get("sessionUpdate")
        handlers = {
            "agent_message_chunk": self._on_message_chunk,
            "agent_thought_chunk": self._on_thought_chunk,
            "tool_call": self._on_tool_call,
            "tool_call_update": self._on_tool_call_update,
            "usage_update": self._on_usage_update,
        }
        handler = handlers.get(kind)
        if handler is not None:
            handler(update)

    def _on_message_chunk(self, update: Dict[str, Any]) -> None:
        """assistant 文本 chunk → delta + 累积最终 content。"""
        text = extract_text(update.get("content"))
        if text:
            self._append_content(text)
            self._emit({"type": "event", "kind": "delta", "text": text})

    def _on_thought_chunk(self, update: Dict[str, Any]) -> None:
        """reasoning chunk → reasoning（不计入最终 content）。"""
        text = extract_text(update.get("content"))
        if text:
            self._emit({"type": "event", "kind": "reasoning", "text": text})

    def _on_tool_call(self, update: Dict[str, Any]) -> None:
        """工具开始 → tool_start（call_id/tool/args_summary）。"""
        self._emit(
            {
                "type": "event",
                "kind": "tool_start",
                "call_id": str(update.get("toolCallId") or ""),
                "tool": str(update.get("title") or update.get("kind") or "tool"),
                "args_summary": args_summary(update.get("rawInput")),
            }
        )

    def _on_tool_call_update(self, update: Dict[str, Any]) -> None:
        """工具结束 → tool_end（ok = status=="completed"；不伪造成功）。"""
        status = update.get("status")
        self._emit(
            {
                "type": "event",
                "kind": "tool_end",
                "call_id": str(update.get("toolCallId") or ""),
                "ok": status == "completed",
                "result_summary": summarize_tool_content(update.get("content")),
            }
        )

    def _on_usage_update(self, _update: Dict[str, Any]) -> None:
        """上下文占用（used/size）不是 input/output token → 忽略，不伪造 usage。"""

    def _emit(self, frame: Dict[str, Any]) -> None:
        """发事件帧（事件计数上限；超出 → event_limit 错误）。"""
        if self.events_sent >= MAX_EVENTS:
            raise AcpError("event_limit", f"external harness emitted more than {MAX_EVENTS} events")
        self.events_sent += 1
        payload = {"v": PROTOCOL_VERSION}
        payload.update(frame)
        send(payload)

    def _append_content(self, text: str) -> None:
        """累积最终 content（≤1 MiB；超出截断并标记）。"""
        if len(self.content) >= MAX_RESULT_CHARS:
            return
        room = MAX_RESULT_CHARS - len(self.content)
        piece = text[:room]
        self.content += piece
        if len(piece) < len(text):
            self.content = self.content[: MAX_RESULT_CHARS - 12] + "…[truncated]"

    def _bounded_content(self) -> str:
        """返回最终 content（防御性 ≤1 MiB）。"""
        if len(self.content) <= MAX_RESULT_CHARS:
            return self.content
        return self.content[: MAX_RESULT_CHARS - 12] + "…[truncated]"

    def _send_error(self, reason: str, message: str) -> None:
        """结构化错误帧（plugin reason 闭集；message 截断）。"""
        send(
            {
                "v": PROTOCOL_VERSION,
                "type": "error",
                "code": "HARNESS_PLUGIN_ERROR",
                "reason": reason,
                "message": message[:2048],
            }
        )

    def _close_session_best_effort(self) -> None:
        """取消/退出时关闭 ACP session（最多 1s，不阻塞退出）。"""
        if self.session_id is None:
            return
        session_id = self.session_id
        self.session_id = None
        self.client.close_session(session_id, 1.0)

    def shutdown(self) -> None:
        """关闭 session + DSH 进程 + scratch。"""
        try:
            self._close_session_best_effort()
        finally:
            self.client.shutdown()
            self.scratch.cleanup()


def install_signal_handlers(adapter: AcpAdapter) -> None:
    """SIGTERM/SIGINT：kill DSH 进程组后退出（父进程取消不走 SIGKILL 残留）。"""

    def handle(_signum: int, _frame: Any) -> None:
        proc = adapter.client.proc
        if proc is not None:
            AcpClient._signal_group(proc, signal.SIGKILL)
        adapter.scratch.cleanup()
        os._exit(0)

    for sig in (signal.SIGTERM, signal.SIGINT):
        try:
            signal.signal(sig, handle)
        except (ValueError, OSError):
            pass


def main() -> int:
    """协议主循环：hello 立即回；run 驱动 ACP prompt；EOF/cancel 清理退出。"""
    channel = ParentChannel()
    hello = channel.read_blocking()
    if not validate_hello(hello):
        return 0
    limits = hello_limits(hello or {})
    send(
        {
            "v": PROTOCOL_VERSION,
            "type": "hello_ok",
            "protocol": PROTOCOL,
            "provider": PROVIDER,
            "capabilities": {"streaming": True, "reasoning": True},
        }
    )
    scratch = Scratch()
    adapter = AcpAdapter(AcpClient(scratch), scratch)
    adapter.configure_limits(limits, channel)
    install_signal_handlers(adapter)
    try:
        while True:
            frame = channel.read_blocking()
            if frame is None:
                return 0
            kind = frame.get("type")
            if kind == "cancel":
                return 0
            if kind != "run":
                continue
            try:
                adapter.execute_run(frame)
            except AdapterCancelled:
                return 0
    finally:
        adapter.shutdown()


if __name__ == "__main__":
    raise SystemExit(main())
