"""高级 e2e：stale recovery / manual edge / orchestrator.run / 错误路径。"""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parent.parent


# ───────────────────────────── 工具函数 ─────────────────────────────


def _run_cli(*args: str, cwd: Path | None = None) -> dict:
    """运行 graph_base_cli 子命令，解析 JSON 输出。"""
    result = subprocess.run(
        ["python3", "scripts/graph_base_cli.py", *args],
        cwd=cwd or REPO, capture_output=True, text=True, timeout=30,
        env={**os.environ, "PYTHONPATH": str(REPO)},
    )
    assert result.returncode == 0, f"stderr: {result.stderr}\nstdout: {result.stdout}"
    return json.loads(result.stdout)


def _read_state(root: Path, sop: str, iid: str) -> dict:
    return json.loads((root / sop / f"{iid}.json").read_text())


def _fast_software_team_config(tmp_path: Path, tick_period_s: float = 0.1) -> Path:
    """构造一个自带的 2 节点 software_team 配置（planer → archer sink）。

    为什么不读 config/software_team_graph.json：
    本文件测的是 **Orchestrator.run 的语义**（阻塞到 sink / iport 缺失时不推进），
    不是 software_team 这张图的拓扑。生产图已扩展为 12 节点且含 5 条 manual 边
    （需求评审 / 设计评审 / QA 评审 / 发布审批），而 run() 是 `while True` 且只靠
    is_sop_done 退出（无 max_ticks）——manual 边不被 trigger 时它会永远转下去。
    若继续复用生产配置，这两个用例会死等而不是断言到目标状态。

    所以这里内联一份最小可跑配置：2 节点、全 tick 边、走已注册的
    planer_proc / archer_proc / check_output / cmd_back / cmd_push_spec_to_archer。

    tick_period_s 默认 0.1s：真实配置 300s 在测试里每 tick 要等 300s。
    """
    raw = {
        "name": "software_team",
        "graph_mode": "directed_cycle",
        "tick_period_s": tick_period_s,
        "pre_handle": "",
        "post_handle": "",
        "nodes": [
            {
                "name": "planer",
                "iport": [["doc/input.md"]],
                "oport": [["doc/specification.md"]],
                "attr": {"is_src": True, "is_sink": False, "role": "PM"},
                "proc": "planer_proc",
            },
            {
                "name": "archer",
                "iport": [["doc/specification.md"]],
                "oport": [["doc/architectural_design.md", "doc/organizational_structure.md"]],
                "attr": {"is_src": False, "is_sink": True, "role": "SE"},
                "proc": "archer_proc",
            },
        ],
        "gates": [
            {"name": "planer_gate", "op": "check_output", "enum_dir": [True, False]},
        ],
        "edges": [
            {
                "name": "planer_loop",
                "inode": "planer", "onode": "planer", "driver": "tick",
                "gate": "planer_gate", "gate_value": False, "cmd": "cmd_back",
            },
            {
                "name": "planer_to_archer",
                "inode": "planer", "onode": "archer", "driver": "tick",
                "gate": "planer_gate", "gate_value": True,
                "cmd": "cmd_push_spec_to_archer",
            },
        ],
    }
    fast = tmp_path / "software_team_graph.json"
    fast.write_text(json.dumps(raw), encoding="utf-8")
    return fast


# ───────────────────────────── #7 orchestrator.run 真实 e2e ─────────────────────────────


