"""graph_base 内核层。

提供图模型的纯数据建模、加载校验、图遍历、文件端口语义、状态原子读写、反射接口。
不调度、不发事件、不写日志——所有"做决定"的逻辑上移到组件层。
"""

from .edge import Driver, Edge
from .gate import Gate
from .graph import Graph, GraphBuildError, GraphMode
from .loader import GraphLoadError, load_graph
from .node import Node, NodeAttr
from .ports import (
    expand_iport_paths,
    expand_oport_paths,
    iport_satisfied,
    oport_produced,
    port_file_path,
)
from .registry import (
    DuplicateRegistrationError,
    RegistryKeyError,
    register_cmd,
    register_op,
    reset_registry,
    resolve_cmd,
    resolve_op,
)
from .state import (
    StateLock,
    claim,
    clear_claim,
    new_instance_id,
    pid_alive,
    read_state,
    state_dir,
    state_path,
    write_state,
)
from .traversal import (
    AmbiguousMatchError,
    CyclicGraphError,
    cycle_check,
    edges_from,
    edges_via_gate,
    match_gate,
    predecessors,
    successors,
    topo_sort,
)
from .validators import Issue, validate

__all__ = [
    # 数据
    "Graph",
    "GraphMode",
    "Node",
    "NodeAttr",
    "Gate",
    "Edge",
    "Driver",
    "Issue",
    # 加载与校验
    "load_graph",
    "validate",
    "GraphLoadError",
    "GraphBuildError",
    # 遍历
    "successors",
    "predecessors",
    "edges_from",
    "edges_via_gate",
    "match_gate",
    "cycle_check",
    "topo_sort",
    "CyclicGraphError",
    "AmbiguousMatchError",
    # 端口
    "iport_satisfied",
    "oport_produced",
    "port_file_path",
    "expand_iport_paths",
    "expand_oport_paths",
    # state
    "state_dir",
    "state_path",
    "new_instance_id",
    "StateLock",
    "read_state",
    "write_state",
    "claim",
    "clear_claim",
    "pid_alive",
    # 反射
    "register_op",
    "resolve_op",
    "register_cmd",
    "resolve_cmd",
    "reset_registry",
    "RegistryKeyError",
    "DuplicateRegistrationError",
]