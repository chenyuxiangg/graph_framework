"""test_for_complex SOP worker 注册中心。

注册 gate op / proc / cmd 到全局 registry。

拓扑（parallel fan-out）：
  fetcher (src, self-loop → raw.json 产出)
    ├─fetcher_to_worker_a (parallel=true) → worker_a
    ├─fetcher_to_worker_b (parallel=true) → worker_b
    └─fetcher_to_worker_c (parallel=true) → worker_c
       ↓ parallel dispatch（一次 tick 内 3 个 worker 同时启动）
  worker_a/b/c（独立 iport=raw.json；各自跑一次后 exit_cnt++）：
    - worker_a 是 primary（state.current_node 切到 worker_a）
    - worker_b/c 是 background（dispatch 但不影响 current_node）
  worker_a 等待 all_workers_done_gate → joiner
  joiner (iport=a/b/c_done.json) → verifier → publisher (sink)

复杂点：
- parallel fan-out（fetcher 一次 tick 启动 3 个 worker）
- 多 cycle / 自环 + gate（worker_a 自环 + 等齐；joiner 自环 + 等待 done；verifier 自环 + 错误路径）
- join 多 iport（joiner 需要 a/b/c 三个 done 文件都产才能切出）
- 多产物文件（每个 worker 一次产 3 个文件：done + 2 batch，每个时间戳唯一）
- 错误路径（verifier 第 1 次故意产 invalid 文件 → 自环 gate 看不到 valid → 继续 loop；
  第 2 次产 valid 文件 → gate 通过 → 切到 publisher）
- node proc 处理超时（worker_c_proc 内部 sleep(0.5) 模拟慢调用，验证
  scheduler 不会因单次 proc > tick_period_s 崩溃——fast tick_period_s=0.5 仍能跑到 sink）
- 全 tick 驱动，无 manual 边；orc.run() 自然停 publisher（sink）
"""

from __future__ import annotations

import time
from datetime import datetime
from pathlib import Path

from src.kernel import register_op


# ───────────── gate op（边放行条件） ─────────────


def fetcher_raw_gate(base_dir, state, graph, gate) -> bool:
    """fetcher → worker_a/b/c 出边 gate：raw.json 存在 → fan-out 切到 worker_a。
    """
    raw = base_dir / "data" / "raw.json"
    return raw.exists()


def worker_a_threshold_gate(base_dir, state, graph, gate) -> bool:
    """worker_a 自环 vs 切出控制：exit_cnt >= 1 表示已跑过一次。

    self-loop 边 gate_value=False（首次进入 worker_a 时跑）；
    worker_a_to_joiner 边用 all_workers_done_gate 单独判定（不依赖此 gate）。
    """
    return state.exit_cnt.get("worker_a", 0) >= 1


def all_workers_done_gate(base_dir, state, graph, gate) -> bool:
    """worker_a → joiner 出边 gate：3 个 done 文件全产 → 切到 joiner。

    worker_a 是 primary，worker_b/c 是 background（parallel dispatch 但不跟踪）。
    worker_b/c 完成后写 done 文件；此 gate 在所有 done 文件齐全时为 True。
    """
    return all(
        (base_dir / "data" / f"{x}_done.json").exists()
        for x in ("a", "b", "c")
    )


def joiner_iport_gate(base_dir, state, graph, gate) -> bool:
    """joiner 出边 gate：3 个 done 文件全产 → 切到 verifier；否则 loop。"""
    return all(
        (base_dir / "data" / f"{x}_done.json").exists()
        for x in ("a", "b", "c")
    )


def verifier_valid_gate(base_dir, state, graph, gate) -> bool:
    """verifier 出边 gate：verified.json 存在 → 切到 publisher；否则 loop。

    注意：只看 verified.json（valid 文件），不看 verified.invalid.json。
    这样 verifier 第 1 次产 invalid 时 gate 仍 False，强制 loop 第二次。
    """
    return (base_dir / "data" / "verified.json").exists()


# ───────────── proc（节点内部执行函数） ─────────────


def _ts_name(prefix: str, ext: str = "json") -> str:
    """生成唯一文件名：<prefix>_<timestamp>。<ext>。"""
    ts = datetime.now().strftime("%Y%m%d_%H%M%S_%f")
    return f"{prefix}_{ts}.{ext}"


def fetcher_proc(base_dir, state, graph, node_name):
    """fetcher 跑一次：写 data/raw.json（worker_a/b/c 的 iport 信号）。"""
    p = base_dir / "data" / "raw.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(
        f'{{"raw": true, "node": "{node_name}", "ts": "{datetime.now().isoformat()}"}}\n',
        encoding="utf-8",
    )
    return (str(p),)