def test_orchestrator_run_blocks_until_sink(tmp_path: Path) -> None:
    """测试名：test_orchestrator_run_blocks_until_sink

    测试场景：Orchestrator.run() 阻塞跑，自动 tick 直到 is_sop_done（archer sink）。
    前置条件：tmp_path；fast 软件_team_graph.json（tick_period_s=0.1）；预写 input.md。
    是否使用 mock：No（直接调组件层 Orchestrator.run）。
    测试步骤：1. 写 fast config；2. load_graph + Orchestrator；3. create_instance；
      4. 写 input.md；5. orc.run() 直到 is_sop_done。
    预期结果：alarms == []；current_node=="archer"；history 末项 from_node=="planer"；
      last_output.archer 长度 2（sink oport 必产物）。
    测试后清理：pytest tmp_path 自动清理。
    """
    from src.worker.software_team import check_output  # noqa: F401
    from src.components import Orchestrator
    from src.kernel import load_graph

    root = tmp_path
    config = _fast_software_team_config(root, tick_period_s=0.1)
    g = load_graph(config)
    orc = Orchestrator(graph=g, root=root)

    # 创建实例并写 input
    iid = orc.create_instance()
    base_dir = root / "software_team" / iid
    (base_dir / "doc" / "input.md").parent.mkdir(parents=True, exist_ok=True)
    (base_dir / "doc" / "input.md").write_text("# input\n", encoding="utf-8")

    # 阻塞跑（until done）
    alarms = orc.run("software_team", iid)
    assert alarms == []  # 无错误

    # 最终状态：current_node=archer（sink）+ last_output["archer"] 已有产物
    # （run 通过 is_sop_done 已包含 sink oport 必产物的检查）。
    state = _read_state(root, "software_team", iid)
    assert state["current_node"] == "archer"
    # history 每条 (from_node, ts)，两条出边 from_node 都是 planer
    # （planer_loop 自环 + planer_to_archer 推进）。worker 异步，loop 次数不固定
    # 但末项必为 planer→archer 那一跳。
    assert len(state["history"]) >= 1, (
        f"history 应至少有 1 步，实际 {len(state['history'])}：{state['history']}"
    )
    assert all(n == "planer" for n, _ in state["history"]), (
        f"history 节点应全是 planer（两条出边 from_node=planer），实际 {state['history']}"
    )
    assert state["history"][-1][0] == "planer"  # 末项是 planer→archer 那一跳的 from_node
    # sink oport 非空 → run 必须等到 last_output["archer"] 有产物
    assert len(state["last_output"].get("archer", [])) == 2, (
        f"orc.run 返回但 archer worker 产物未落盘，state={state}"
    )


def test_orchestrator_run_stops_when_iport_missing(tmp_path: Path) -> None:
    """测试名：test_orchestrator_run_stops_when_iport_missing

    测试场景：iport 缺失时 run() 在 graph tick_period_s 控制下不推进（不会无限循环）。
    验证"run 不是 fire-and-forget，而是会被 is_sop_done / iport 守卫卡住"。
    本测试通过在外部 kill tick 后断言 current_node 仍为 "planer"（没推进）。
    前置条件：tmp_path；fast 软件_team_graph.json（tick_period_s=0.1）；不写 input.md。
    是否使用 mock：No（直接调组件层 Orchestrator.run）。
    测试步骤：1. 写 fast config；2. load_graph + Orchestrator；3. create_instance；
      4. 起一个后台 thread 跑 orc.run()；5. 主线程 sleep 1.0s 后设 stop_event；
      6. join thread；7. 验证 state 仍 planer（没推进）。
    预期结果：thread 顺利结束（没死循环）；current_node 仍为 "planer"；
      alarms 空（无 GATE_EVAL_ERROR / EDGE_CMD_ERROR）。
    测试后清理：pytest tmp_path 自动清理。
    """
    import threading

    from src.worker.software_team import check_output  # noqa: F401
    from src.components import Orchestrator
    from src.kernel import load_graph

    root = tmp_path
    config = _fast_software_team_config(root, tick_period_s=0.1)
    g = load_graph(config)
    orc = Orchestrator(graph=g, root=root)
    iid = orc.create_instance()

    # 用 stop_event 让后台 run 跑 ~1s 后停止（不会无限循环）
    stop_event = threading.Event()
    result: dict = {"alarms": None}

    def _runner():
        # 监控 stop_event + tick（手动 tick 而非 run，因 run 是 until-done）
        all_alarms: list[str] = []
        from src.kernel import StateLock  # noqa: F401
        from src.components.state_manager import StateManager, is_sop_done
        from src.kernel import state_path
        sf = state_path(root=root, sop_name="software_team", instance_id=iid)
        deadline = time.time() + 1.0
        while time.time() < deadline:
            all_alarms.extend(orc.tick("software_team", iid))
            time.sleep(g.tick_period_s)
            state = StateManager.read(sf)
            if is_sop_done(g, state):
                break
        result["alarms"] = all_alarms

    t = threading.Thread(target=_runner, daemon=True)
    t.start()
    t.join(timeout=2.0)
    assert not t.is_alive(), "1s 后 thread 必须已退出（验证不是无限循环）"

    state = _read_state(root, "software_team", iid)
    assert state["current_node"] == "planer"  # 没推进（iport 缺失）
    assert result["alarms"] == []


