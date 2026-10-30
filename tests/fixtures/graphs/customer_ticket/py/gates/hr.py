"""AC-01 门控前置 python 节点：从 answer_generator 输出解析 ``need_human``。

图（``customer_ticket.json``）把本节点放在 gate 之前：``need_human=false`` 时
``check_output`` 内置 gate 拦截 ``human_handoff``（AC-01 期望其 Skipped）。
"""

from __future__ import annotations

import json
from typing import Any

from zgraph_worker import WorkerContext


def need_human_check(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """解析上游 agent 四字段整包输出里的 ``content``，返回 ``{"need_human": bool}``。

    入参 ``ctx`` 为 worker 协议统一签名的一部分，本函数不使用；``content`` 非 JSON /
    缺字段时保守取 ``False``（AC-01 正常路径的 fake LLM 内容含显式 ``need_human``）。
    """
    package: Any = inputs.get("answer") or {}
    content: Any = package.get("content") if isinstance(package, dict) else None
    need_human = False
    if isinstance(content, str):
        try:
            parsed: Any = json.loads(content)
        except json.JSONDecodeError:
            parsed = {}
        if isinstance(parsed, dict) and isinstance(parsed.get("need_human"), bool):
            need_human = parsed["need_human"]
    return {"need_human": need_human}
