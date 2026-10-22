"""复杂图 e2e：自环阈值 + manual trigger + 多产物文件 + 跨实例并发。

对应 graph: config/test_for_e2e_graph.json
  harvester (src, self-loop ×3 → qa_reviewer → publisher (manual, sink))

覆盖的缺失场景：
- 自环 N 次后切出边（gate 阈值翻转）
- 多产物文件（harvester 一次 3 个 batch）
- manual 边 trigger 后推进
- 跨实例并发隔离
"""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
import time
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parent.parent  # scripts/graph_base
CONFIG_NAME = "test_for_e2e_graph.json"
SOP_NAME = "test_for_e2e"


def _load_complex() -> None:
    """强制 import test_for_e2e worker，触发 register_op。"""
    from src.worker import test_for_e2e  # noqa: F401


def _read_state(root: Path, iid: str) -> dict:
    return json.loads((root / SOP_NAME / f"{iid}.json").read_text())


def _wait_for(predicate, timeout: float = 10.0, interval: float = 0.1) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return predicate()


def _tick_until(orc, sop: str, iid: str, base: Path,
                stop_predicate, max_ticks: int = 30) -> list[str]:
    """Tick + 等 worker 产出文件，避免 scheduler 比 worker 快撞到 iport not ready。

    每轮：tick() → 短暂 sleep → 重新检查 stop_predicate。
    """
    all_alarms: list[str] = []
    for _ in range(max_ticks):
        all_alarms.extend(orc.tick(sop, iid))
        time.sleep(0.3)  # 给 worker 一些时间写产物
        if stop_predicate():
            break
    return all_alarms


def _run_one_instance(root_str: str, label: str) -> str:
    """模块级函数给 ProcessPoolExecutor 用（避免 local 不能 pickle）。

    返回真实 instance_id（create_instance 内部生成，不接受入参）。
    """
    root = Path(root_str)
    from src.components import Orchestrator
    from src.kernel import load_graph

    g = load_graph(REPO / "config" / CONFIG_NAME)
    orc = Orchestrator(graph=g, root=root)
    real_iid = orc.create_instance()  # 自动生成 12 字符 hex
    base = root / SOP_NAME / real_iid
    (base / "config").mkdir(parents=True, exist_ok=True)
    (base / "config" / "seed.txt").write_text("seed\n", encoding="utf-8")
    # tick + 等 worker 写产物
    deadline = time.time() + 20
    while time.time() < deadline:
        orc.tick(SOP_NAME, real_iid)
        time.sleep(0.3)
        try:
            state = _read_state(root, real_iid)
            if state["current_node"] == "qa_reviewer":
                break
        except FileNotFoundError:
            continue
    return real_iid


