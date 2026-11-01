"""DSH ACP 到 `zgraph-harness/1` 的适配器**骨架**（v0.3/V4；best-effort）。

设计依据（DR-011 §3.2/§3.3）：可依赖入口仅 `dsh --profile acp`（ACP v1 stdio：
`initialize` / `session/new` / `session/prompt` / `session/cancel` / `session/update`）。
ACP 协议层有 `agent_message_chunk`/`agent_thought_chunk`，但 **DSH 服务端只发
committed 消息** → 一条消息一次 delta（粒度降级）。

v0.3 范围：本模块只交付**协议握手 + 结构化错误**骨架（真实 ACP transport 属
best-effort，不阻塞发布；无 DSH 环境的 release notes 标注「adapter 骨架，待真实
环境验证」）。骨架行为：

1. `hello`/`hello_ok`：声明 `capabilities.streaming = true`（committed 粒度）；
2. `run`：返回结构化 `error` 帧（`reason="protocol"`），说明 v0.3 未实现 ACP 传输；
3. `cancel`：退出（幂等）。

实现真实 ACP 时应替换 `execute_run`，并保持「只经协议帧返回四字段输出、
不写 trace/状态机」的硬约束。
"""

from __future__ import annotations

import json
import sys
from typing import Any, Dict, IO, Optional

PROTOCOL = "zgraph-harness/1"
PROTOCOL_VERSION = 1
PROVIDER = "acp"
UNSUPPORTED_MESSAGE = (
    "DSH ACP adapter skeleton (v0.3): ACP transport is best-effort and not implemented yet; "
    "see crates/zgraph-harness/PROTOCOL.md"
)


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


def validate_hello(hello: Optional[Dict[str, Any]]) -> bool:
    """校验父进程 `hello` 帧（协议/v；`limits` 若存在必须是对象）。"""
    if hello is None or hello.get("type") != "hello":
        return False
    if hello.get("protocol") != PROTOCOL or hello.get("v") != PROTOCOL_VERSION:
        return False
    limits = hello.get("limits")
    return limits is None or isinstance(limits, dict)


def handshake() -> bool:
    """握手：读 `hello`、校验协议与 limits，回 `hello_ok`（committed 消息粒度）。"""
    hello = read_frame(sys.stdin)
    if not validate_hello(hello):
        return False
    send(
        {
            "v": PROTOCOL_VERSION,
            "type": "hello_ok",
            "protocol": PROTOCOL,
            "provider": PROVIDER,
            "capabilities": {"streaming": True, "reasoning": False},
        }
    )
    return True


def execute_run(_run_frame: Dict[str, Any]) -> None:
    """v0.3 骨架：返回结构化错误（真实 ACP 传输未实现，不阻塞发布）。"""
    send(
        {
            "v": PROTOCOL_VERSION,
            "type": "error",
            "code": "HARNESS_PLUGIN_ERROR",
            "reason": "protocol",
            "message": UNSUPPORTED_MESSAGE,
        }
    )


def main() -> int:
    """协议主循环：握手 → 逐 `run` 返回结构化错误；`cancel`/EOF 退出。"""
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