def worker_a_proc(base_dir, state, graph, node_name):
    """worker_a 跑一次：写 3 个文件（done + 2 batch），每次时间戳命名。"""
    out_dir = base_dir / "data"
    out_dir.mkdir(parents=True, exist_ok=True)
    files = []
    done = out_dir / "a_done.json"
    done.write_text(
        f'{{"worker": "a", "ts": "{datetime.now().isoformat()}"}}\n',
        encoding="utf-8",
    )
    files.append(str(done))
    for i in (1, 2):
        p = out_dir / _ts_name(f"a_batch_{i}")
        p.write_text(
            f'{{"worker": "a", "batch": {i}, "ts": "{datetime.now().isoformat()}"}}\n',
            encoding="utf-8",
        )
        files.append(str(p))
    return tuple(files)


def worker_b_proc(base_dir, state, graph, node_name):
    """worker_b 跑一次：写 3 个文件（done + 2 batch），每次时间戳命名。

    与原顺序版不同的是：worker_b 不再依赖 worker_a 的 done 文件——iport 是
    raw.json（独立输入）；其产物 b_done.json 给 joiner。
    """
    out_dir = base_dir / "data"
    out_dir.mkdir(parents=True, exist_ok=True)
    files = []
    done = out_dir / "b_done.json"
    done.write_text(
        f'{{"worker": "b", "ts": "{datetime.now().isoformat()}"}}\n',
        encoding="utf-8",
    )
    files.append(str(done))
    for i in (1, 2):
        p = out_dir / _ts_name(f"b_batch_{i}")
        p.write_text(
            f'{{"worker": "b", "batch": {i}, "ts": "{datetime.now().isoformat()}"}}\n',
            encoding="utf-8",
        )
        files.append(str(p))
    return tuple(files)


def worker_c_proc(base_dir, state, graph, node_name):
    """worker_c 跑一次：写 3 个文件 + 内部 sleep 模拟慢调用（验证 scheduler 不崩）。

    sleep 0.5s == tick_period_s=0.5 —— 测试用 fast tick_period_s，验证即便单次
    proc 等于 tick_period，scheduler 也能正确同步（enter/exit 检查）。
    """
    time.sleep(0.5)  # 模拟慢 proc
    out_dir = base_dir / "data"
    out_dir.mkdir(parents=True, exist_ok=True)
    files = []
    done = out_dir / "c_done.json"
    done.write_text(
        f'{{"worker": "c", "ts": "{datetime.now().isoformat()}"}}\n',
        encoding="utf-8",
    )
    files.append(str(done))
    for i in (1, 2):
        p = out_dir / _ts_name(f"c_batch_{i}")
        p.write_text(
            f'{{"worker": "c", "batch": {i}, "ts": "{datetime.now().isoformat()}"}}\n',
            encoding="utf-8",
        )
        files.append(str(p))
    return tuple(files)


def joiner_proc(base_dir, state, graph, node_name):
    """joiner 跑一次：读 3 个 done 文件 + 写 joined.json。"""
    out_dir = base_dir / "data"
    out_dir.mkdir(parents=True, exist_ok=True)
    sources = []
    for x in ("a", "b", "c"):
        p = out_dir / f"{x}_done.json"
        if not p.exists():
            raise RuntimeError(f"joiner: {p.name} 未产，不能 join")
        sources.append(str(p))
    joined = out_dir / "joined.json"
    joined.write_text(
        f'{{"joined_from": {sources}, "ts": "{datetime.now().isoformat()}"}}\n',
        encoding="utf-8",
    )
    return (str(joined),)


def verifier_proc(base_dir, state, graph, node_name):
    """verifier 跑一次：第 1 次产 invalid，第 2 次产 valid（错误路径演练）。

    用 exit_cnt 判断：==0 时产 invalid（错误路径），>=1 时产 valid（正确）。
    强制自环 1 次：第 1 次 gate=False（verified.json 不存在）→ 继续 loop；
    第 2 次 gate=True → 切到 publisher。
    """
    out_dir = base_dir / "data"
    out_dir.mkdir(parents=True, exist_ok=True)
    if state.exit_cnt.get("verifier", 0) == 0:
        # 第 1 次：错误路径——产 invalid 文件（不满足 downstream gate）
        invalid = out_dir / "verified.invalid.json"
        invalid.write_text(
            f'{{"valid": false, "node": "{node_name}", "ts": "{datetime.now().isoformat()}"}}\n',
            encoding="utf-8",
        )
        return (str(invalid),)
    # 第 2 次：产 valid 文件
    valid = out_dir / "verified.json"
    valid.write_text(
        f'{{"valid": true, "node": "{node_name}", "ts": "{datetime.now().isoformat()}"}}\n',
        encoding="utf-8",
    )
    return (str(valid),)


def publisher_proc(base_dir, state, graph, node_name):
    """publisher 写 release/notes.md + changelog.md。"""
    out_dir = base_dir / "release"
    out_dir.mkdir(parents=True, exist_ok=True)
    p1 = out_dir / "notes.md"
    p2 = out_dir / "changelog.md"
    p1.write_text(
        "# Release Notes\n\n_auto-generated by test_for_complex_\n",
        encoding="utf-8",
    )
    p2.write_text(
        "# Changelog\n\n- v1.0 (test_for_complex)\n",
        encoding="utf-8",
    )
    return (str(p1), str(p2))


