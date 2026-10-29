"""JSON 配置加载。

只做 JSON → 对象 + 字段类型校验 + 跨字段一致性校验，构造 Graph。
不做跨 SOP 唯一性检查（由调用方注入 existing_sop_names）。
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from .edge import Driver, Edge
from .gate import Gate
from .graph import Graph, GraphMode
from .node import Node, NodeAttr


class GraphLoadError(Exception):
    """加载 JSON 失败。"""

    def __init__(self, message: str, path: str = "") -> None:
        super().__init__(message)
        self.path = path


def load_graph(
    path: Path,
    existing_sop_names: list[str] | None = None,
) -> Graph:
    """读 JSON 配置文件，返回 Graph。失败抛 GraphLoadError。"""
    raw = _read_json_file(Path(path))
    return _build_from_raw(raw, existing_sop_names)


def _read_json_file(path: Path) -> dict:
    """读 JSON 文件并保证根是 dict。"""
    if not path.exists():
        raise GraphLoadError(f"配置文件不存在: {path}", "path")
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise GraphLoadError(f"JSON 解析失败: {exc}", "path") from exc
    if not isinstance(raw, dict):
        raise GraphLoadError("根必须是 JSON object", "path")
    return raw


def _build_from_raw(
    raw: dict, existing_sop_names: list[str] | None
) -> Graph:
    """从已解码的 dict 构造 Graph；失败统一转 GraphLoadError。"""
    name = _require_str(raw, "name", "name")
    graph_mode = _parse_graph_mode(raw)
    pre_handle = _opt_str(raw, "pre_handle", "pre_handle", "")
    post_handle = _opt_str(raw, "post_handle", "post_handle", "")
    nodes = _parse_nodes(raw)
    gates = _parse_gates(raw)
    edges = _parse_edges(raw)
    tick_period_s = _require_tick_period_s(raw)
    try:
        return Graph.build(
            name=name,
            graph_mode=graph_mode,
            pre_handle=pre_handle,
            post_handle=post_handle,
            nodes=nodes,
            gates=gates,
            edges=edges,
            tick_period_s=tick_period_s,
            existing_sop_names=existing_sop_names,
        )
    except Exception as exc:
        raise GraphLoadError(str(exc), getattr(exc, "path", "")) from exc


def _require_tick_period_s(raw: dict) -> float:
    """读 tick_period_s 字段（必须显式声明，无默认值）。

    强制显式声明——避免隐式 0.05 类短周期导致 gate 与 worker 写盘之间竞态：
    tick_period_s 太短时，scheduler tick 频率超过 worker 子进程启动 + 落盘速度，
    gate 评估看不到最新 exit_cnt，就会多派一次 worker。
    """
    if "tick_period_s" not in raw:
        raise GraphLoadError(
            "缺少必需字段 「tick_period_s」（调度器轮询周期，秒）",
            "tick_period_s",
        )
    v = raw["tick_period_s"]
    if not isinstance(v, (int, float)) or isinstance(v, bool):
        raise GraphLoadError(
            "tick_period_s 必须是数字（int/float）", "tick_period_s"
        )
    if v <= 0:
        raise GraphLoadError(
            "tick_period_s 必须 > 0", "tick_period_s"
        )
    return float(v)


def _parse_graph_mode(raw: dict) -> GraphMode:
    """读 graph_mode 字段并转换为 GraphMode。"""
    raw_value = _require(raw, "graph_mode", "graph_mode")
    if isinstance(raw_value, GraphMode):
        return raw_value
    if isinstance(raw_value, str):
        try:
            return GraphMode(raw_value)
        except ValueError as exc:
            raise GraphLoadError(
                f"graph_mode 非法: {raw_value!r}", "graph_mode"
            ) from exc
    raise GraphLoadError(
        f"graph_mode 必须是字符串，实际: {type(raw_value).__name__}",
        "graph_mode",
    )


def _parse_nodes(raw: dict) -> list[Node]:
    """解析 nodes 数组。"""
    raw_list = _require_list(raw, "nodes", "nodes")
    return [_parse_node(n, idx) for idx, n in enumerate(raw_list)]


def _parse_gates(raw: dict) -> list[Gate]:
    """解析 gates 数组。"""
    raw_list = _require_list(raw, "gates", "gates")
    return [_parse_gate(g, idx) for idx, g in enumerate(raw_list)]


def _parse_edges(raw: dict) -> list[Edge]:
    """解析 edges 数组。"""
    raw_list = _require_list(raw, "edges", "edges")
    return [_parse_edge(e, idx) for idx, e in enumerate(raw_list)]


def _require(d: dict, key: str, path: str) -> Any:
    if key not in d:
        raise GraphLoadError(f"缺少必需字段 「{key}」", path)
    return d[key]


def _require_str(d: dict, key: str, path: str) -> str:
    v = _require(d, key, path)
    if not isinstance(v, str):
        raise GraphLoadError(f"「{key}」 必须是字符串", path)
    return v


def _opt_str(d: dict, key: str, path: str, default: str) -> str:
    v = d.get(key)
    if v is None:
        return default
    if not isinstance(v, str):
        raise GraphLoadError(f"{path} 必须是字符串", path)
    return v


def _require_list(d: dict, key: str, path: str) -> list[Any]:
    v = _require(d, key, path)
    if not isinstance(v, list):
        raise GraphLoadError(f"「{key}」 必须是数组", path)
    return v


def _parse_node(raw: Any, idx: int) -> Node:
    if not isinstance(raw, dict):
        raise GraphLoadError(f"nodes[{idx}] 必须是 object", f"nodes[{idx}]")
    name = _require_str(raw, "name", f"nodes[{idx}].name")
    iport = _parse_port(raw.get("iport", []), f"nodes[{idx}].iport")
    oport = _parse_port(raw.get("oport", []), f"nodes[{idx}].oport")
    attr = _parse_node_attr(raw.get("attr", {}), f"nodes[{idx}].attr")
    proc = _opt_str(raw, "proc", f"nodes[{idx}].proc", "")
    return Node(name=name, iport=iport, oport=oport, attr=attr, proc=proc)


def _parse_node_attr(raw: Any, path: str) -> NodeAttr:
    """解析 nodes[i].attr 子对象（is_src / is_sink / role）。"""
    if raw is None:
        return NodeAttr()
    if not isinstance(raw, dict):
        raise GraphLoadError(f"{path} 必须是 object", path)
    is_src_raw = raw.get("is_src", False)
    if not isinstance(is_src_raw, bool):
        raise GraphLoadError(f"{path}.is_src 必须是 bool", f"{path}.is_src")
    is_sink_raw = raw.get("is_sink", False)
    if not isinstance(is_sink_raw, bool):
        raise GraphLoadError(f"{path}.is_sink 必须是 bool", f"{path}.is_sink")
    role = raw.get("role", "")
    if not isinstance(role, str):
        raise GraphLoadError(f"{path}.role 必须是字符串", f"{path}.role")
    return NodeAttr(is_src=is_src_raw, is_sink=is_sink_raw, role=role)


def _parse_port(raw: Any, path: str) -> tuple[tuple[str, ...], ...]:
    if not isinstance(raw, list):
        raise GraphLoadError(f"{path} 必须是二维数组", path)
    out: list[tuple[str, ...]] = []
    for g_idx, group in enumerate(raw):
        if not isinstance(group, list):
            raise GraphLoadError(f"{path}[{g_idx}] 必须是数组", f"{path}[{g_idx}]")
        files: list[str] = []
        for f_idx, spec in enumerate(group):
            if not isinstance(spec, str):
                raise GraphLoadError(
                    f"{path}[{g_idx}][{f_idx}] 必须是字符串",
                    f"{path}[{g_idx}][{f_idx}]",
                )
            files.append(spec)
        out.append(tuple(files))
    return tuple(out)


def _parse_gate(raw: Any, idx: int) -> Gate:
    if not isinstance(raw, dict):
        raise GraphLoadError(f"gates[{idx}] 必须是 object", f"gates[{idx}]")
    name = _require_str(raw, "name", f"gates[{idx}].name")
    op = _require_str(raw, "op", f"gates[{idx}].op")
    enum_raw = _require_list(raw, "enum_dir", f"gates[{idx}].enum_dir")
    enum_dir = tuple(enum_raw)
    return Gate(name=name, op=op, enum_dir=enum_dir)


def _parse_edge(raw: Any, idx: int) -> Edge:
    if not isinstance(raw, dict):
        raise GraphLoadError(f"edges[{idx}] 必须是 object", f"edges[{idx}]")
    name = _require_str(raw, "name", f"edges[{idx}].name")
    inode = _require_str(raw, "inode", f"edges[{idx}].inode")
    onode = _require_str(raw, "onode", f"edges[{idx}].onode")
    driver_raw = _require_str(raw, "driver", f"edges[{idx}].driver")
    try:
        driver = Driver(driver_raw)
    except ValueError as exc:
        raise GraphLoadError(
            f"edges[{idx}].driver 非法: {driver_raw!r}",
            f"edges[{idx}].driver",
        ) from exc
    gate = _require_str(raw, "gate", f"edges[{idx}].gate")
    if "gate_value" not in raw:
        raise GraphLoadError(
            f"edges[{idx}] 缺少 gate_value", f"edges[{idx}].gate_value"
        )
    gate_value = raw["gate_value"]
    cmd = _require_str(raw, "cmd", f"edges[{idx}].cmd")
    parallel_raw = raw.get("parallel", False)
    if not isinstance(parallel_raw, bool):
        raise GraphLoadError(
            f"edges[{idx}].parallel 必须是 bool，得到 {parallel_raw!r}",
            f"edges[{idx}].parallel",
        )
    return Edge(
        name=name,
        inode=inode,
        onode=onode,
        driver=driver,
        gate=gate,
        gate_value=gate_value,
        cmd=cmd,
        parallel=parallel_raw,
    )