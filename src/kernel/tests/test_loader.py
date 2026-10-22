"""loader 测试。"""

import json
from pathlib import Path

import pytest

from src.kernel import load_graph
from src.kernel.loader import GraphLoadError


def test_load_software_team(tmp_path: Path):
    """测试名：test_load_software_team

    测试场景：加载内置 software_team_graph.json 得到 12 节点 / 11 gate / 25 边的 Graph。
    前置条件：config/software_team_graph.json 存在。
    是否使用 mock：No。
    测试步骤：调 load_graph(REPO/config/software_team_graph.json)。
    预期结果：name == "software_team"；12 个角色节点齐备；planer 是 src、
      release_done 是 sink（各恰好一个）；每个非 sink 节点都有出边。
    测试后清理：pytest tmp_path 自动清理。
    """
    src = Path(__file__).resolve().parents[3] / "config" / "software_team_graph.json"
    g = load_graph(src)
    assert g.name == "software_team"
    assert len(g.nodes) == 12
    assert len(g.gates) == 11
    assert len(g.edges) == 25

    names = {n.name for n in g.nodes}
    assert names == {
        "planer", "archer", "mde", "te", "mte", "ste",
        "fo", "mto", "sto", "qa", "release", "release_done",
    }
    # 角色标签：planer=PM / archer=SE，其余按 8 角色命名
    roles = {n.name: n.attr.role for n in g.nodes}
    assert roles["planer"] == "PM"
    assert roles["archer"] == "SE"
    assert roles["mde"] == "MDE" and roles["te"] == "TE"
    assert roles["mte"] == "MTE" and roles["ste"] == "STE"
    assert roles["fo"] == "FO"
    assert roles["mto"] == "MTO" and roles["sto"] == "STO"
    assert roles["qa"] == "QA"

    # src / sink 各恰好一个（loader 也会校验，这里断言图确实表达了这个意图）
    srcs = [n.name for n in g.nodes if n.attr.is_src]
    sinks = [n.name for n in g.nodes if n.attr.is_sink]
    assert srcs == ["planer"]
    assert sinks == ["release_done"]

    # 除 sink 外每个节点都有出边——否则 tick 走到那里就静默卡死
    out_nodes = {e.inode for e in g.edges}
    for n in g.nodes:
        if n.attr.is_sink:
            continue
        assert n.name in out_nodes, f"非 sink 节点 {n.name} 没有出边"


def test_load_missing_file(tmp_path: Path):
    """测试名：test_load_missing_file

    测试场景：load_graph 路径不存在时抛 GraphLoadError。
    前置条件：tmp_path 下无 nope.json。
    是否使用 mock：No。
    测试步骤：调 load_graph(tmp_path/"nope.json")。
    预期结果：抛 GraphLoadError 且 message 含 "不存在"。
    测试后清理：pytest tmp_path 自动清理。
    """
    with pytest.raises(GraphLoadError, match="不存在"):
        load_graph(tmp_path / "nope.json")


def test_load_invalid_json(tmp_path: Path):
    """测试名：test_load_invalid_json

    测试场景：JSON 解析失败时抛 GraphLoadError。
    前置条件：tmp_path/bad.json 写入 "{not json"。
    是否使用 mock：No。
    测试步骤：调 load_graph(bad.json)。
    预期结果：抛 GraphLoadError 且 message 含 "JSON"。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "bad.json"
    p.write_text("{not json", encoding="utf-8")
    with pytest.raises(GraphLoadError, match="JSON"):
        load_graph(p)


def test_load_missing_required_field(tmp_path: Path):
    """测试名：test_load_missing_required_field

    测试场景：JSON 缺 name 字段时抛 GraphLoadError。
    前置条件：tmp_path/missing.json 写入 {"nodes":[], "gates":[], "edges":[]}。
    是否使用 mock：No。
    测试步骤：调 load_graph(missing.json)。
    预期结果：抛 GraphLoadError 且 message 含 "name"。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "missing.json"
    p.write_text(json.dumps({"nodes": [], "gates": [], "edges": []}), encoding="utf-8")
    with pytest.raises(GraphLoadError, match="name"):
        load_graph(p)


