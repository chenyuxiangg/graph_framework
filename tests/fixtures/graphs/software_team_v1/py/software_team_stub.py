"""software_team SOP v1 e2e 工作 agent stub（W1：`python_node` 纯函数，不消耗 LLM）。

SOP 设计稿 D11：确定性、无需 LLM 的工作项用 `python_node` 承载。本模块只做**产物
构造 + 上游任务包透传**：每个 item 一个函数，产出可辨识的 artifact

    {"kind": "software_team_work", "role": <设计角色>, "item": <item>, "task": <上游整包>}

**W2a 增量（仅 rework profile）**：rework 的 `W` 节点额外声明可选 `feedback` 端口；
当输入里**存在** `feedback` 键时，artifact 追加 `"feedback": <P→W 回投的上一轮解析输出>`。
approved profile 的 W 不声明该端口 → 输入无该键 → 输出与 W1 **逐字节一致**（W1 e2e 断言
全绿）；rework e2e 据此断言「下游拿到的是返工后最终 attempt 的 artifact」。

解析节点 `P` 会把该 artifact 原样透传给下游设计角色，e2e 据此断言「下游拿到的是工作
agent 的完整产物，不是评审摘要」（SOP §3 产物传播 / §11.3）。

模块名/函数名由 `codegen.py` 写进节点 `config.module/function`；worker 通过
`--python-path <fixture>/py` 导入本模块。函数覆盖 full profile 的 16 个工作 item 与
smoke profile 额外的 2 个 planer item。
"""

from __future__ import annotations

from typing import Any

from zgraph_worker import WorkerContext

ARTIFACT_KIND: str = "software_team_work"


def _artifact(role: str, item: str, inputs: dict[str, Any]) -> dict[str, Any]:
    """构造 item 产物：角色/item + 上游任务包（`task` 端口原样带回）。

    rework profile 的 W 声明了可选 `feedback` 端口：仅当输入**存在**该键时把
    P→W 回投的上一轮解析输出内嵌进 artifact（用于「下游拿到返工后最终产物」断言）；
    approved profile 输入无该键，输出键集与 W1 完全相同。
    """
    artifact: dict[str, Any] = {
        "kind": ARTIFACT_KIND,
        "role": role,
        "item": item,
        "task": inputs.get("task"),
    }
    if "feedback" in inputs:
        artifact["feedback"] = inputs["feedback"]
    return artifact


def work_planer_specification(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """planer.specification 工作项产物。"""
    return _artifact("planer", "specification", inputs)


def work_planer_requirement_analysis(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """planer.requirement_analysis 工作项产物（smoke profile）。"""
    return _artifact("planer", "requirement_analysis", inputs)


def work_planer_iter_plan(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """planer.iter_plan 工作项产物（smoke profile）。"""
    return _artifact("planer", "iter_plan", inputs)


def work_archer_arch(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """archer.arch 工作项产物。"""
    return _artifact("archer", "arch", inputs)


def work_archer_org(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """archer.org 工作项产物。"""
    return _artifact("archer", "org", inputs)


def work_archer_iter_plan(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """archer.iter_plan 工作项产物。"""
    return _artifact("archer", "iter_plan", inputs)


def work_mde_module_design(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """mde.module_design 工作项产物。"""
    return _artifact("mde", "module_design", inputs)


def work_mde_module_iter_plan(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """mde.module_iter_plan 工作项产物。"""
    return _artifact("mde", "module_iter_plan", inputs)


def work_te_test_plan(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """te.test_plan 工作项产物。"""
    return _artifact("te", "test_plan", inputs)


def work_mte_module_cases(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """mte.module_cases 工作项产物。"""
    return _artifact("mte", "module_cases", inputs)


def work_ste_sys_cases(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """ste.sys_cases 工作项产物。"""
    return _artifact("ste", "sys_cases", inputs)


def work_fo_feature_process(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """fo.feature_process 工作项产物。"""
    return _artifact("fo", "feature_process", inputs)


def work_mto_module_report(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """mto.module_report 工作项产物。"""
    return _artifact("mto", "module_report", inputs)


def work_sto_sys_report(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """sto.sys_report 工作项产物。"""
    return _artifact("sto", "sys_report", inputs)


def work_qa_qa_verdict(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """qa.qa_verdict 工作项产物。"""
    return _artifact("qa", "qa_verdict", inputs)


def work_release_notes(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """release.notes 工作项产物。"""
    return _artifact("release", "notes", inputs)


def work_release_changelog(
    inputs: dict[str, Any], ctx: WorkerContext
) -> dict[str, Any]:
    """release.changelog 工作项产物。"""
    return _artifact("release", "changelog", inputs)


def work_release_guide(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """release.guide 工作项产物。"""
    return _artifact("release", "guide", inputs)
