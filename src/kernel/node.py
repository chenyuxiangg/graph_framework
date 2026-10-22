"""Node 数据类。"""

from dataclasses import dataclass, field


@dataclass(frozen=True)
class NodeAttr:
    """节点的"标识属性"，对应 JSON attr 子对象。

    is_src / is_sink / role 三者语义独立，可各自为空值。
    """

    is_src: bool = False
    is_sink: bool = False
    role: str = ""


@dataclass(frozen=True)
class Node:
    """图中的一个节点。

    iport / oport 都是二维 tuple（元组的元组）：
      - 组内 AND：组内所有文件必须都存在，组才"满足"或"产出"
      - 组间 OR：任一组满足，节点即可开始（或门控即可放行）

    attr 是 NodeAttr 子 dataclass，对应 JSON attr 子对象。
    proc 是节点内部执行函数名（注册到 kernel registry）；空串表示无内部逻辑，
    worker 内部用公共 proc_bypass 函数兜底。proc 与 pre_handle / post_handle
    独立，三者不互相替代。
    """

    name: str
    iport: tuple[tuple[str, ...], ...]
    oport: tuple[tuple[str, ...], ...]
    attr: NodeAttr = field(default_factory=NodeAttr)
    proc: str = ""