def test_load_bad_graph_mode(tmp_path: Path):
    """测试名：test_load_bad_graph_mode

    测试场景：graph_mode 不在枚举值内时抛 GraphLoadError。
    前置条件：tmp_path/bad.json 写入 graph_mode="bogus"。
    是否使用 mock：No。
    测试步骤：调 load_graph(bad.json)。
    预期结果：抛 GraphLoadError 且 message 含 "graph_mode"。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "bad.json"
    p.write_text(
        json.dumps(
            {
                "name": "x",
                "graph_mode": "bogus",
                "nodes": [],
                "gates": [],
                "edges": [],
            }
        ),
        encoding="utf-8",
    )
    with pytest.raises(GraphLoadError, match="graph_mode"):
        load_graph(p)


def test_load_node_attr_is_src_must_be_bool(tmp_path: Path):
    """测试名：test_load_node_attr_is_src_must_be_bool

    测试场景：attr.is_src 非 bool 时抛 GraphLoadError。
    前置条件：tmp_path/bad.json 节点 attr.is_src="yes"（字符串）。
    是否使用 mock：No。
    测试步骤：调 load_graph(bad.json)。
    预期结果：抛 GraphLoadError 且 message 含 "is_src"。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "bad.json"
    p.write_text(
        json.dumps(
            {
                "name": "x",
                "graph_mode": "directed_cycle",
                "tick_period_s": 0.5,
                "nodes": [
                    {
                        "name": "a",
                        "iport": [],
                        "oport": [],
                        "attr": {"is_src": "yes", "role": "r"},
                    }
                ],
                "gates": [{"name": "g", "op": "f", "enum_dir": [True]}],
                "edges": [
                    {
                        "name": "e",
                        "inode": "a",
                        "onode": "a",
                        "driver": "tick",
                        "gate": "g",
                        "gate_value": True,
                        "cmd": "c",
                    }
                ],
            }
        ),
        encoding="utf-8",
    )
    with pytest.raises(GraphLoadError, match="is_src"):
        load_graph(p)


def test_missing_tick_period_s_raises(tmp_path: Path):
    """测试名：test_missing_tick_period_s_raises

    测试场景：JSON 缺 tick_period_s 字段时 load 抛 GraphLoadError。
    前置条件：tmp_path；最小合法图（无 tick_period_s）。
    是否使用 mock：No。
    测试步骤：1. 写不带 tick_period_s 的 JSON；2. load_graph(p)。
    预期结果：抛 GraphLoadError 且 message 含 "tick_period_s"。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "no_tick.json"
    p.write_text(json.dumps({
        "name": "x",
        "graph_mode": "directed_cycle",
        "nodes": [
            {"name": "a", "iport": [], "oport": [], "attr": {"is_src": True, "is_sink": True, "role": "r"}}
        ],
        "gates": [{"name": "g", "op": "f", "enum_dir": [True]}],
        "edges": [
            {"name": "e", "inode": "a", "onode": "a", "driver": "tick", "gate": "g", "gate_value": True, "cmd": "c"}
        ],
    }), encoding="utf-8")
    with pytest.raises(GraphLoadError, match="tick_period_s"):
        load_graph(p)


def test_tick_period_s_zero_or_negative_raises(tmp_path: Path):
    """测试名：test_tick_period_s_zero_or_negative_raises

    测试场景：tick_period_s <= 0 时 load 抛 GraphLoadError（避免硬编码短周期）。
    前置条件：tmp_path；JSON 含 tick_period_s=0 / tick_period_s=-1。
    是否使用 mock：No。
    测试步骤：分别写 tick_period_s=0 和 tick_period_s=-1 的 JSON，调 load_graph。
    预期结果：两次都抛 GraphLoadError 且 message 含 "tick_period_s 必须 > 0"。
    测试后清理：pytest tmp_path 自动清理。
    """
    for bad_val in (0, -1):
        p = tmp_path / f"bad_tick_{bad_val}.json"
        p.write_text(json.dumps({
            "name": "x",
            "graph_mode": "directed_cycle",
            "tick_period_s": bad_val,
            "nodes": [
                {"name": "a", "iport": [], "oport": [], "attr": {"is_src": True, "is_sink": True, "role": "r"}}
            ],
            "gates": [{"name": "g", "op": "f", "enum_dir": [True]}],
            "edges": [
                {"name": "e", "inode": "a", "onode": "a", "driver": "tick", "gate": "g", "gate_value": True, "cmd": "c"}
            ],
        }), encoding="utf-8")
        with pytest.raises(GraphLoadError, match="tick_period_s 必须 > 0"):
            load_graph(p)


def test_tick_period_s_non_numeric_raises(tmp_path: Path):
    """测试名：test_tick_period_s_non_numeric_raises

    测试场景：tick_period_s 类型非数字（字符串）时 load 抛 GraphLoadError。
    前置条件：tmp_path；JSON 含 tick_period_s="0.5"（字符串）。
    是否使用 mock：No。
    测试步骤：写 tick_period_s="0.5" 的 JSON，调 load_graph。
    预期结果：抛 GraphLoadError 且 message 含 "tick_period_s 必须是数字"。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "str_tick.json"
    p.write_text(json.dumps({
        "name": "x",
        "graph_mode": "directed_cycle",
        "tick_period_s": "0.5",  # 字符串，类型错
        "nodes": [
            {"name": "a", "iport": [], "oport": [], "attr": {"is_src": True, "is_sink": True, "role": "r"}}
        ],
        "gates": [{"name": "g", "op": "f", "enum_dir": [True]}],
        "edges": [
            {"name": "e", "inode": "a", "onode": "a", "driver": "tick", "gate": "g", "gate_value": True, "cmd": "c"}
        ],
    }), encoding="utf-8")
    with pytest.raises(GraphLoadError, match="tick_period_s 必须是数字"):
        load_graph(p)


