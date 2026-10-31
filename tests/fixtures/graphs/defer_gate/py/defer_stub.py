"""W2a e2e fixture worker：defer 自环/回边/死锁图的纯函数 python_node。

模块名 ``defer_stub``（由 codegen/手写 fixture 的 ``config.module`` 引用）；
e2e 通过 ``--python-path tests/fixtures/graphs/defer_gate/py`` 提供。

所有函数都是**纯函数**：只读 ``inputs``、无随机/时间/外部副作用，相同输入输出相同
（可安全重试，也不依赖 worker 进程跨实例状态）。
"""

from __future__ import annotations

from typing import Any

from zgraph_worker import WorkerContext

FIRST_ROUND: int = 1
SECOND_ROUND: int = 2
LAST_ROUND: int = 2


def a_step(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """A→B→A 图的 A 节点：依据回边 ``back`` 是否到达决定轮次。

    输入：
        ``seed``（any，必填）：start 注入；
        ``back``（any，可选）：``B→A.back`` 回投的 B 输出；首轮为 None。
    输出：
        ``{"go": True, "round": 1|2}``——gate 只读 ``go``（恒 true），轮次用于 B 判定。
    幂等性：
        纯函数，相同输入输出相同。
    外部副作用：
        无。
    """
    back: Any = inputs.get("back")
    round_index: int = FIRST_ROUND if back is None else SECOND_ROUND
    return {"go": True, "round": round_index}


def b_step(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """A→B→A 图的 B 节点：按 A 的轮次决定是否继续回环。

    输入：
        ``from_a``（any，必填）：``A→B.from_a`` 投递的 A 输出（dict）。
    输出：
        ``{"next": bool, "round": int}``——``next`` 为 true 时 ``B→A`` 放行触发 A 重入，
        第二轮为 false（gate=false + defer → 挂账 dormant）。
    幂等性：
        纯函数，相同输入输出相同。
    外部副作用：
        无。
    """
    from_a: Any = inputs.get("from_a")
    round_index: int = from_a.get("round", 0) if isinstance(from_a, dict) else 0
    return {"next": round_index < LAST_ROUND, "round": round_index}


def stop(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """defer_blocked 图的 a 节点：输出 ``go=false``，使两条 defer 边都只挂账。

    输入：
        ``seed``（any，必填）：start 注入；``again``（any，可选）：自环回投。
    输出：
        ``{"go": False}``——``check_output{path:"go",equals:true}`` 恒 false。
    幂等性：
        纯函数，相同输入输出相同。
    外部副作用：
        无。
    """
    return {"go": False}
