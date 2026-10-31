"""software_team SOP v1（W1）e2e 图 codegen：依赖矩阵 → DSL 图 JSON。

来源：`doc/design/software-team-sop-v1.md` §4.1（设计角色依赖矩阵）、§7（N 与 codegen
规则）、附录 A（单 item 管线骨架）、附录 B（DSL 约束）。

两个 profile 共用同一套生成规则（节点/边命名与 §7 一一对应）：

- ``full``：11 个设计角色 + 16 个工作 agent + 16 个评审 + 16 个解析 + 1 个 ``end``
  = **60 节点 / 100 边**（参考图；e2e 只做 ``zgraph validate`` 与规模断言）；
- ``smoke``：缩小但同构的单阶段链路——``planer`` 设计角色扇出 3 个工作 agent →
  3 个评审 → 3 个解析 → ``archer`` 叶节点 = **11 节点 / 18 边**；fake LLM 按调用序
  消费脚本，因此 smoke 只保留一条阶段链路（不跑 full）。

设计约束（附录 B）：

- 节点 id ``[A-Za-z0-9_.-]``、端口名是 Python 标识符；
- ``(from, to_node)`` 与 ``(to_node, to_input_port)`` 各自唯一 → 每个 item 独立节点、
  每个下游依赖 item 独立输入端口；
- 非 entry 节点入边 ≥ 1；``trigger=all`` 时每个 ``required`` 端口都有入边；
- ``planer`` 的 ``requirement`` 由 run 注入（不生成入边）；
- 所有 ``P → 下游`` 边都挂 ``check_output{path:"verdict", equals:"approved"}``。

实现是**纯函数**（``build_graph`` / ``build_nodes`` / ``build_edges``）+ ``main``，
只用 stdlib：无随机数、无时间戳、字典按构造顺序序列化，同参数两次生成字节级一致
（e2e 断言两次生成的 graph_hash 相同）。

用法（在仓库根执行）::

    # full 参考图（60 节点/100 边）→ 文件
    python tests/fixtures/graphs/software_team_v1/codegen.py --profile full --out /tmp/software_team_full.json

    # smoke 单阶段链路（11 节点/18 边）→ 文件
    python tests/fixtures/graphs/software_team_v1/codegen.py --profile smoke --out /tmp/software_team_smoke.json

    # 省略 --out 时输出到 stdout；--profile 缺省为 full
    python tests/fixtures/graphs/software_team_v1/codegen.py --profile smoke

生成后可用 ``zgraph validate <graph.json> --assets-dir tests/fixtures/graphs/software_team_v1``
校验（含 R14c agent 资产索引）；e2e 即按此链路执行。
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence

GRAPH_VERSION: str = "1.0"
PROFILE_FULL: str = "full"
PROFILE_SMOKE: str = "smoke"
PROFILES: tuple[str, ...] = (PROFILE_FULL, PROFILE_SMOKE)

WORK_MODULE: str = "software_team_stub"
PARSE_MODULE: str = "workers.software_team.parse_verdict"
PARSE_FUNCTION: str = "run"
REVIEWER_AGENT: str = "design_reviewer"
ENTRY_ROLE: str = "planer"
END_NODE_ID: str = "end"

GATE_FUNCTION: str = "check_output"
GATE_PATH: str = "verdict"
GATE_EQUALS: str = "approved"

EXPECTED_COUNTS: dict[str, tuple[int, int]] = {
    PROFILE_FULL: (60, 100),
    PROFILE_SMOKE: (11, 18),
}


@dataclass(frozen=True)
class Role:
    """一个设计角色：名字、依赖的上游 item（``role.item``）、产出的 item。"""

    name: str
    deps: tuple[str, ...]
    items: tuple[str, ...]


# §4.1 依赖矩阵（顺序即节点/边的生成顺序，保证确定性）。
FULL_ROLES: tuple[Role, ...] = (
    Role("planer", (), ("specification",)),
    Role("archer", ("planer.specification",), ("arch", "org", "iter_plan")),
    Role(
        "mde",
        ("archer.arch", "archer.org", "archer.iter_plan"),
        ("module_design", "module_iter_plan"),
    ),
    Role("te", ("archer.arch", "archer.org", "archer.iter_plan"), ("test_plan",)),
    Role("mte", ("te.test_plan",), ("module_cases",)),
    Role("ste", ("te.test_plan",), ("sys_cases",)),
    Role("fo", ("mde.module_design", "mde.module_iter_plan"), ("feature_process",)),
    Role("mto", ("mte.module_cases", "fo.feature_process"), ("module_report",)),
    Role("sto", ("ste.sys_cases", "mto.module_report"), ("sys_report",)),
    Role("qa", ("sto.sys_report",), ("qa_verdict",)),
    Role("release", ("qa.qa_verdict",), ("notes", "changelog", "guide")),
)
FULL_END_DEPS: tuple[str, ...] = (
    "release.notes",
    "release.changelog",
    "release.guide",
)

# smoke：同一套规则，单阶段链路（planer 扇出 3 个工作 agent → archer 叶节点）。
SMOKE_ROLES: tuple[Role, ...] = (
    Role(
        "planer",
        (),
        ("specification", "requirement_analysis", "iter_plan"),
    ),
    Role(
        "archer",
        (
            "planer.specification",
            "planer.requirement_analysis",
            "planer.iter_plan",
        ),
        (),
    ),
)
SMOKE_END_DEPS: tuple[str, ...] = ()


def _split_ref(ref: str) -> tuple[str, str]:
    """``"role.item"`` → ``(role, item)``。"""
    role, _, item = ref.partition(".")
    return role, item


def _input(name: str, type_name: str) -> dict[str, Any]:
    """一个 required 输入端口。"""
    return {"name": name, "type": type_name, "required": True}


def _design_node(role: Role) -> dict[str, Any]:
    """设计角色节点：entry 注入 ``requirement``，其余每个依赖 item 一个端口。"""
    if role.deps:
        inputs = [_input(_split_ref(dep)[1], "any") for dep in role.deps]
    else:
        inputs = [_input("requirement", "string")]
    return {
        "id": role.name,
        "type": "agent_call",
        "inputs": inputs,
        "trigger": "all",
        "config": {"agent_id": role.name},
    }


def _item_base(role: Role, item: str) -> str:
    """一个 item 的节点 id 前缀（work / review / parse 同前缀加后缀）。"""
    return f"{role.name}_{item}"


def _work_node(role: Role, item: str) -> dict[str, Any]:
    """工作 agent 节点：``python_node`` 纯函数 stub（W1 不消耗 LLM）。"""
    return {
        "id": _item_base(role, item),
        "type": "python_node",
        "inputs": [_input("task", "any")],
        "trigger": "all",
        "config": {
            "module": WORK_MODULE,
            "function": f"work_{role.name}_{item}",
        },
    }


def _review_node(role: Role, item: str) -> dict[str, Any]:
    """评审节点：共享 ``design_reviewer`` 资产，``spec`` + ``result`` 到齐才执行。"""
    return {
        "id": f"{_item_base(role, item)}_review",
        "type": "agent_call",
        "inputs": [_input("spec", "any"), _input("result", "any")],
        "trigger": "all",
        "config": {"agent_id": REVIEWER_AGENT},
    }


def _parse_node(role: Role, item: str) -> dict[str, Any]:
    """解析节点 `P`：``review`` + ``artifact`` 到齐后解析 verdict 并透传产物。"""
    return {
        "id": f"{_item_base(role, item)}_parse",
        "type": "python_node",
        "inputs": [_input("review", "any"), _input("artifact", "any")],
        "trigger": "all",
        "config": {"module": PARSE_MODULE, "function": PARSE_FUNCTION},
    }


def _end_node(end_deps: tuple[str, ...]) -> dict[str, Any]:
    """``end`` 节点：每个 release 产物一个端口（附录 B：≥1 required、trigger=all）。"""
    return {
        "id": END_NODE_ID,
        "type": "end",
        "inputs": [_input(_split_ref(dep)[1], "any") for dep in end_deps],
        "trigger": "all",
        "config": {},
    }


def build_nodes(roles: Sequence[Role], end_deps: tuple[str, ...]) -> list[dict[str, Any]]:
    """按 §7 规则展开节点：``D`` + 每个 item 的 ``W/R/P`` + 可选 ``end``。"""
    nodes: list[dict[str, Any]] = []
    for role in roles:
        nodes.append(_design_node(role))
        for item in role.items:
            nodes.append(_work_node(role, item))
            nodes.append(_review_node(role, item))
            nodes.append(_parse_node(role, item))
    if end_deps:
        nodes.append(_end_node(end_deps))
    return nodes


def _gate() -> dict[str, Any]:
    """approved 不变量 gate（SOP §5.3：失败由 `P` 抛错承担，gate 只做最后一道保险）。"""
    return {
        "function": GATE_FUNCTION,
        "params": {"path": GATE_PATH, "equals": GATE_EQUALS},
    }


def _item_edges(role: Role, item: str) -> list[dict[str, Any]]:
    """单 item 管线的 5 条内部边（SOP §7 第 2 步）。"""
    base = _item_base(role, item)
    return [
        {"from": role.name, "to_node": base, "to_input_port": "task"},
        {"from": role.name, "to_node": f"{base}_review", "to_input_port": "spec"},
        {"from": base, "to_node": f"{base}_review", "to_input_port": "result"},
        {"from": base, "to_node": f"{base}_parse", "to_input_port": "artifact"},
        {"from": f"{base}_review", "to_node": f"{base}_parse", "to_input_port": "review"},
    ]


def _delivery_edge(role: Role, item: str, to_node: str, to_port: str) -> dict[str, Any]:
    """``P → 下游 D / end`` 的 approved gate 投递边。"""
    return {
        "from": f"{_item_base(role, item)}_parse",
        "to_node": to_node,
        "to_input_port": to_port,
        "gate": _gate(),
    }


def build_edges(roles: Sequence[Role], end_deps: tuple[str, ...]) -> list[dict[str, Any]]:
    """按 §7 规则展开边：80 条 item 内部边 + 每个依赖 item 一条 gate 投递边。"""
    edges: list[dict[str, Any]] = []
    for role in roles:
        for item in role.items:
            edges.extend(_item_edges(role, item))
    for role in roles:
        for item in role.items:
            ref = f"{role.name}.{item}"
            for downstream in roles:
                for dep in downstream.deps:
                    if dep == ref:
                        edges.append(
                            _delivery_edge(role, item, downstream.name, _split_ref(dep)[1])
                        )
            for dep in end_deps:
                if dep == ref:
                    edges.append(
                        _delivery_edge(role, item, END_NODE_ID, _split_ref(dep)[1])
                    )
    return edges


def _profile(profile: str) -> tuple[Sequence[Role], tuple[str, ...]]:
    """profile → (设计角色矩阵, end 依赖)。"""
    if profile == PROFILE_FULL:
        return FULL_ROLES, FULL_END_DEPS
    if profile == PROFILE_SMOKE:
        return SMOKE_ROLES, SMOKE_END_DEPS
    raise ValueError(f"unknown profile {profile!r}; expected one of {PROFILES}")


def build_graph(profile: str) -> dict[str, Any]:
    """生成一个 profile 的图定义，并自检节点/边规模（§4.1 参考规模）。"""
    roles, end_deps = _profile(profile)
    nodes = build_nodes(roles, end_deps)
    edges = build_edges(roles, end_deps)
    expected = EXPECTED_COUNTS[profile]
    actual = (len(nodes), len(edges))
    if actual != expected:
        raise ValueError(
            f"codegen {profile}: expected {expected[0]} nodes/{expected[1]} edges, "
            f"got {actual[0]} nodes/{actual[1]} edges"
        )
    return {
        "version": GRAPH_VERSION,
        "metadata": {
            "name": f"software_team_v1_{profile}",
            "description": (
                "software_team SOP v1 e2e 图（SOP 设计稿 §4.1 依赖矩阵 / §7 codegen）"
            ),
            "author": "w1-e2e",
        },
        "entry_points": [ENTRY_ROLE],
        "nodes": nodes,
        "edges": edges,
    }


def render(graph: dict[str, Any]) -> str:
    """确定性序列化：UTF-8、2 空格缩进、结尾换行。"""
    return json.dumps(graph, ensure_ascii=False, indent=2) + "\n"


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    """解析 ``--profile`` / ``--out``。"""
    parser = argparse.ArgumentParser(description="software_team SOP v1 e2e 图 codegen")
    parser.add_argument(
        "--profile",
        choices=PROFILES,
        default=PROFILE_FULL,
        help="full = 60 节点/100 边参考图；smoke = 11 节点/18 边单阶段链路",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="输出 JSON 文件路径（缺省写 stdout）",
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    """写图 JSON；``--out`` 缺省时输出到 stdout。"""
    args = parse_args(argv)
    graph = build_graph(args.profile)
    text = render(graph)
    if args.out is None:
        sys.stdout.write(text)
        return 0
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8", newline="\n")
    print(
        f"generated {args.profile}: {len(graph['nodes'])} nodes, "
        f"{len(graph['edges'])} edges -> {args.out}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
