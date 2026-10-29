"""Edge 数据类与 Driver 枚举。"""

from dataclasses import dataclass
from enum import Enum
from typing import Any


class Driver(str, Enum):
    """边的驱动方式。"""

    TICK = "tick"
    MANUAL = "manual"


@dataclass(frozen=True)
class Edge:
    """图中的一条有向边（inode -> onode）。

    gate 引用 gates[].name；gate_value 是命中值（在对应 gate.enum_dir 里）；
    cmd 是函数名（registry.resolve_cmd 解析为可调用对象）。

    parallel：默认 False（单条 dispatch，向后兼容）。同一 inode 下共享 (gate,
    gate_value) 的整组边若全标 parallel=True → 一次 tick 内并行 dispatch 所有
    onode worker（state.current_node 切到 primary = edges[0].onode）。
    混合（部分 parallel=True + 部分 parallel=False）由 validator 报 E_PARALLEL_MIXED。
    """

    name: str
    inode: str
    onode: str
    driver: Driver
    gate: str
    gate_value: Any
    cmd: str
    parallel: bool = False