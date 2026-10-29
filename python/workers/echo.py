"""示例 worker：回显节点输入（M0 ``zgraph_worker.echo`` 的 M3 对外示例迁移）。"""

from __future__ import annotations

from typing import Any

from zgraph_worker import WorkerContext


def run(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """回显本次节点输入，并发送一条 progress 流式事件。

    输入：
        ``inputs``：框架注入的全部已声明端口（未到达的 optional 端口为 ``None``）。
    输出：
        ``{"echo": inputs.get("message"), "inputs": <inputs 的浅拷贝>, "node_id": ctx.node_id}``。
    幂等性：
        只读输入，相同 ``inputs`` 必得相同 ``node_id`` 以外的相同输出；重复执行结果一致，
        可安全重试。
    外部副作用：
        无（不写文件、不发网络请求、不写全局状态）；仅 ``ctx.emit`` 发送一条可丢
        ``progress`` 流式事件，不属于外部副作用。
    """
    ctx.emit({"type": "progress", "percent": 100, "message": "echo"})
    return {"echo": inputs.get("message"), "inputs": dict(inputs), "node_id": ctx.node_id}