def test_harvester_self_loop_threshold_to_qa(tmp_path: Path) -> None:
    """测试名：test_harvester_self_loop_threshold_to_qa

    测试场景：用 orc.run 真实跑 test_for_e2e SOP，验证 harvester 自环 3 次后
      切到 qa_reviewer（gate 按 exit_cnt 翻转），并最终停在 qa_reviewer
      （因为 qa→publisher 是 manual 边，没 trigger 时 orc.run 耗尽 max_ticks）。

    注意：本测试不 trigger qa_to_publisher，验证"run 真实跑 + 等不到 manual trigger 时
      停在中间节点 + 不崩"三件事。

    前置条件：tmp_path；REPO/config/test_for_e2e_graph.json；预写 config/seed.txt。
    是否使用 mock：No（真 Orchestrator + 真 ProcessDispatcher + 真 worker 子进程）。
    测试步骤：1. 创建 instance；2. 写 seed.txt；3. orc.run(SOP_NAME, iid)
      （不 trigger manual 边，停 qa 节点）；4. 等 worker 产物落盘；5. 读 state 验证自环阈值。
    预期结果：alarms 无 GATE_EVAL_ERROR / EDGE_CMD_ERROR；state.current_node=="qa_reviewer"；
      exit_cnt["harvester"]>=3 + exit_cnt["qa_reviewer"]>=1；
      harvester 写了 3×3 = 9 个唯一 batch 文件（每文件独立时间戳命名）+ 1 marker；
      loop_iter_{0,1,2}.json（cmd 副作用）+ harvester_passed.json + qa_pass_1.json 存在。
    测试后清理：pytest tmp_path 自动清理。
    """
    from src.components import Orchestrator
    from src.kernel import load_graph

    _load_complex()
    root = tmp_path
    g = load_graph(REPO / "config" / CONFIG_NAME)
    orc = Orchestrator(graph=g, root=root)

    iid = orc.create_instance()
    base = root / SOP_NAME / iid
    (base / "config").mkdir(parents=True, exist_ok=True)
    (base / "config" / "seed.txt").write_text("seed\n", encoding="utf-8")

    # 真用 orc.run：until-done；qa 节点没 manual trigger 时永远不 done，
    # run 会一直跑——所以这里不调 run，改用 _tick_until（带 max_ticks 兜底）。
    # （仍走真实 Orchestrator + 真实 worker 子进程，不绕过任何代码路径。）
    def _reached_qa() -> bool:
        try:
            return _read_state(root, iid).get("current_node") == "qa_reviewer"
        except FileNotFoundError:
            return False
    alarms = _tick_until(orc, SOP_NAME, iid, base, stop_predicate=_reached_qa, max_ticks=30)
    assert "GATE_EVAL_ERROR" not in alarms
    assert "EDGE_CMD_ERROR" not in alarms

    state = _read_state(root, iid)
    # harvester self-loop 恰好 3 次后切到 qa_reviewer：
    # tick_period_s=30（test_for_e2e 配置）+ enter/exit 同步检查保证 gate 评估必看到最新 exit_cnt
    assert state["current_node"] == "qa_reviewer", f"state: {state}"
    assert state["exit_cnt"]["harvester"] == 3
    assert state["exit_cnt"]["qa_reviewer"] == 1

    # orc.run 不等 worker 落盘（每个 tick 是 scheduler-side 调度），
    # 额外等 worker 产物落盘后再做内容断言。
    ok = _wait_for(
        lambda: (
            (base / "data" / "harvester_done.json").exists()
            and (base / "data" / "qa_pass_1.json").exists()
            and (base / "data" / "harvester_passed.json").exists()
        ),
        timeout=15,
    )
    assert ok, "harvester/qa worker 应在 15s 内完成 record_completion"

    # 关键断言（修复 Bug 1 后）：每个 harvester run 写 3 个 batch 文件，
    # 文件名按 timestamp 唯一化（无并发覆盖）；共 3 次 harvester run = 9 个 batch 文件。
    # last_output["harvester"] 是最后一次运行的产物（3 batch + 1 marker = 4 项）——
    # record_completion 用最新覆盖语义，不是累计。
    assert len(state["last_output"]["harvester"]) == 4
    batch_files = [
        Path(p) for p in state["last_output"]["harvester"]
        if "raw_batch_" in p
    ]
    assert len(batch_files) == 3, f"应 3 个最新 batch 文件，实际 {len(batch_files)}"
    # 文件名唯一（timestamp 不同）
    names = [p.name for p in batch_files]
    assert len(set(names)) == 3, f"文件名应唯一，实际 {names}"
    # 每个 batch 文件前缀匹配 raw_batch_<i>_<timestamp>
    import re
    for n in names:
        assert re.match(r"raw_batch_\d+_\d{8}_\d{6}_\d+\.json", n), f"格式错: {n}"
    # 落盘恰好 9 个 batch 文件（exit_cnt=3 × 3 files/run）
    actual_batch_count = len(list((base / "data").glob("raw_batch_*.json")))
    assert actual_batch_count == 9, (
        f"应 9 个 batch 文件落盘，实际 {actual_batch_count}"
    )

    # harvester_loop_cmd 副作用：3 次自环，每次 exit_cnt=X 时写 loop_iter_X.json
    for c in range(3):
        p = base / "data" / f"loop_iter_{c}.json"
        assert p.exists(), f"loop_iter_{c}.json 缺失（cmd 没跑）"
        content = p.read_text(encoding="utf-8")
        assert f'"cycle": {c}' in content, f"loop_iter_{c}: cycle 字段错位"
    # harvester_pass_cmd + qa_reviewer_proc 副作用
    assert (base / "data" / "harvester_passed.json").exists()
    assert (base / "data" / "qa_pass_1.json").exists()


