"""示例 worker 共用：兼容 v1「整包广播」的端口取值辅助。

spec 04 §4.5：v1 下游端口收到的是上游**整个 output 对象**，不是字段值。当节点端口被上游
节点直接喂入时，端口值可能是 ``{"字段": 值, ...}`` 形态的 dict；业务示例需要同时支持
「直接给字段值」与「给整包 dict」两种调用方式，保证 fixture 图与单元测试都可用。
"""

from __future__ import annotations

from typing import Any


def unwrap_port(inputs: dict[str, Any], port: str, field: str) -> Any:
    """读取端口 ``port`` 的值；若它是上游整包 dict 且含 ``field``，返回该字段值。

    输入：
        ``inputs``：框架注入的端口字典；``port``：端口名；``field``：业务字段名。
    输出：
        端口值本身；当端口值是非空 dict 且包含 ``field`` 键时返回 ``value[field]``。
    幂等性：
        纯读函数，不修改 ``inputs``，相同入参返回相同结果。
    外部副作用：
        无。
    """
    value: Any = inputs.get(port)
    if isinstance(value, dict) and field in value:
        return value[field]
    return value
