"""v1 整包广播解包辅助（e2e fixture 共用）。"""

from __future__ import annotations

from typing import Any


def unwrap_start(inputs: dict[str, Any]) -> dict[str, Any]:
    """从端口 ``x`` 取出测试 payload。

    输入：``inputs`` 为框架注入的端口字典。``start`` 输出是 ``{"x": payload}``，
    它被整体投递到下游端口 ``x``，因此首节点看到 ``{"x": {"x": payload}}``；
    中间节点若把 payload **原样透传**，下游看到 ``{"x": payload}``。

    输出：payload dict（取不到时返回空 dict）。
    幂等性：纯读，不修改入参。
    外部副作用：无。
    """
    value: Any = inputs.get("x") or {}
    if isinstance(value, dict) and isinstance(value.get("x"), dict):
        inner: Any = value["x"]
        return inner
    if isinstance(value, dict):
        return value
    return {}
