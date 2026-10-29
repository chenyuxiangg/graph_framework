"""示例 worker：``valid_cycle.json`` 的自环节点（``module=workers.cycle``）。"""

from __future__ import annotations

from typing import Any

from zgraph_worker import WorkerContext

FIRST_ITERATION: int = 1
RETRY_ITERATION: int = 2


def looping(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """自环节点：首轮返回 ``continue=True`` 触发回环，重试轮返回 ``False`` 走向 end。

    输入：
        ``seed``（any）：原样带回输出；首轮通常是 start 的整包 output（v1 整包广播）。
        ``again``（any）：自环边回投的上一轮整个 output。``None``（首轮尚未到达）或
        ``False`` 视为首轮；其余真值（含上一轮 output dict）视为重试轮。
    输出：
        ``{"continue": <bool>, "iteration": <1|2>, "seed": <seed>}``。fixture 门控读取
        ``continue`` 等价于 ``true`` 时回环，否则走至 end。
    幂等性：
        纯函数，只依据 ``inputs`` 决策；相同输入输出相同，可安全重试。
    外部副作用：
        无；仅 ``ctx.emit`` 发送一条可丢 ``progress`` 事件。
    """
    seed: Any = inputs.get("seed")
    again: Any = inputs.get("again")
    is_retry: bool = again is not None and again is not False
    iteration: int = RETRY_ITERATION if is_retry else FIRST_ITERATION
    ctx.emit(
        {
            "type": "progress",
            "percent": 100 if is_retry else 50,
            "message": "cycle retry" if is_retry else "cycle first",
        }
    )
    return {"continue": not is_retry, "iteration": iteration, "seed": seed}
