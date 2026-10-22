"""复杂 tick-driven + parallel fan-out 场景 e2e：用 orc.run() 跑完整 test_for_complex SOP。

对应 graph: config/test_for_complex_graph.json
  fetcher (src, self-loop → raw.json 产出)
    ├─fetcher_to_worker_a (parallel=true) → worker_a
    ├─fetcher_to_worker_b (parallel=true) → worker_b
    └─fetcher_to_worker_c (parallel=true) → worker_c
  worker_a/b/c（独立 iport=raw.json；parallel dispatch 一次 tick 启动 3 个 worker）
    - worker_a 是 primary（state.current_node 切到 worker_a）
    - worker_b/c 是 background（dispatch 但不影响 current_node）
  worker_a 自环 1 次 → 等齐 all_workers_done_gate → joiner
  joiner (iport=a/b/c_done.json) → verifier → publisher (sink)

覆盖场景（全 tick 驱动，无 manual 边）：
- parallel fan-out（fetcher 一次 tick 启动 3 个 worker，验证 kernel select_next_edges
  + scheduler tick 都正确处理 parallel 边组）
- 多 cycle / 自环 + gate（fetcher / worker_a / joiner / verifier 全部自环 + gate 翻转）
- join 多 iport（joiner 需 3 个 done 文件都产）
- 多产物文件（每个 worker 一次产 3 个文件：done + 2 batch）
- 错误路径（verifier 第 1 次产 invalid 文件 → 自环 gate 看不到 valid → loop；
  第 2 次产 valid 文件 → gate 通过 → 切到 publisher）
- node proc 处理超时（worker_c_proc 内部 sleep 0.5 模拟慢调用，验证
  parallel dispatch 后 scheduler 不会因单次 proc > tick_period_s 崩溃——
  fast tick_period_s=0.5 仍能跑到 sink）
- orc.run() until-done：publisher 是 sink，跑完自然停
"""

from __future__ import annotations

import json
import re
import time
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent  # scripts/graph_base
CONFIG_NAME = "test_for_complex_graph.json"
SOP_NAME = "test_for_complex"


def _load_test_for_complex() -> None:
    """强制 import test_for_complex worker，触发 register_op（带 tfc_ 前缀）。"""
    from src.worker import test_for_complex  # noqa: F401


def _read_state(root: Path, iid: str) -> dict:
    return json.loads((root / SOP_NAME / f"{iid}.json").read_text())


def _wait_for(predicate, timeout: float = 20.0, interval: float = 0.1) -> bool:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if predicate():
            return True
        time.sleep(interval)
    return predicate()


def _load_graph_fast() -> object:
    """加载 config 并把 tick_period_s 改写成 0.5，加快 e2e 运行。

    原 config 也已改为 0.5，但保留此 helper 以防 config tick_period_s 被调整。
    """
    from src.kernel import Graph, load_graph
    from dataclasses import replace

    g = load_graph(REPO / "config" / CONFIG_NAME)
    if g.tick_period_s != 0.5:
        g = replace(g, tick_period_s=0.5)
    return g