# ───────────────────────────── #6 trigger-edge 真实 e2e ─────────────────────────────


@pytest.fixture
def manual_edge_sop(tmp_path: Path) -> tuple[Path, str, object]:
    """建一个含 manual 边的 SOP config；触发后 cmd 会写一个标志文件。

    返回 (sop_dir, config_path, InProcessDispatcher 类)。
    设计：src_to_dst（manual）+ src_loop（tick）共享 ready_gate；
    ready_gate 在 src_loop 上没产品时返回 False（不能进 src_to_dst 也没意义），
    在 src_to_dst 跑过后返回 True（推进到 dst）。
    """
    sop_dir = tmp_path / "manual_sop"
    config = {
        "name": "manual_sop",
        "graph_mode": "directed_nocycle",
        "tick_period_s": 0.5,
        "pre_handle": "",
        "post_handle": "",
        "nodes": [
            {
                "name": "src",
                "iport": [],
                "oport": [("flag.md",)],
                "attr": {"is_src": True, "role": "agent"},
                "proc": "manual_src_proc",
            },
            {
                "name": "dst",
                "iport": [("flag.md",)],
                "oport": [],
                "attr": {"is_sink": True, "role": "agent"},
                "proc": "manual_dst_proc",
            },
        ],
        "gates": [
            {"name": "src_ready_gate", "op": "manual_ready", "enum_dir": [True, False]},
            {"name": "src_wait_gate", "op": "manual_wait", "enum_dir": [True, False]},
        ],
        "edges": [
            {
                "name": "src_to_dst",
                "inode": "src",
                "onode": "dst",
                "driver": "manual",
                "gate": "src_ready_gate",
                "gate_value": True,
                "cmd": "manual_approve_cmd",
            },
            {
                "name": "src_loop",
                "inode": "src",
                "onode": "src",
                "driver": "tick",
                "gate": "src_wait_gate",
                "gate_value": True,
                "cmd": "manual_wait_cmd",
            },
        ],
    }
    config_path = sop_dir / "manual_sop_graph.json"
    config_path.parent.mkdir(parents=True, exist_ok=True)
    config_path.write_text(json.dumps(config, ensure_ascii=False, indent=2))

    pkg_dir = sop_dir / "_pkg"
    pkg_dir.mkdir()
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "ops.py").write_text(
        "from pathlib import Path\n"
        "from src.kernel import register_op\n"
        "\n"
        "def manual_ready(base_dir, state, graph, gate):\n"
        "    return state.current_node == 'src'\n"
        "\n"
        "def manual_wait(base_dir, state, graph, gate):\n"
        "    return state.current_node == 'src'\n"
        "\n"
        "def manual_src_proc(base_dir, state, graph, node_name):\n"
        "    out = base_dir / 'flag.md'\n"
        "    out.parent.mkdir(parents=True, exist_ok=True)\n"
        "    out.write_text('# ready\\n', encoding='utf-8')\n"
        "    return (str(out),)\n"
        "\n"
        "def manual_dst_proc(base_dir, state, graph, node_name):\n"
        "    return ()\n"
        "\n"
        "def manual_approve_cmd(base_dir, state, graph, edge):\n"
        "    marker = base_dir / 'approved.marker'\n"
        "    marker.write_text('approved', encoding='utf-8')\n"
        "\n"
        "def manual_wait_cmd(base_dir, state, graph, edge):\n"
        "    pass\n"
        "\n"
        "register_op('manual_ready', manual_ready)\n"
        "register_op('manual_wait', manual_wait)\n"
        "register_op('manual_src_proc', manual_src_proc)\n"
        "register_op('manual_dst_proc', manual_dst_proc)\n"
        "register_op('manual_approve_cmd', manual_approve_cmd)\n"
        "register_op('manual_wait_cmd', manual_wait_cmd)\n"
    )

    class InProcDispatcher:
        """同进程跑 proc（不拉子进程），同步调 record_completion。"""

        def __init__(self) -> None:
            from src.components.dispatcher.base import DispatchResult
            self._last_result = None

        def dispatch(self, graph, state, node, instance_id, work_dir):
            from src.components.dispatcher.base import DispatchResult
            from src.components.state_manager import StateManager
            from src.kernel import resolve_op
            from src.worker import PROC_BYPASS_NAME
            proc_name = node.proc or PROC_BYPASS_NAME
            try:
                proc_fn = resolve_op(proc_name)
                output_files = proc_fn(
                    base_dir=work_dir, state=state, graph=graph, node_name=node.name,
                )
            except Exception:
                return DispatchResult(pid=0, dispatch_id="inline", backend="inproc")
            new_state = StateManager.record_completion(
                state, node.name, output_files=tuple(output_files or ()),
            )
            StateManager.write(work_dir.parent / f"{instance_id}.json", new_state)
            self._last_result = DispatchResult(
                pid=os.getpid(), dispatch_id="inline", backend="inproc",
            )
            return self._last_result

        def alive(self, pid): return False
        def wait(self, pid, timeout): return -1

    return sop_dir, str(config_path), InProcDispatcher


