"""Gate 数据类。"""

from dataclasses import dataclass
from typing import Any


@dataclass(frozen=True)
class Gate:
    """图中的一个门控。

    op 是函数名，由 registry.resolve_op 解析为可调用对象。
    enum_dir 是一维 tuple，与 edges[].gate_value 对应：门控函数返回 enum_dir
    中的某个值时，匹配对应 gate_value 的边即被触发。
    """

    name: str
    op: str
    enum_dir: tuple[Any, ...]