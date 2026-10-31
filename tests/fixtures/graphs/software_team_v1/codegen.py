"""software_team SOP v1（W1）e2e 图 codegen：依赖矩阵 → DSL 图 JSON。

来源：`doc/design/software-team-sop-v1.md` §4.1（设计角色依赖矩阵）、§7（N 与 codegen
规则）、附录 A（单 item 管线骨架）、附录 B（DSL 约束）。

两个 profile 共用同一套生成规则（节点/边命名与 §7 一一对应）：

两个 **mode**（``--mode``，W2a / SOP v1.1）共用同一套节点/边规则：

- ``approved``（缺省，W1 基线）：无 defer 边、解析节点 ``parse_verdict.run``、
  不写 ``feedback``/``retry.max_attempts`` → 输出与 W1 **逐字节一致**；
- ``rework``（v1.1，W2a）：``W`` 增可选 ``feedback`` 端口、解析节点
  ``parse_verdict_rework``、环上全部可重入边标 ``on_gate_false:"defer"``、新增
  ``P → W.feedback`` 回边、``W/R/P`` 写 ``config.retry.max_attempts=3``。

两个 profile 的规模：

- ``full``：11 个设计角色 + 16 个工作 agent + 16 个评审 + 16 个解析 + 1 个 ``end``
  = **60 节点 / 100 边**（approved；rework 因 16 条 ``P→W`` 回边为 **60/116**）；
- ``smoke``：缩小但同构的**合成单阶段链路**（不是 §4.1 矩阵的子集）——``planer``
  设计角色扇出 3 个工作 agent → 3 个评审 → 3 个解析 → ``archer`` 叶节点 =
  **11 节点 / 18 边**（approved；rework 为 **11/21**）；fake LLM 按调用序消费脚本，
  因此 smoke 只保留一条阶段链路（不跑 full）。评审 agent 可用 ``--review-agent``
  换成 e2e 失败路径变体。

设计约束（附录 B）：

- 节点 id ``[A-Za-z0-9_.-]``、端口名是 Python 标识符；
- ``(from, to_node)`` 与 ``(to_node, to_input_port)`` 各自唯一 → 每个 item 独立节点、
  每个下游依赖 item 独立输入端口；
- 非 entry 节点入边 ≥ 1；``trigger=all`` 时每个 ``required`` 端口都有入边；
- ``planer`` 的 ``requirement`` 由 run 注入（不生成入边）；
- 所有 ``P → 下游`` 边都挂 ``check_output{path:"verdict", equals:"approved"}``；
- rework 的 defer 边必须满足 R15（源在含 defer 边的环上）：环上 ``W/R/P`` 的出边
  全部标 defer，环外 ``D → W/R`` 保持缺省 settle。

实现是**纯函数**（``build_graph`` / ``build_nodes`` / ``build_edges``）+ ``main``，
只用 stdlib：无随机数、无时间戳、字典按构造顺序序列化，同参数两次生成字节级一致
（e2e 断言两次生成的 graph_hash 相同）。

用法（在仓库根执行）::

    # full 参考图（60 节点/100 边）→ 文件
    python tests/fixtures/graphs/software_team_v1/codegen.py --profile full --out /tmp/software_team_full.json

    # smoke 单阶段链路（11 节点/18 边）→ 文件
    python tests/fixtures/graphs/software_team_v1/codegen.py --profile smoke --out /tmp/software_team_smoke.json

    # 省略 --out 时输出到 stdout；--profile 缺省为 full、--mode 缺省为 approved
    python tests/fixtures/graphs/software_team_v1/codegen.py --profile smoke

    # rework profile（SOP v1.1，AC-14 c/e）：smoke 11 节点/21 边
    python tests/fixtures/graphs/software_team_v1/codegen.py --profile smoke --mode rework --out /tmp/software_team_smoke_rework.json

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

MODE_APPROVED: str = "approved"
MODE_REWORK: str = "rework"
MODES: tuple[str, ...] = (MODE_APPROVED, MODE_REWORK)

WORK_MODULE: str = "software_team_stub"
PARSE_MODULE: str = "workers.software_team.parse_verdict"
PARSE_FUNCTION: str = "run"
PARSE_FUNCTION_REWORK: str = "parse_verdict_rework"
REVIEWER_AGENT: str = "design_reviewer"
ENTRY_ROLE: str = "planer"
END_NODE_ID: str = "end"

GATE_FUNCTION: str = "check_output"
GATE_PATH: str = "verdict"
GATE_EQUALS: str = "approved"
GATE_EQUALS_REWORK: str = "rework"
REWORK_MAX_ATTEMPTS: int = 3
REWORK_METADATA_SUFFIX: str = "_rework"

# (profile, mode) → (节点数, 边数)。approved 是 W1 硬基线（hash 逐字节不变）。
EXPECTED_COUNTS: dict[tuple[str, str], tuple[int, int]] = {
    (PROFILE_FULL, MODE_APPROVED): (60, 100),
    (PROFILE_SMOKE, MODE_APPROVED): (11, 18),
    (PROFILE_FULL, MODE_REWORK): (60, 116),
    (PROFILE_SMOKE, MODE_REWORK): (11, 21),
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


def _with_retry(config: dict[str, Any], mode: str) -> dict[str, Any]:
    """rework profile 给节点写 ``config.retry.max_attempts``（approved 不加键）。"""
    if mode == MODE_REWORK:
        config["retry"] = {"max_attempts": REWORK_MAX_ATTEMPTS}
    return config


def _work_node(role: Role, item: str, mode: str) -> dict[str, Any]:
    """工作 agent 节点：``python_node`` 纯函数 stub（W1 不消耗 LLM）。

    rework profile 增可选 ``feedback`` 端口（承接 ``P → W`` 回边；不能复用 ``task``，
    DR-005 §11 C-2）并写 ``max_attempts``；approved profile 输出与 W1 逐字节一致。
    """
    inputs: list[dict[str, Any]] = [_input("task", "any")]
    if mode == MODE_REWORK:
        inputs.append({"name": "feedback", "type": "any", "required": False})
    return {
        "id": _item_base(role, item),
        "type": "python_node",
        "inputs": inputs,
        "trigger": "all",
        "config": _with_retry(
            {"module": WORK_MODULE, "function": f"work_{role.name}_{item}"}, mode
        ),
    }


def _review_node(
    role: Role, item: str, review_agent: str, mode: str
) -> dict[str, Any]:
    """评审节点：共享 ``review_agent`` 资产（缺省 ``design_reviewer``），``spec`` + ``result`` 到齐才执行。"""
    return {
        "id": f"{_item_base(role, item)}_review",
        "type": "agent_call",
        "inputs": [_input("spec", "any"), _input("result", "any")],
        "trigger": "all",
        "config": _with_retry({"agent_id": review_agent}, mode),
    }


def _parse_node(role: Role, item: str, mode: str) -> dict[str, Any]:
    """解析节点 `P`：``review`` + ``artifact`` 到齐后解析 verdict 并透传产物。

    rework profile 用 ``parse_verdict_rework``（rework 是正常输出，不抛错）并写
    ``max_attempts``；approved profile 仍用 ``parse_verdict.run``（W1 不变）。
    """
    function = PARSE_FUNCTION_REWORK if mode == MODE_REWORK else PARSE_FUNCTION
    return {
        "id": f"{_item_base(role, item)}_parse",
        "type": "python_node",
        "inputs": [_input("review", "any"), _input("artifact", "any")],
        "trigger": "all",
        "config": _with_retry({"module": PARSE_MODULE, "function": function}, mode),
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


def build_nodes(
    roles: Sequence[Role],
    end_deps: tuple[str, ...],
    review_agent: str = REVIEWER_AGENT,
    mode: str = MODE_APPROVED,
) -> list[dict[str, Any]]:
    """按 §7 规则展开节点：``D`` + 每个 item 的 ``W/R/P`` + 可选 ``end``。"""
    nodes: list[dict[str, Any]] = []
    for role in roles:
        nodes.append(_design_node(role))
        for item in role.items:
            nodes.append(_work_node(role, item, mode))
            nodes.append(_review_node(role, item, review_agent, mode))
            nodes.append(_parse_node(role, item, mode))
    if end_deps:
        nodes.append(_end_node(end_deps))
    return nodes


def _gate() -> dict[str, Any]:
    """approved 不变量 gate（SOP §5.3：失败由 `P` 抛错承担，gate 只做最后一道保险）。"""
    return {
        "function": GATE_FUNCTION,
        "params": {"path": GATE_PATH, "equals": GATE_EQUALS},
    }


def _gate_rework() -> dict[str, Any]:
    """rework 回边 gate：``check_output{path:"verdict", equals:"rework"}``。"""
    return {
        "function": GATE_FUNCTION,
        "params": {"path": GATE_PATH, "equals": GATE_EQUALS_REWORK},
    }


def _defer(edge: dict[str, Any], mode: str) -> dict[str, Any]:
    """rework profile 给可重入边标 ``on_gate_false:"defer"``（approved 不写该键）。"""
    if mode == MODE_REWORK:
        edge["on_gate_false"] = "defer"
    return edge


def _item_edges(role: Role, item: str, mode: str) -> list[dict[str, Any]]:
    """单 item 管线的 5 条内部边（SOP §7 第 2 步）。

    rework profile 把 ``W→R``、``W→P.artifact``、``R→P`` 三条边标 defer（目标已
    Completed 时 attempt 重入）；``D→W/D→R`` 在环外，保持缺省 settle。
    """
    base = _item_base(role, item)
    return [
        {"from": role.name, "to_node": base, "to_input_port": "task"},
        {"from": role.name, "to_node": f"{base}_review", "to_input_port": "spec"},
        _defer(
            {"from": base, "to_node": f"{base}_review", "to_input_port": "result"}, mode
        ),
        _defer(
            {"from": base, "to_node": f"{base}_parse", "to_input_port": "artifact"}, mode
        ),
        _defer(
            {
                "from": f"{base}_review",
                "to_node": f"{base}_parse",
                "to_input_port": "review",
            },
            mode,
        ),
    ]


def _delivery_edge(
    role: Role, item: str, to_node: str, to_port: str, mode: str
) -> dict[str, Any]:
    """``P → 下游 D / end`` 的 approved gate 投递边（rework 追加 defer）。"""
    return _defer(
        {
            "from": f"{_item_base(role, item)}_parse",
            "to_node": to_node,
            "to_input_port": to_port,
            "gate": _gate(),
        },
        mode,
    )


def _feedback_edge(role: Role, item: str) -> dict[str, Any]:
    """rework 回边 ``P → W.feedback``（gate=rework、defer：rework 放行重入 W）。"""
    return {
        "from": f"{_item_base(role, item)}_parse",
        "to_node": _item_base(role, item),
        "to_input_port": "feedback",
        "gate": _gate_rework(),
        "on_gate_false": "defer",
    }


def build_edges(
    roles: Sequence[Role],
    end_deps: tuple[str, ...],
    mode: str = MODE_APPROVED,
) -> list[dict[str, Any]]:
    """按 §7 规则展开边：80 条 item 内部边 + 每个依赖 item 一条 gate 投递边（+ rework 回边）。"""
    edges: list[dict[str, Any]] = []
    for role in roles:
        for item in role.items:
            edges.extend(_item_edges(role, item, mode))
    for role in roles:
        for item in role.items:
            ref = f"{role.name}.{item}"
            for downstream in roles:
                for dep in downstream.deps:
                    if dep == ref:
                        edges.append(
                            _delivery_edge(
                                role,
                                item,
                                downstream.name,
                                _split_ref(dep)[1],
                                mode,
                            )
                        )
            for dep in end_deps:
                if dep == ref:
                    edges.append(
                        _delivery_edge(
                            role, item, END_NODE_ID, _split_ref(dep)[1], mode
                        )
                    )
    if mode == MODE_REWORK:
        for role in roles:
            for item in role.items:
                edges.append(_feedback_edge(role, item))
    return edges


def _rework_item_ids(
    roles: Sequence[Role],
) -> tuple[set[str], set[str], set[str]]:
    """rework 自检用：``(W ids, R ids, P ids)``（节点 id 前缀 + 后缀约定）。"""
    work: set[str] = set()
    review: set[str] = set()
    parse: set[str] = set()
    for role in roles:
        for item in role.items:
            base = _item_base(role, item)
            work.add(base)
            review.add(f"{base}_review")
            parse.add(f"{base}_parse")
    return work, review, parse


def _assert_rework_nodes(
    nodes: Sequence[dict[str, Any]], roles: Sequence[Role]
) -> None:
    """rework 节点自检：W 有可选 feedback、P 用 v2 解析函数、W/R/P 有 max_attempts。"""
    work, review, parse = _rework_item_ids(roles)
    by_id = {node["id"]: node for node in nodes}
    for node_id in work:
        ports = {port["name"]: port for port in by_id[node_id]["inputs"]}
        assert ports.get("feedback", {}).get("required") is False, (
            f"{node_id}: rework W 必须有 required:false 的 feedback 端口"
        )
    for node_id in work | review | parse:
        retry = by_id[node_id]["config"].get("retry", {})
        assert retry.get("max_attempts") == REWORK_MAX_ATTEMPTS, (
            f"{node_id}: rework W/R/P 必须写 retry.max_attempts={REWORK_MAX_ATTEMPTS}"
        )
    for node_id in parse:
        assert by_id[node_id]["config"]["function"] == PARSE_FUNCTION_REWORK, (
            f"{node_id}: rework P 必须用 {PARSE_FUNCTION_REWORK}"
        )


def _assert_rework_edges(
    edges: Sequence[dict[str, Any]], roles: Sequence[Role]
) -> None:
    """rework 边自检（DR-005 §6.2 统一规则）：环上 W/R/P 出边全 defer、环外不得 defer。"""
    work, review, parse = _rework_item_ids(roles)
    ring_sources = work | review | parse
    feedback_edges = 0
    for edge in edges:
        source = edge["from"]
        policy = edge.get("on_gate_false")
        if source in ring_sources:
            assert policy == "defer", (
                f"rework 环边 {source}->{edge['to_node']}.{edge['to_input_port']} 必须 defer"
            )
            if edge["to_input_port"] == "feedback":
                feedback_edges += 1
        else:
            assert "on_gate_false" not in edge, (
                f"rework 环外边 {source}->{edge['to_node']} 不得标 defer"
            )
    expected = sum(len(role.items) for role in roles)
    assert feedback_edges == expected, (
        f"P→W.feedback 回边数 {feedback_edges} != item 数 {expected}"
    )


def _assert_rework_graph(
    graph: dict[str, Any], roles: Sequence[Role]
) -> None:
    """rework 生成后自检（节点端口/预算 + 环上 defer 全覆盖），漏标即 codegen 自身报错。"""
    _assert_rework_nodes(graph["nodes"], roles)
    _assert_rework_edges(graph["edges"], roles)


def _profile(profile: str) -> tuple[Sequence[Role], tuple[str, ...]]:
    """profile → (设计角色矩阵, end 依赖)。"""
    if profile == PROFILE_FULL:
        return FULL_ROLES, FULL_END_DEPS
    if profile == PROFILE_SMOKE:
        return SMOKE_ROLES, SMOKE_END_DEPS
    raise ValueError(f"unknown profile {profile!r}; expected one of {PROFILES}")


def build_graph(
    profile: str,
    review_agent: str = REVIEWER_AGENT,
    mode: str = MODE_APPROVED,
) -> dict[str, Any]:
    """生成一个 profile+mode 的图定义，并自检规模（§4.1）与 rework defer 统一规则。

    ``review_agent`` 缺省 ``design_reviewer``；e2e 失败路径可用
    ``design_reviewer_exhaust``（``max_iterations`` 耗尽的变体）覆盖
    ``HARNESS_MAX_ITERATIONS``，其余结构不变。``mode`` 缺省 ``approved``（W1 基线，
    输出逐字节不变）；``rework`` 追加 SOP v1.1 增量（defer/feedback/max_attempts）。
    """
    if mode not in MODES:
        raise ValueError(f"unknown mode {mode!r}; expected one of {MODES}")
    roles, end_deps = _profile(profile)
    nodes = build_nodes(roles, end_deps, review_agent, mode)
    edges = build_edges(roles, end_deps, mode)
    expected = EXPECTED_COUNTS[(profile, mode)]
    actual = (len(nodes), len(edges))
    if actual != expected:
        raise ValueError(
            f"codegen {profile}/{mode}: expected {expected[0]} nodes/{expected[1]} edges, "
            f"got {actual[0]} nodes/{actual[1]} edges"
        )
    name = f"software_team_v1_{profile}"
    description = "software_team SOP v1 e2e 图（SOP 设计稿 §4.1 依赖矩阵 / §7 codegen）"
    if mode == MODE_REWORK:
        name += REWORK_METADATA_SUFFIX
        description += "——rework profile（SOP v1.1 / DR-005）"
    graph = {
        "version": GRAPH_VERSION,
        "metadata": {"name": name, "description": description, "author": "w1-e2e"},
        "entry_points": [ENTRY_ROLE],
        "nodes": nodes,
        "edges": edges,
    }
    if mode == MODE_REWORK:
        _assert_rework_graph(graph, roles)
    return graph


def render(graph: dict[str, Any]) -> str:
    """确定性序列化：UTF-8、2 空格缩进、结尾换行。"""
    return json.dumps(graph, ensure_ascii=False, indent=2) + "\n"


def parse_args(argv: Sequence[str] | None) -> argparse.Namespace:
    """解析 ``--profile`` / ``--mode`` / ``--out`` / ``--review-agent``。"""
    parser = argparse.ArgumentParser(description="software_team SOP v1/v1.1 e2e 图 codegen")
    parser.add_argument(
        "--profile",
        choices=PROFILES,
        default=PROFILE_FULL,
        help="full = 60 节点参考图；smoke = 11 节点单阶段链路",
    )
    parser.add_argument(
        "--mode",
        choices=MODES,
        default=MODE_APPROVED,
        help=(
            "approved = W1 基线（无 defer，逐字节不变）；"
            "rework = SOP v1.1 有限返工（defer/feedback/max_attempts）"
        ),
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="输出 JSON 文件路径（缺省写 stdout）",
    )
    parser.add_argument(
        "--review-agent",
        default=REVIEWER_AGENT,
        help=(
            "评审 agent 资产 id（缺省 design_reviewer；e2e 失败路径可用 "
            "design_reviewer_exhaust 覆盖 HARNESS_MAX_ITERATIONS）"
        ),
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    """写图 JSON；``--out`` 缺省时输出到 stdout。"""
    args = parse_args(argv)
    graph = build_graph(args.profile, args.review_agent, args.mode)
    text = render(graph)
    if args.out is None:
        sys.stdout.write(text)
        return 0
    args.out.parent.mkdir(parents=True, exist_ok=True)
    args.out.write_text(text, encoding="utf-8", newline="\n")
    print(
        f"generated {args.profile}/{args.mode}: {len(graph['nodes'])} nodes, "
        f"{len(graph['edges'])} edges -> {args.out}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