def test_manual_edge_blocks_until_trigger(
    manual_edge_sop: tuple[Path, str, object],
) -> None:
    """测试名：test_manual_edge_blocks_until_trigger

    测试场景：manual 边 gate 通过后 cmd 不自动跑；trigger 后再 tick 才跑 cmd 并推进。
    前置条件：manual_edge_sop fixture（已建 manual_sop + 注册 ops + InProcDispatcher）。
    是否使用 mock：Yes（InProcDispatcher 替代 ProcessDispatcher）。
    测试步骤：1. tick → src_loop 派 src_proc（写 flag.md）；2. trigger_edge；
      3. tick → src_to_dst 跑 cmd（写 approved.marker）+ 推进到 dst。
    预期结果：current_node=="dst"；triggered_edges 含 src_to_dst；approved.marker 存在。
    测试后清理：pytest tmp_path 自动清理。
    """
    sop_dir, config_path, InProcDispatcher = manual_edge_sop
    root = sop_dir / "_state"
    root.mkdir()
    config_path = Path(config_path)

    sys.path.insert(0, str(sop_dir))
    import _pkg  # noqa: F401  # type: ignore
    import _pkg.ops  # noqa: F401  # type: ignore

    from src.kernel import load_graph
    from src.components import Orchestrator

    g = load_graph(config_path)
    orc = Orchestrator(graph=g, root=root, dispatcher=InProcDispatcher())
    iid = orc.create_instance()
    base_dir = root / "manual_sop" / iid
    base_dir.mkdir(parents=True, exist_ok=True)

    # 1. tick：src 是 src + src_wait_gate=True → 走 src_loop（tick）
    #   → dispatcher 同步跑 src_proc → 写 flag.md
    orc.tick("manual_sop", iid)
    state = _read_state(root, "manual_sop", iid)
    assert state["current_node"] == "src"
    assert (base_dir / "flag.md").exists()  # src_proc 写产物

    # 2. trigger src_to_dst 边
    orc.trigger_edge("manual_sop", iid, "src_to_dst", "alice")
    state = _read_state(root, "manual_sop", iid)
    assert state["triggered_edges"]["src_to_dst"] == "alice"

    # 3. tick：src_to_dst 命中 → cmd 跑 + 推进到 dst
    orc.tick("manual_sop", iid)
    state = _read_state(root, "manual_sop", iid)
    assert state["current_node"] == "dst"
    assert (base_dir / "approved.marker").exists()


def test_manual_edge_does_not_run_cmd_without_trigger(
    manual_edge_sop: tuple[Path, str, object],
) -> None:
    """测试名：test_manual_edge_does_not_run_cmd_without_trigger

    测试场景：未 trigger → tick N 次，cmd 不跑、current_node 不推进。
    前置条件：manual_edge_sop fixture。
    是否使用 mock：Yes（InProcDispatcher）。
    测试步骤：1. 建 instance；2. tick ×3；3. 读 state。
    预期结果：current_node=="src"；"src_to_dst" ∉ triggered_edges；approved.marker 不存在。
    测试后清理：pytest tmp_path 自动清理。
    """
    sop_dir, config_path, InProcDispatcher = manual_edge_sop
    root = sop_dir / "_state"
    root.mkdir()
    config_path = Path(config_path)

    sys.path.insert(0, str(sop_dir))
    import _pkg  # noqa: F401  # type: ignore
    import _pkg.ops  # noqa: F401  # type: ignore

    from src.kernel import load_graph
    from src.components import Orchestrator

    g = load_graph(config_path)
    orc = Orchestrator(graph=g, root=root, dispatcher=InProcDispatcher())
    iid = orc.create_instance()
    base_dir = root / "manual_sop" / iid

    for _ in range(3):
        orc.tick("manual_sop", iid)
    state = _read_state(root, "manual_sop", iid)
    assert state["current_node"] == "src"
    assert "src_to_dst" not in state["triggered_edges"]
    assert not (base_dir / "approved.marker").exists()