def test_full_parallel_fanout_complex_sop(tmp_path: Path) -> None:
    """测试名：test_full_parallel_fanout_complex_sop

    测试场景：用 orc.run 真实跑 parallel fan-out 版 test_for_complex SOP 直到
    publisher（sink）。

    验证：
      1. fetcher parallel fan-out → worker_a（primary）+ worker_b/c（background）
         一次 tick 内 3 个 worker 全部 dispatch；
      2. 每节点 exit_cnt 符合预期（fetcher=1, workers=1, joiner=1, verifier=2, publisher=1）；
      3. 所有产物文件落盘：raw.json + 3×done.json + 3×2×batch.json + joined.json +
         verified.json + verified.invalid.json + release/notes.md + release/changelog.md；
      4. 错误路径：verifier 第 1 次产 invalid（verified.invalid.json 存在）；
      5. worker_c sleep(0.5) 不阻塞整体（fast tick_period_s=0.5 仍能跑到 sink）；
      6. orc.run 自然退出（不超时），返回的 alarms 无 GATE_EVAL_ERROR / EDGE_CMD_ERROR；
      7. parallel fan-out 的 3 个 cmd marker（fetcher_to_a/b/c.json）全部落盘；
      8. history 反映 parallel topology（fetcher self-loop + parallel transition +
         worker_a → joiner + joiner → verifier + verifier self-loop + verifier → publisher）。

    前置条件：tmp_path；REPO/config/test_for_complex_graph.json（tick_period_s=0.5）。
    是否使用 mock：No（真 Orchestrator + 真 ProcessDispatcher + 真 worker 子进程）。
    测试步骤：1. 创建 instance；2. 预写 seed.txt（fetcher iport 触发器）；
      3. orc.run() 阻塞到 publisher（sink）；4. 等所有 worker 产物落盘；
      5. 读 state 验证 exit_cnt 与 current_node；
      6. 验证每个产物的文件名格式与存在性。
    预期结果：alarms 无 gate/cmd 错误；current_node="publisher"（sink）；
      exit_cnt: fetcher=1, worker_a/b/c=1, joiner=1, verifier=2, publisher=1；
      全部产物文件落盘；3 个 parallel cmd marker 落盘。
    测试后清理：pytest tmp_path 自动清理。
    """
    from src.components import Orchestrator

    _load_test_for_complex()
    root = tmp_path
    g = _load_graph_fast()
    orc = Orchestrator(graph=g, root=root)

    iid = orc.create_instance()
    base = root / SOP_NAME / iid
    (base / "config").mkdir(parents=True, exist_ok=True)
    (base / "config" / "seed.txt").write_text("seed\n", encoding="utf-8")

    # 真实 orc.run()：until-done，publisher 是 sink 自动停。
    # tick_period_s=0.5，worker_c sleep 0.5 → 单 proc 跨多个 tick；
    # 但 enter/exit 同步检查保证 scheduler 不会重复 dispatch 同节点。
    alarms = orc.run(SOP_NAME, iid)
    # 关键断言：无错误类 alarm（gate / cmd 异常才会产生）
    assert "GATE_EVAL_ERROR" not in alarms, f"alarms: {alarms}"
    assert "EDGE_CMD_ERROR" not in alarms, f"alarms: {alarms}"

    state = _read_state(root, iid)
    # publisher 是 is_sink，run() 停在这里
    assert state["current_node"] == "publisher", f"state: {state}"
    # exit_cnt 精确（parallel fan-out 版）：
    # fetcher: 1 (self-loop 跑 1 次产 raw.json；之后 parallel transition 不再 dispatch fetcher)
    # workers: 各 1 (parallel dispatch 一次，proc 跑一次 → exit_cnt=1)
    # joiner: 1 (3 iport 满足后切出 → joiner 跑 1 次)
    # verifier: 2 (1st invalid loop, 2nd valid pass)
    # publisher: 1 (sink)
    expected_exit = {
        "fetcher": 1,
        "worker_a": 1,
        "worker_b": 1,
        "worker_c": 1,
        "joiner": 1,
        "verifier": 2,
        "publisher": 1,
    }
    assert dict(state["exit_cnt"]) == expected_exit, (
        f"exit_cnt 错位: 实际 {dict(state['exit_cnt'])}, 期望 {expected_exit}"
    )

    # 等所有 worker 产物落盘（tick 调度后 worker 子进程是异步写产物，
    # 读 state 时可能 last_output 已记录但文件还在写）。
    ok = _wait_for(
        lambda: (
            (base / "release" / "notes.md").exists()
            and (base / "release" / "changelog.md").exists()
            and (base / "data" / "raw.json").exists()
            and (base / "data" / "a_done.json").exists()
            and (base / "data" / "b_done.json").exists()
            and (base / "data" / "c_done.json").exists()
            and (base / "data" / "joined.json").exists()
            and (base / "data" / "verified.json").exists()
        ),
        timeout=20,
    )
    assert ok, "worker 产物应在 20s 内全部落盘"

    # 重新读 state —— 等所有 worker 子进程 record_completion 写完 last_output
    # （product files 落盘在 Phase 2（proc），last_output 写在 Phase 3 —— 有微小时间差）
    state = _read_state(root, iid)

    # 验证错误路径：verifier 第 1 次产 invalid（verified.invalid.json 存在）
    assert (base / "data" / "verified.invalid.json").exists(), (
        "verifier 第 1 次必须产 invalid 文件（错误路径演练）"
    )
    # valid 文件应包含 valid: true
    valid_content = (base / "data" / "verified.json").read_text(encoding="utf-8")
    assert '"valid": true' in valid_content, f"verified.json 内容错: {valid_content}"

    # 验证多产物文件：每个 worker 一次产 2 个 batch 文件（timestamp 唯一命名）
    # worker_a/b/c 各自 1 次 proc × 2 batch = 各 2 个
    for w in ("a", "b", "c"):
        batch_files = sorted((base / "data").glob(f"{w}_batch_*.json"))
        assert len(batch_files) == 2, (
            f"worker_{w} 应产 2 个 batch 文件，实际 {len(batch_files)}: "
            f"{[p.name for p in batch_files]}"
        )
        # 文件名格式：<prefix>_<timestamp>.json
        for p in batch_files:
            assert re.match(rf"{w}_batch_\d+_\d{{8}}_\d{{6}}_\d+\.json", p.name), (
                f"worker_{w} batch 文件名格式错: {p.name}"
            )
        # 文件名唯一（timestamp 不同）
        names = [p.name for p in batch_files]
        assert len(set(names)) == 2, f"worker_{w} 文件名应唯一: {names}"

    # 验证 parallel cmd marker：fetcher_to_{a,b,c}.json 3 个都应落盘
    # （证明 parallel 边组 3 条 cmd 都跑了，不是只跑第一条）
    for w in ("a", "b", "c"):
        assert (base / "data" / f"fetcher_to_{w}.json").exists(), (
            f"fetcher_to_{w}.json 缺失（parallel cmd 没跑：{w}）"
        )

    # 验证 last_output：每个 worker 都产 3 个文件（done + 2 batch）
    # worker_b/c 是 background，但 last_output 反映每次运行的产物
    for w in ("a", "b", "c"):
        last = state["last_output"].get(f"worker_{w}", ())
        assert len(last) == 3, (
            f"worker_{w} 应产 3 个文件，实际 {len(last)}: {last}"
        )

    # 验证 cmd 副作用：fetcher self-loop + parallel transition cmd 都跑过
    assert (base / "data" / "fetcher_loop.json").exists()
    # worker_a 不再走 self-loop（parallel fan-out 直接 dispatch worker_a_proc），
    # 所以 worker_a_loop_*.json 不存在；只看 to_joiner cmd。
    assert (base / "data" / "worker_a_passed.json").exists(), (
        "worker_a → joiner cmd 应写 worker_a_passed.json"
    )
    # joiner self-loop 不该跑（直接 all done → to_verifier）
    joiner_loop_files = list((base / "data").glob("joiner_loop_*.json"))
    assert not joiner_loop_files, (
        f"joiner self-loop 不该跑（all_workers_done_gate 直接 True），但发现: {joiner_loop_files}"
    )
    assert (base / "data" / "joiner_passed.json").exists()
    # verifier 1 次 loop cmd（exit_cnt=1 时跑）+ 1 次 pass cmd
    assert (base / "data" / "verifier_loop_1.json").exists(), (
        "verifier 第 1 次 self-loop cmd（exit_cnt=1）应写 verifier_loop_1.json"
    )
    assert (base / "data" / "verifier_passed.json").exists()

    # 验证 publisher 产物（last_output 反映最新一次运行的产物）
    state = _read_state(root, iid)
    last = state["last_output"]["publisher"]
    assert len(last) == 2, f"publisher 应产 2 个文件: {last}"
    assert any("notes.md" in p for p in last)
    assert any("changelog.md" in p for p in last)

    # 验证 history 完整（parallel fan-out 版）：
    # fetcher → worker_a(loop) → worker_a(to_joiner) → joiner(to_verifier)
    # → verifier(loop) → verifier(to_publisher)
    # history 记录每次 transition 的 from_node，publisher 是 sink 故不出现在
    # history 末尾；通过 current_node 验证。
    history_nodes = [h[0] for h in state["history"]]
    assert history_nodes[0] == "fetcher", f"history 开头错: {history_nodes}"
    # fetcher 出现 2 次（self-loop 1 + parallel transition 1）
    assert history_nodes.count("fetcher") == 2, (
        f"fetcher 应出现 2 次（self-loop + parallel 切出），实际 {history_nodes.count('fetcher')}: {history_nodes}"
    )
    # worker_a 出现 1 次（parallel fan-out 后直接 to_joiner，无 self-loop：
    # parallel dispatch 启动 worker_a_proc，proc 完成 → exit_cnt=1 →
    # 下一 tick worker_a_to_joiner 触发。无 worker_a_loop 边触发时机）
    a_indices = [i for i, n in enumerate(history_nodes) if n == "worker_a"]
    assert len(a_indices) == 1, (
        f"worker_a 应出现 1 次（parallel → to_joiner），实际 {a_indices}: {history_nodes}"
    )
    # joiner 出现 1 次（直接到 verifier，无 self-loop）
    assert history_nodes.count("joiner") == 1, (
        f"joiner 应出现 1 次（直接 to_verifier），实际 {history_nodes}"
    )
    joiner_idx = history_nodes.index("joiner")
    # verifier 出现 2 次（self-loop + to_publisher）均在 joiner 之后
    verifier_indices = [i for i, n in enumerate(history_nodes) if n == "verifier"]
    assert len(verifier_indices) == 2, (
        f"verifier 应出现 2 次（self-loop + to_publisher），实际 {verifier_indices}: {history_nodes}"
    )
    assert all(i > joiner_idx for i in verifier_indices), (
        f"verifier 应在 joiner 后: {history_nodes}"
    )
    # worker_b / worker_c 不出现在 history（parallel background，不参与 transition）
    assert "worker_b" not in history_nodes, (
        f"worker_b 是 parallel background，不应在 history: {history_nodes}"
    )
    assert "worker_c" not in history_nodes, (
        f"worker_c 是 parallel background，不应在 history: {history_nodes}"
    )


