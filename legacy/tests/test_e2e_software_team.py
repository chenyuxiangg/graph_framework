"""e2e 测试：CLI create → tick → 写 input → tick → 等 worker → tick 全链路。"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

import pytest

REPO = Path(__file__).resolve().parent.parent  # scripts/graph_base


def _run_cli(*args: str) -> dict:
    """运行 graph_base_cli 子命令，解析 JSON 输出。"""
    result = subprocess.run(
        ["python3", "scripts/graph_base_cli.py", *args],
        cwd=REPO, capture_output=True, text=True, timeout=30,
        env={**os.environ, "PYTHONPATH": str(REPO)},
    )
    assert result.returncode == 0, f"stderr: {result.stderr}"
    return json.loads(result.stdout)


def test_create_then_tick_awaits_iport(tmp_path: Path) -> None:
    """测试名：test_create_then_tick_awaits_iport

    测试场景：create 后立刻 tick → iport 缺失 → 留在 planer。
    前置条件：tmp_path；REPO/config/software_team_graph.json。
    是否使用 mock：No（subprocess 调真实 CLI）。
    测试步骤：1. CLI create；2. CLI tick；3. 读 state。
    预期结果：state["current_node"] == "planer"（未推进）。
    测试后清理：pytest tmp_path 自动清理。
    """
    create = _run_cli(
        "create", "--sop", "software_team", "--root", str(tmp_path),
    )
    iid = create["instance_id"]
    sf = tmp_path / "software_team" / f"{iid}.json"
    assert sf.exists()

    _run_cli("tick", "--sop", "software_team",
             "--instance", iid, "--root", str(tmp_path))
    state = json.loads(sf.read_text())
    assert state["current_node"] == "planer"


def test_full_tick_dispatches_planer(tmp_path: Path) -> None:
    """测试名：test_full_tick_dispatches_planer

    测试场景：写 iport → tick 派 planer → spec.md 出现 → 需求评审（manual trigger）
    → 再 tick 推进到 archer。

    新语义：单 tick 在 planer_loop 边（gate_value=False）下不会 advance，会派 planer worker；
    spec.md 写出后 gate 翻 true，但 planer_to_archer 是 **manual 边**（需求评审），
    必须先 trigger-edge 放行，tick 才会 advance 到 archer。

    这条 manual 边是本用例比改图前多覆盖的东西：manual 边不被 trigger 时 tick 停在 planer。
    前置条件：tmp_path；REPO/config/software_team_graph.json。
    是否使用 mock：No（subprocess 调真实 CLI）。
    测试步骤：1. CLI create；2. 写 input.md；3. tick1（派 planer）；4. 等 spec.md；
      5. tick2（未 trigger，应仍停在 planer）；6. trigger-edge planer_to_archer；
      7. tick3（推进到 archer）；8. 等 last_output["archer"]=2 文件。
    预期结果：step5 后 current_node=="planer"；最终 current_node=="archer"；
      archer 产物 2 个文件存在。
    测试后清理：pytest tmp_path 自动清理。
    """
    create = _run_cli(
        "create", "--sop", "software_team", "--root", str(tmp_path),
    )
    iid = create["instance_id"]
    sf = tmp_path / "software_team" / f"{iid}.json"
    base_dir = sf.parent / iid

    input_md = base_dir / "doc" / "input.md"
    input_md.parent.mkdir(parents=True, exist_ok=True)
    input_md.write_text("# input\n", encoding="utf-8")

    # Tick 1: planer_loop 边放行（gate_value=False，spec.md 还没产）→ 派 planer worker
    _run_cli("tick", "--sop", "software_team",
             "--instance", iid, "--root", str(tmp_path))

    # 等 planer worker 写出 spec.md
    deadline = time.time() + 10
    while time.time() < deadline:
        if (base_dir / "doc" / "specification.md").exists():
            break
        time.sleep(0.1)
    assert (base_dir / "doc" / "specification.md").exists(), "planer worker 写出 spec.md"

    # Tick 2: gate 已翻 true，但 planer_to_archer 是 manual 边未 trigger → 停在 planer
    _run_cli("tick", "--sop", "software_team",
             "--instance", iid, "--root", str(tmp_path))
    assert json.loads(sf.read_text())["current_node"] == "planer", (
        "manual 边未 trigger 时不应推进到 archer"
    )

    # 需求评审：放行 planer_to_archer
    _run_cli("trigger-edge", "--sop", "software_team", "--instance", iid,
             "--edge", "planer_to_archer", "--approver", "pm",
             "--root", str(tmp_path))

    # Tick 3: planer_to_archer 放行（gate_value=True）→ advance + 派 archer worker
    _run_cli("tick", "--sop", "software_team",
             "--instance", iid, "--root", str(tmp_path))

    # 等 archer worker 写出 arch outputs
    deadline = time.time() + 10
    while time.time() < deadline:
        state = json.loads(sf.read_text())
        if state["current_node"] == "archer":
            break
        time.sleep(0.1)
    state = json.loads(sf.read_text())
    assert state["current_node"] == "archer", f"state: {state}"
    assert len(state["last_output"].get("archer", [])) == 2
    assert (base_dir / "doc" / "architectural_design.md").exists()
    assert (base_dir / "doc" / "organizational_structure.md").exists()


def test_sop_continues_past_archer_midchain(tmp_path: Path) -> None:
    """测试名：test_sop_continues_past_archer_midchain

    测试场景：推进到 archer 后 SOP **不**终止——archer 在 12 节点图里是中途节点，
    不是 sink。真正 sink 是 release_done（需一路 trigger 5 道人工边才能到）。

    改图前本用例叫 test_sop_terminates_after_archer，断言 archer 是 sink。
    图扩到 12 节点后该前提消失：archer 的出边 archer_to_mde / archer_to_te 都是
    manual 边（设计评审），不 trigger 就停在 archer——SOP 既不终止也不推进。

    覆盖的仍是终止判定逻辑：is_sop_done 只在 is_sink 节点为真，中途节点永远不 done。
    前置条件：tmp_path；REPO/config/software_team_graph.json。
    是否使用 mock：No（subprocess 调真实 CLI）。
    测试步骤：1. create + 写 input；2. tick1 → 等 spec；3. trigger planer_to_archer；
      4. tick2 → 等 archer；5. 等 archer 产物；6. 再 tick 3 次（不 trigger 设计评审）；
      7. 验证 current_node 仍是 archer、无新 history。
    预期结果：archer 产物已落盘；last_output["archer"] 长度 == 2；
      current_node 保持 "archer"（manual 边未放行，不推进也不终止）；
      history 末项为 ("planer", ts)；后续 tick 不产生新 history。
    测试后清理：pytest tmp_path 自动清理。
    """
    create = _run_cli(
        "create", "--sop", "software_team", "--root", str(tmp_path),
    )
    iid = create["instance_id"]
    sf = tmp_path / "software_team" / f"{iid}.json"
    base_dir = sf.parent / iid

    input_md = base_dir / "doc" / "input.md"
    input_md.parent.mkdir(parents=True, exist_ok=True)
    input_md.write_text("# input\n", encoding="utf-8")

    # Tick 1: 派 planer worker
    _run_cli("tick", "--sop", "software_team",
             "--instance", iid, "--root", str(tmp_path))
    # 等 spec.md 出现
    deadline = time.time() + 10
    while time.time() < deadline:
        if (base_dir / "doc" / "specification.md").exists():
            break
        time.sleep(0.1)
    # 需求评审放行 + Tick 2: advance 到 archer
    _run_cli("trigger-edge", "--sop", "software_team", "--instance", iid,
             "--edge", "planer_to_archer", "--approver", "pm",
             "--root", str(tmp_path))
    _run_cli("tick", "--sop", "software_team",
             "--instance", iid, "--root", str(tmp_path))
    deadline = time.time() + 10
    while time.time() < deadline:
        s = json.loads(sf.read_text())
        if s["current_node"] == "archer":
            break
        time.sleep(0.1)

    # 等 archer worker 落盘产物（CLI 异步 worker）
    deadline = time.time() + 10
    while time.time() < deadline:
        if (base_dir / "doc" / "architectural_design.md").exists() \
                and (base_dir / "doc" / "organizational_structure.md").exists():
            break
        time.sleep(0.1)

    final = json.loads(sf.read_text())
    # archer 是中途节点：产物落盘但 SOP 未终止
    assert final["current_node"] == "archer"
    assert len(final["last_output"].get("archer", [])) == 2, (
        f"archer 产物未落盘，state={final}"
    )
    # 不变量：history 末项是 planer→archer 那一跳的 from_node
    assert final["history"][-1][0] == "planer"

    # 设计评审（archer_to_mde / archer_to_te）未 trigger → 连 tick 3 次都不推进
    history_len_before = len(final["history"])
    for _ in range(3):
        _run_cli("tick", "--sop", "software_team",
                 "--instance", iid, "--root", str(tmp_path))
    after = json.loads(sf.read_text())
    assert after["current_node"] == "archer", (
        f"设计评审未 trigger 时不该离开 archer，state={after}"
    )
    assert len(after["history"]) == history_len_before, (
        f"未 trigger 的 manual 边不应推进（却产生了新 history）：{after['history']}"
    )


def test_list_shows_instance(tmp_path: Path) -> None:
    """测试名：test_list_shows_instance

    测试场景：CLI list 能列出刚 create 的 instance。
    前置条件：tmp_path；REPO/config/software_team_graph.json。
    是否使用 mock：No（subprocess 调真实 CLI）。
    测试步骤：1. CLI create；2. CLI list；3. 断言 list 中含该 iid。
    预期结果：list 任一 item["instance_id"] == iid。
    测试后清理：pytest tmp_path 自动清理。
    """
    create = _run_cli(
        "create", "--sop", "software_team", "--root", str(tmp_path),
    )
    iid = create["instance_id"]
    listed = _run_cli(
        "list", "--sop", "software_team", "--root", str(tmp_path),
    )
    assert any(item["instance_id"] == iid for item in listed)


def test_show_displays_state(tmp_path: Path) -> None:
    """测试名：test_show_displays_state

    测试场景：CLI show 返回 instance 当前 state 摘要。
    前置条件：tmp_path；REPO/config/software_team_graph.json。
    是否使用 mock：No（subprocess 调真实 CLI）。
    测试步骤：1. CLI create；2. CLI show。
    预期结果：show["instance_id"]==iid；show["current_node"]=="planer"；show["history_len"]==0。
    测试后清理：pytest tmp_path 自动清理。
    """
    create = _run_cli(
        "create", "--sop", "software_team", "--root", str(tmp_path),
    )
    iid = create["instance_id"]
    show = _run_cli(
        "show", "--sop", "software_team",
        "--instance", iid, "--root", str(tmp_path),
    )
    assert show["instance_id"] == iid
    assert show["current_node"] == "planer"
    assert show["history_len"] == 0


def test_trigger_edge_writes_approver(tmp_path: Path) -> None:
    """测试名：test_trigger_edge_writes_approver

    测试场景：trigger-edge 把 approver 写进 state.triggered_edges。
    前置条件：tmp_path；REPO/config/software_team_graph.json。
    是否使用 mock：No（subprocess 调真实 CLI）。
    测试步骤：1. CLI create；2. CLI trigger-edge --approver alice；3. 读 state。
    预期结果：state["triggered_edges"]["planer_to_archer"] == "alice"。
    测试后清理：pytest tmp_path 自动清理。
    """
    create = _run_cli(
        "create", "--sop", "software_team", "--root", str(tmp_path),
    )
    iid = create["instance_id"]
    _run_cli(
        "trigger-edge", "--sop", "software_team",
        "--instance", iid, "--edge", "planer_to_archer",
        "--approver", "alice", "--root", str(tmp_path),
    )
    sf = tmp_path / "software_team" / f"{iid}.json"
    state = json.loads(sf.read_text())
    assert state["triggered_edges"]["planer_to_archer"] == "alice"


def test_rollback_clears_triggered_and_advances_to_prev(tmp_path: Path) -> None:
    """测试名：test_rollback_clears_triggered_and_advances_to_prev

    测试场景：rollback 把 current_node 回到 history 上一节点 + 清 triggered_edges。
    前置条件：tmp_path；REPO/config/software_team_graph.json；写 input.md。
    是否使用 mock：No（subprocess 调真实 CLI）。
    测试步骤：1. CLI create；2. 写 input.md；3. tick → 等 archer；4. CLI trigger-edge + rollback；
      5. 读 state。
    预期结果：current_node is None（planer 在 history 首项，前无节点）；triggered_edges 清空。
    测试后清理：pytest tmp_path 自动清理。
    """
    create = _run_cli(
        "create", "--sop", "software_team", "--root", str(tmp_path),
    )
    iid = create["instance_id"]
    sf = tmp_path / "software_team" / f"{iid}.json"
    base_dir = sf.parent / iid

    input_md = base_dir / "doc" / "input.md"
    input_md.parent.mkdir(parents=True, exist_ok=True)
    input_md.write_text("# input\n", encoding="utf-8")

    # tick → 推到 archer
    _run_cli("tick", "--sop", "software_team",
             "--instance", iid, "--root", str(tmp_path))
    deadline = time.time() + 10
    while time.time() < deadline:
        s = json.loads(sf.read_text())
        if s["current_node"] == "archer":
            break
        time.sleep(0.1)

    # 写一个 trigger + rollback
    _run_cli(
        "trigger-edge", "--sop", "software_team",
        "--instance", iid, "--edge", "planer_to_archer",
        "--approver", "bob", "--root", str(tmp_path),
    )
    _run_cli(
        "rollback", "--sop", "software_team",
        "--instance", iid, "--node", "planer", "--reason", "manual",
        "--root", str(tmp_path),
    )
    after = json.loads(sf.read_text())
    # planer 在 history[-1][0]，但它是 src 的前一项 → current = None
    assert after["current_node"] is None
    assert dict(after["triggered_edges"]) == {}  # 清空