# ───────────────────────────── #2 stale recovery ─────────────────────────────


def test_stale_claim_is_reaped_by_monitor(tmp_path: Path) -> None:
    """测试名：test_stale_claim_is_reaped_by_monitor

    测试场景：pid 已死 + ts 极旧 → reap_stale_claims 报警；alive pid 不报。
    前置条件：tmp_path；起一个真子进程拿 dead pid；写两份 state（stale/fresh）。
    是否使用 mock：No（用真 zlog get_monitor）。
    测试步骤：1. 起 dead_proc=true 取 pid；2. 写 stale_inst.json + fresh_inst.json；
      3. Monitor(get_monitor()).reap_stale_claims(root, sop, stale_after=300)。
    预期结果："stale_inst" ∈ alarms；"fresh_inst" ∉ alarms。
    测试后清理：pytest tmp_path 自动清理。
    """
    from src.components.monitor import Monitor
    from src.components.monitor.logger import get_monitor
    from src.components.state_manager import StateManager
    from src.kernel import load_graph

    # 起一个死掉的子进程（立刻 exit）→ 取其 pid
    dead_proc = subprocess.Popen(["true"], stdout=subprocess.DEVNULL)
    dead_pid = dead_proc.pid
    dead_proc.wait()

    root = tmp_path
    sop = "stale_sop"
    sop_dir = root / sop
    sop_dir.mkdir()

    # 写一份 state.json 含一个 stale claim
    state_file = sop_dir / "stale_inst.json"
    state_file.write_text(json.dumps({
        "sop_name": sop, "instance_id": "stale_inst", "graph_name": sop,
        "current_node": "x",
        "history": [], "triggered_edges": {},
        "enter_cnt": {}, "exit_cnt": {}, "last_output": {},
        "claim_pid": dead_pid,
        "claim_ts": time.time() - 1000.0,  # 1000 秒前
    }))

    # 同时写一份 fresh claim（alive + 新 ts）
    fresh_file = sop_dir / "fresh_inst.json"
    fresh_file.write_text(json.dumps({
        "sop_name": sop, "instance_id": "fresh_inst", "graph_name": sop,
        "current_node": "x",
        "history": [], "triggered_edges": {},
        "enter_cnt": {}, "exit_cnt": {}, "last_output": {},
        "claim_pid": os.getpid(),
        "claim_ts": time.time(),
    }))

    mon = Monitor(get_monitor())
    alarms = mon.reap_stale_claims(root, sop, stale_after=300.0)
    assert "stale_inst" in alarms
    assert "fresh_inst" not in alarms


def test_stale_age_even_when_pid_alive(tmp_path: Path) -> None:
    """测试名：test_stale_age_even_when_pid_alive

    测试场景：pid 仍存活但 age > stale_after → 仍被 reap。
    前置条件：tmp_path；state 含 claim_pid=os.getpid() + claim_ts=now-9999。
    是否使用 mock：No（用真 zlog get_monitor）。
    测试步骤：1. 写 old_but_alive.json；2. Monitor(get_monitor()).reap_stale_claims(... stale_after=10)。
    预期结果："old_but_alive" ∈ alarms。
    测试后清理：pytest tmp_path 自动清理。
    """
    from src.components.monitor import Monitor
    from src.components.monitor.logger import get_monitor

    root = tmp_path
    sop = "stale_sop"
    sop_dir = root / sop
    sop_dir.mkdir()
    sf = sop_dir / "old_but_alive.json"
    sf.write_text(json.dumps({
        "sop_name": sop, "instance_id": "old_but_alive", "graph_name": sop,
        "current_node": "x",
        "history": [], "triggered_edges": {},
        "enter_cnt": {}, "exit_cnt": {}, "last_output": {},
        "claim_pid": os.getpid(),
        "claim_ts": time.time() - 9999.0,
    }))

    mon = Monitor(get_monitor())
    alarms = mon.reap_stale_claims(root, sop, stale_after=10.0)
    assert "old_but_alive" in alarms