# ───────────── cmd（边副作用动作） ─────────────


def fetcher_loop_cmd(base_dir, state, graph, edge):
    """fetcher 自环 cmd：debug marker（实际产物由 fetcher_proc 写 raw.json）。"""
    p = base_dir / "data" / "fetcher_loop.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text('{"edge": "fetcher_loop"}\n', encoding="utf-8")


def fetcher_to_a_cmd(base_dir, state, graph, edge):
    """fetcher → worker_a cmd：标记 dispatch 触发。"""
    p = base_dir / "data" / "fetcher_to_a.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text('{"dispatch": "fetcher_to_worker_a"}\n', encoding="utf-8")


def fetcher_to_b_cmd(base_dir, state, graph, edge):
    """fetcher → worker_b cmd：标记 dispatch 触发（parallel fan-out）。"""
    p = base_dir / "data" / "fetcher_to_b.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text('{"dispatch": "fetcher_to_worker_b"}\n', encoding="utf-8")


def fetcher_to_c_cmd(base_dir, state, graph, edge):
    """fetcher → worker_c cmd：标记 dispatch 触发（parallel fan-out）。"""
    p = base_dir / "data" / "fetcher_to_c.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text('{"dispatch": "fetcher_to_worker_c"}\n', encoding="utf-8")


def worker_a_loop_cmd(base_dir, state, graph, edge):
    cycle = state.exit_cnt.get("worker_a", 0)
    dbg = base_dir / "data" / f"worker_a_loop_{cycle}.json"
    dbg.parent.mkdir(parents=True, exist_ok=True)
    dbg.write_text(
        f'{{"edge": "worker_a_loop", "cycle": {cycle}}}\n',
        encoding="utf-8",
    )


def worker_a_to_joiner_cmd(base_dir, state, graph, edge):
    """worker_a → joiner cmd：标记 transition 触发（all_workers_done_gate=True 后）。"""
    p = base_dir / "data" / "worker_a_passed.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text('{"worker": "a", "passed_to_joiner": true}\n', encoding="utf-8")


def joiner_loop_cmd(base_dir, state, graph, edge):
    cycle = state.exit_cnt.get("joiner", 0)
    dbg = base_dir / "data" / f"joiner_loop_{cycle}.json"
    dbg.parent.mkdir(parents=True, exist_ok=True)
    dbg.write_text(
        f'{{"edge": "joiner_loop", "cycle": {cycle}}}\n',
        encoding="utf-8",
    )


def joiner_pass_cmd(base_dir, state, graph, edge):
    p = base_dir / "data" / "joiner_passed.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text('{"joiner": "passed_to_verifier"}\n', encoding="utf-8")


def verifier_loop_cmd(base_dir, state, graph, edge):
    cycle = state.exit_cnt.get("verifier", 0)
    dbg = base_dir / "data" / f"verifier_loop_{cycle}.json"
    dbg.parent.mkdir(parents=True, exist_ok=True)
    dbg.write_text(
        f'{{"edge": "verifier_loop", "cycle": {cycle}}}\n',
        encoding="utf-8",
    )


def verifier_pass_cmd(base_dir, state, graph, edge):
    p = base_dir / "data" / "verifier_passed.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text('{"verifier": "passed_to_publisher"}\n', encoding="utf-8")


# ───────────── register ─────────────


register_op("tfc_fetcher_raw_gate", fetcher_raw_gate)
register_op("tfc_worker_a_threshold_gate", worker_a_threshold_gate)
register_op("tfc_all_workers_done_gate", all_workers_done_gate)
register_op("tfc_joiner_iport_gate", joiner_iport_gate)
register_op("tfc_verifier_valid_gate", verifier_valid_gate)

register_op("tfc_fetcher_proc", fetcher_proc)
register_op("tfc_worker_a_proc", worker_a_proc)
register_op("tfc_worker_b_proc", worker_b_proc)
register_op("tfc_worker_c_proc", worker_c_proc)
register_op("tfc_joiner_proc", joiner_proc)
register_op("tfc_verifier_proc", verifier_proc)
register_op("tfc_publisher_proc", publisher_proc)

register_op("tfc_fetcher_loop_cmd", fetcher_loop_cmd)
register_op("tfc_fetcher_to_a_cmd", fetcher_to_a_cmd)
register_op("tfc_fetcher_to_b_cmd", fetcher_to_b_cmd)
register_op("tfc_fetcher_to_c_cmd", fetcher_to_c_cmd)
register_op("tfc_worker_a_loop_cmd", worker_a_loop_cmd)
register_op("tfc_worker_a_to_joiner_cmd", worker_a_to_joiner_cmd)
register_op("tfc_joiner_loop_cmd", joiner_loop_cmd)
register_op("tfc_joiner_pass_cmd", joiner_pass_cmd)
register_op("tfc_verifier_loop_cmd", verifier_loop_cmd)
register_op("tfc_verifier_pass_cmd", verifier_pass_cmd)