def test_load_parallel_defaults_to_false(tmp_path: Path):
    """测试名：test_load_parallel_defaults_to_false

    测试场景：edge 不写 parallel 字段时，Edge.parallel 默认 False（向后兼容）。
    前置条件：tmp_path；JSON 边缺 parallel 字段。
    是否使用 mock：No。
    测试步骤：1. 写不带 parallel 字段的 JSON；2. load_graph。
    预期结果：所有边的 parallel == False。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "no_parallel.json"
    p.write_text(json.dumps({
        "name": "x",
        "graph_mode": "directed_cycle",
        "tick_period_s": 0.5,
        "nodes": [
            {"name": "a", "iport": [], "oport": [], "attr": {"is_src": True, "is_sink": True, "role": "r"}}
        ],
        "gates": [{"name": "g", "op": "f", "enum_dir": [True]}],
        "edges": [
            {"name": "e", "inode": "a", "onode": "a", "driver": "tick", "gate": "g", "gate_value": True, "cmd": "c"}
        ],
    }), encoding="utf-8")
    g = load_graph(p)
    assert all(e.parallel is False for e in g.edges)


def test_load_parallel_true_parsed(tmp_path: Path):
    """测试名：test_load_parallel_true_parsed

    测试场景：edge parallel=true 时正确解析为 True。
    前置条件：tmp_path；JSON 边 parallel=true。
    是否使用 mock：No。
    测试步骤：1. 写带 parallel=true 的 JSON；2. load_graph。
    预期结果：该边的 parallel is True。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "parallel.json"
    p.write_text(json.dumps({
        "name": "x",
        "graph_mode": "directed_cycle",
        "tick_period_s": 0.5,
        "nodes": [
            {"name": "a", "iport": [], "oport": [], "attr": {"is_src": True, "is_sink": True, "role": "r"}}
        ],
        "gates": [{"name": "g", "op": "f", "enum_dir": [True]}],
        "edges": [
            {"name": "e", "inode": "a", "onode": "a", "driver": "tick",
             "gate": "g", "gate_value": True, "cmd": "c", "parallel": True}
        ],
    }), encoding="utf-8")
    g = load_graph(p)
    assert g.edges[0].parallel is True


def test_load_parallel_non_bool_raises(tmp_path: Path):
    """测试名：test_load_parallel_non_bool_raises

    测试场景：edge parallel 字段类型非 bool（字符串）时抛 GraphLoadError。
    前置条件：tmp_path；JSON 边 parallel="yes"。
    是否使用 mock：No。
    测试步骤：1. 写 parallel="yes" 的 JSON；2. load_graph。
    预期结果：抛 GraphLoadError 且 message 含 "parallel 必须是 bool"。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "bad_parallel.json"
    p.write_text(json.dumps({
        "name": "x",
        "graph_mode": "directed_cycle",
        "tick_period_s": 0.5,
        "nodes": [
            {"name": "a", "iport": [], "oport": [], "attr": {"is_src": True, "is_sink": True, "role": "r"}}
        ],
        "gates": [{"name": "g", "op": "f", "enum_dir": [True]}],
        "edges": [
            {"name": "e", "inode": "a", "onode": "a", "driver": "tick",
             "gate": "g", "gate_value": True, "cmd": "c", "parallel": "yes"}
        ],
    }), encoding="utf-8")
    with pytest.raises(GraphLoadError, match="parallel 必须是 bool"):
        load_graph(p)