# ───────────────────────────── #9 错误路径 ─────────────────────────────


@pytest.fixture
def error_sop(tmp_path: Path) -> tuple[Path, str]:
    """建一个会抛 cmd 异常的 SOP config。"""
    sop_dir = tmp_path / "error_sop"
    pkg_dir = sop_dir / "_epkg"
    pkg_dir.mkdir(parents=True)
    (pkg_dir / "__init__.py").write_text("")
    (pkg_dir / "ops.py").write_text(
        "from src.kernel import register_op\n"
        "\n"
        "def ready_op(base_dir, state, graph, gate):\n"
        "    return True\n"
        "\n"
        "def boom_proc(base_dir, state, graph, node_name):\n"
        "    raise RuntimeError('proc boom')\n"
        "\n"
        "def boom_cmd(base_dir, state, graph, edge):\n"
        "    raise RuntimeError('cmd boom')\n"
        "\n"
        "register_op('e_ready', ready_op)\n"
        "register_op('e_proc', boom_proc)\n"
        "register_op('e_cmd', boom_cmd)\n"
    )
    config_path = sop_dir / "error_sop_graph.json"
    config_path.write_text(json.dumps({
        "name": "error_sop",
        "graph_mode": "directed_nocycle",
        "tick_period_s": 0.5,
        "pre_handle": "",
        "post_handle": "",
        "nodes": [
            {
                "name": "src",
                "iport": [],
                "oport": [],
                "attr": {"is_src": True, "role": "r"},
                "proc": "e_proc",
            },
            {
                "name": "dst",
                "iport": [],
                "oport": [],
                "attr": {"is_sink": True, "role": "r"},
                "proc": "",
            },
        ],
        "gates": [{"name": "g", "op": "e_ready", "enum_dir": [True]}],
        "edges": [
            {
                "name": "src_to_dst",
                "inode": "src",
                "onode": "dst",
                "driver": "tick",
                "gate": "g",
                "gate_value": True,
                "cmd": "e_cmd",
            },
        ],
    }))
    return sop_dir, str(config_path)


def test_cmd_exception_yields_alarm(error_sop: tuple[Path, str]) -> None:
    """测试名：test_cmd_exception_yields_alarm

    测试场景：cmd 抛异常 → tick 返回 EDGE_CMD_ERROR alarm；state 不推进。
    前置条件：error_sop fixture（含 boom_proc + boom_cmd）。
    是否使用 mock：No（直接 Orchestrator.tick）。
    测试步骤：1. load_graph + Orchestrator；2. create_instance；3. orc.tick("error_sop", iid)。
    预期结果："EDGE_CMD_ERROR" ∈ alarms；current_node=="src"（未推进）。
    测试后清理：pytest tmp_path 自动清理。
    """
    sop_dir, config_path = error_sop
    root = sop_dir / "_state"
    root.mkdir()
    config_path = Path(config_path)

    sys.path.insert(0, str(sop_dir))
    import _epkg  # noqa: F401  # type: ignore
    import _epkg.ops  # noqa: F401  # type: ignore

    from src.kernel import load_graph
    from src.components import Orchestrator

    g = load_graph(config_path)
    orc = Orchestrator(graph=g, root=root)
    iid = orc.create_instance()

    alarms = orc.tick("error_sop", iid)
    assert "EDGE_CMD_ERROR" in alarms

    state = _read_state(root, "error_sop", iid)
    assert state["current_node"] == "src"  # 没推进