def test_qa_to_publisher_requires_manual_trigger(tmp_path: Path) -> None:
    """测试名：test_qa_to_publisher_requires_manual_trigger

    测试场景：qa→publisher 是 manual 边；未 trigger 时停在 qa，
      trigger 后再 tick 才推进到 publisher（sink）。
    前置条件：tmp_path；test_for_e2e graph；预写 seed.txt。
    是否使用 mock：No（真 Orchestrator + manual trigger API）。
    测试步骤：1. 跑完 harvester 阶段到 qa；2. tick 多次验证卡在 qa；
      3. trigger_edge(qa_to_publisher)；4. 再 tick。
    预期结果：未 trigger 时 current_node="qa_reviewer" + 无 qa_to_publisher trigger；
      trigger 后 current_node="publisher" + release/notes.md + changelog.md + qa_approved.json。
    测试后清理：pytest tmp_path 自动清理。
    """
    from src.components import Orchestrator
    from src.kernel import load_graph

    _load_complex()
    root = tmp_path
    g = load_graph(REPO / "config" / CONFIG_NAME)
    orc = Orchestrator(graph=g, root=root)

    iid = orc.create_instance()
    base = root / SOP_NAME / iid
    (base / "config").mkdir(parents=True, exist_ok=True)
    (base / "config" / "seed.txt").write_text("seed\n", encoding="utf-8")

    # 跑完 harvester 阶段到 qa_reviewer
    def _reached_qa() -> bool:
        return _read_state(root, iid).get("current_node") == "qa_reviewer"

    _tick_until(orc, SOP_NAME, iid, base, stop_predicate=_reached_qa, max_ticks=30)
    state = _read_state(root, iid)
    assert state["current_node"] == "qa_reviewer"

    # tick 3 次：manual 边未 trigger → 应卡在 qa
    for _ in range(3):
        orc.tick(SOP_NAME, iid)
    state = _read_state(root, iid)
    assert state["current_node"] == "qa_reviewer"
    assert "qa_to_publisher" not in state["triggered_edges"]
    assert not (base / "release" / "notes.md").exists()

    # trigger + tick：publisher 跑 + 推进
    orc.trigger_edge(SOP_NAME, iid, "qa_to_publisher", "alice")
    orc.tick(SOP_NAME, iid)
    state = _read_state(root, iid)
    assert state["triggered_edges"]["qa_to_publisher"] == "alice"

    # 等 publisher worker 写产物
    ok = _wait_for(
        lambda: (base / "release" / "notes.md").exists()
        and (base / "release" / "changelog.md").exists(),
        timeout=10,
    )
    assert ok, "publisher worker 应在 trigger 后写出 release 产物"
    state = _read_state(root, iid)
    assert state["current_node"] == "publisher"
    assert len(state["last_output"].get("publisher", [])) == 2
    # qa_approve_cmd 副作用（trigger 后跑）
    assert (base / "data" / "qa_approved.json").exists()


def test_multi_instance_independent_complex_graph(tmp_path: Path) -> None:
    """测试名：test_multi_instance_independent_complex_graph

    测试场景：3 个 instance_id 并发跑同一 test_for_e2e SOP，
      各自独立完成 harvester 自环 + 各自停在 qa。
    前置条件：tmp_path；test_for_e2e graph；预写各 instance 的 seed.txt。
    是否使用 mock：No（ProcessPoolExecutor 真并发）。
    测试步骤：3 个 worker 进程各自 orc.run(max_ticks=20) 后读 state。
    预期结果：3 个 instance 的 current_node 都是 qa_reviewer；
      每个 instance 自己的 exit_cnt["harvester"] == 3；harvester 产物各恰好 9 个。
    测试后清理：pytest tmp_path 自动清理。
    """
    from concurrent.futures import ProcessPoolExecutor
    from src.components import Orchestrator
    from src.kernel import load_graph

    _load_complex()
    root = tmp_path

    labels = [f"multi_{i:06x}" for i in range(3)]
    with ProcessPoolExecutor(max_workers=3) as ex:
        real_iids = list(ex.map(_run_one_instance, [str(root)] * 3, labels))

    # 3 个真实 iid 各自独立跑到 qa
    for iid in real_iids:
        # 改用 last_output 计数（timestamp 文件名难预判）
        ok = _wait_for(
            lambda iid=iid: len([
                p for p in (root / SOP_NAME / iid / "data").glob("raw_batch_*.json")
            ]) >= 9,
            timeout=10,
        )
        assert ok, f"{iid}: harvester 产物不足"
        state = _read_state(root, iid)
        assert state["current_node"] == "qa_reviewer", f"{iid}: {state}"
        assert state["exit_cnt"]["harvester"] == 3, f"{iid}: {state['exit_cnt']}"
        # last_output 是最后一次运行的产物（3 batch + 1 marker = 4 项）；
        # 落盘恰好 9 个 batch 文件
        assert len(state["last_output"]["harvester"]) == 4
        actual_batch_count = len(
            list((root / SOP_NAME / iid / "data").glob("raw_batch_*.json"))
        )
        assert actual_batch_count == 9, (
            f"{iid}: 应 9 个 batch，实际 {actual_batch_count}"
        )
        assert (root / SOP_NAME / iid / "data" / "loop_iter_0.json").exists()
        assert (root / SOP_NAME / iid / "data" / "loop_iter_2.json").exists()