def test_complex_sop_runner_reaches_sink_within_bounded_time(tmp_path: Path) -> None:
    """测试名：test_complex_sop_runner_reaches_sink_within_bounded_time

    测试场景：orc.run() 必须在合理时间（< 30s）内跑完整个 test_for_complex SOP
    并自然停在 publisher（sink）。验证 scheduler 不会因 worker_c sleep(0.5)
    而死锁或反复重入，且 parallel fan-out 不导致重复 dispatch。

    前置条件：tmp_path；REPO/config/test_for_complex_graph.json。
    是否使用 mock：No（真 Orchestrator + 真 ProcessDispatcher + 真 worker 子进程）。
    测试步骤：1. 创建 instance + 预写 seed.txt；2. 计时 orc.run()；
      3. 验证总耗时 < 30s 且 current_node="publisher"。
    预期结果：orc.run() 返回时间 < 30s；current_node="publisher"；alarms 无错。
    测试后清理：pytest tmp_path 自动清理。
    """
    from src.components import Orchestrator

    _load_test_for_complex()
    root = tmp_path
    g = _load_graph_fast()
    orc = Orchestrator(graph=g, root=root)

    iid = orc.create_instance()
    base = root / SOP_NAME / iid
    (base / "config").mkdir(parents=True, exist_ok=True)
    (base / "config" / "seed.txt").write_text("seed\n", encoding="utf-8")

    start = time.time()
    alarms = orc.run(SOP_NAME, iid)
    elapsed = time.time() - start

    # 兜底时限：tick_period_s=0.5 + 7 节点（最坏 ~ 5s） + 调度开销，30s 留足安全余量
    assert elapsed < 30.0, f"orc.run 超时：{elapsed:.2f}s"
    assert "GATE_EVAL_ERROR" not in alarms
    assert "EDGE_CMD_ERROR" not in alarms

    state = _read_state(root, iid)
    assert state["current_node"] == "publisher", f"应停 publisher: {state}"