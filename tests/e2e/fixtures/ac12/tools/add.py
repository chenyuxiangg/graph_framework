"""AC-12 工具 fixture：``add`` 求和 + 记录工具侧真实进程区间。

AC-08 的同名工具（``tests/fixtures/graphs/harness_ac08/tools/add.py``）只返回求和；
本副本**仅**增加进程区间日志，用于 AC-12 证明「工具调用与 python 节点共享同一份
全局/每 run worker 上限」（ISSUE-079 / REV-052 / DR-003 D7）：

- 日志行格式与 ``e2e_fixtures.ac12`` 的节点区间一致：
  ``<run_tag> tool <pid> <wall-clock> start|end``；与节点区间写同一日志文件，测试
  按 run_tag 分组求峰值 → 「工具 + 节点」合计并发才算数；
- 工具 worker 的 permit 在「进程启动 + 调用」期间持有；这里把调用窗口拉宽到约
  200ms（``TOOL_SLEEP_SECONDS``），使叠加峰值能被区间日志观测；
- run_tag / 日志路径由测试通过子进程环境变量注入（工具本身不感知 run 结构）。
"""

from __future__ import annotations

import os
import time
from typing import Any

TOOL: dict[str, Any] = {
    "name": "add",
    "description": "把两个数字相加，返回 {\"sum\": a+b}",
    "parameters": {
        "type": "object",
        "properties": {
            "a": {"type": "number"},
            "b": {"type": "number"},
        },
        "required": ["a", "b"],
        "additionalProperties": False,
    },
}

RUN_TAG_ENV: str = "ZGRAPH_AC12_RUN_TAG"
LOG_FILE_ENV: str = "ZGRAPH_AC12_TOOL_LOG"
TOOL_SLEEP_SECONDS: float = 0.2


def call(args: dict[str, Any]) -> dict[str, Any]:
    """记录工具进程区间（窗口约 200ms）后返回 ``{"sum": a+b}``。"""
    log_file: str | None = os.environ.get(LOG_FILE_ENV)
    run_tag: str = os.environ.get(RUN_TAG_ENV, "run")
    _append(log_file, f"{run_tag} tool {os.getpid()} {time.time():.6f} start")
    time.sleep(TOOL_SLEEP_SECONDS)
    _append(log_file, f"{run_tag} tool {os.getpid()} {time.time():.6f} end")
    return {"sum": args["a"] + args["b"]}


def _append(log_file: str | None, line: str) -> None:
    """向共享区间日志追加一行（未注入日志路径时静默跳过）。"""
    if not log_file:
        return
    with open(log_file, "a", encoding="utf-8") as handle:
        handle.write(line + "\n")