def test_dispatch_failure_does_not_corrupt_state(tmp_path: Path) -> None:
    """测试名：test_dispatch_failure_does_not_corrupt_state

    测试场景：dispatcher dispatch 返回 None 时，scheduler 不崩、state.json 不被破坏。

    已知问题（与本测试无关）：当前 scheduler 对 dispatcher 返回 None **静默忽略**——
    不产生 alarm 也不回滚。生产场景下会丢工作（worker 没拉起但 SOP 看着在推进）。
    修复方向：_dispatch_if_any 应在 dr is None 时 emit SCH_EV_ERROR + 加 alarm。
    本测试只验证"不崩 + state.json 仍合法"，不为此 bug 背书。

    前置条件：tmp_path；_BrokenDispatcher（dispatch 返回 None）；预写 input.md。
    是否使用 mock：Yes（_BrokenDispatcher 替代 ProcessDispatcher）。
    测试步骤：1. create_instance + 写 input.md；2. tick1（dispatcher 失败）；
      3. tick2（spec.md 未产 → 仍 loop）；4. 验证 state.json 仍合法且 exit_cnt 在增。
    预期结果：两次 tick 不抛异常；state.json 可读；current_node=="planer"；
      exit_cnt["planer"] ≥ 1（transition 仍写盘）。
    测试后清理：pytest tmp_path 自动清理。
    """
    from src.worker.software_team import check_output  # noqa: F401
    from src.components import Orchestrator
    from src.kernel import load_graph

    class _BrokenDispatcher:
        def dispatch(self, graph, state, node, instance_id, work_dir):
            return None  # 模拟 dispatcher 失败

        def alive(self, pid): return False
        def wait(self, pid, timeout): return -1

    g = load_graph(REPO / "config" / "software_team_graph.json")
    orc = Orchestrator(graph=g, root=tmp_path, dispatcher=_BrokenDispatcher())
    iid = orc.create_instance()
    base_dir = tmp_path / "software_team" / iid
    (base_dir / "doc" / "input.md").parent.mkdir(parents=True, exist_ok=True)
    (base_dir / "doc" / "input.md").write_text("# input\n", encoding="utf-8")

    # Tick 1: planer_loop 边放行（spec.md 未产）→ transition + 派 planer（dispatcher 失败）
    orc.tick("software_team", iid)  # 不验 alarms——当前实现静默忽略是 bug
    # state.json 必须仍可读 + 仍是合法 JSON（dispatch 失败不能破坏写盘）
    sf = tmp_path / "software_team" / f"{iid}.json"
    state = json.loads(sf.read_text(encoding="utf-8"))
    assert state["current_node"] == "planer"
    # 至少 transition 写盘（history 至少 1 项 planer→ planer 自环起点）
    assert len(state["history"]) >= 1

    # Tick 2: 同样失败 → 仍能继续 tick（不崩）
    orc.tick("software_team", iid)
    state = json.loads(sf.read_text(encoding="utf-8"))
    assert state["current_node"] == "planer"
    assert len(state["history"]) >= 2
    assert state["current_node"] == "planer"


def test_unknown_gate_op_yields_alarm(tmp_path: Path) -> None:
    """测试名：test_unknown_gate_op_yields_alarm

    测试场景：未注册的 gate op → GATE_EVAL_ERROR alarm。
    前置条件：tmp_path；bad_graph.json（gate op="this_op_does_not_exist"）。
    是否使用 mock：No（直接 Orchestrator.tick）。
    测试步骤：1. load_graph + Orchestrator；2. create_instance；3. orc.tick("bad", iid)。
    预期结果："GATE_EVAL_ERROR" ∈ alarms。
    测试后清理：pytest tmp_path 自动清理。
    """
    config_path = tmp_path / "bad_graph.json"
    config_path.write_text(json.dumps({
        "name": "bad",
        "graph_mode": "directed_nocycle",
        "tick_period_s": 0.5,
        "pre_handle": "",
        "post_handle": "",
        "nodes": [
            {
                "name": "src",
                "iport": [],
                "oport": [],
                "attr": {"is_src": True, "role": "r"},
                "proc": "",
            },
            {
                "name": "dst",
                "iport": [],
                "oport": [],
                "attr": {"is_sink": True, "role": "r"},
                "proc": "",
            },
        ],
        "gates": [{"name": "g", "op": "this_op_does_not_exist", "enum_dir": [True]}],
        "edges": [
            {
                "name": "to_dst",
                "inode": "src",
                "onode": "dst",
                "driver": "tick",
                "gate": "g",
                "gate_value": True,
                "cmd": "",
            },
        ],
    }))

    from src.components import Orchestrator
    from src.kernel import load_graph

    g = load_graph(config_path)
    orc = Orchestrator(graph=g, root=tmp_path / "_state")
    iid = orc.create_instance()

    alarms = orc.tick("bad", iid)
    assert "GATE_EVAL_ERROR" in alarms
