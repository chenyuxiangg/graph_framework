"""graph_base CLI：tick / create / list / show / record-completion / trigger-edge / rollback / validate。"""

from __future__ import annotations

import argparse
import json
import os
import sys
from pathlib import Path

THIS_DIR = Path(__file__).resolve().parent
ROOT = THIS_DIR.parent
SRC = ROOT / "src"
if str(SRC) not in sys.path:
    sys.path.insert(0, str(SRC))
# zlog 包位于 <ROOT>.parent / "zlog"，需把 ROOT.parent（即 scripts/）放入 sys.path
SCRIPTS_DIR = ROOT.parent
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

# 触发 src.worker 自动发现（注册各 SOP 的 gate op / proc / cmd）
import src.worker  # noqa: F401


def cmd_tick(args) -> int:
    from src.components import Orchestrator
    from src.kernel import load_graph

    g = load_graph(_config_path(args.sop))
    root = Path(args.root)
    orc = Orchestrator(graph=g, root=root)
    alarms = orc.tick(args.sop, args.instance)
    print(json.dumps({"alarms": alarms}, ensure_ascii=False))
    return 0


def cmd_create(args) -> int:
    from src.components import Orchestrator
    from src.kernel import load_graph

    g = load_graph(_config_path(args.sop))
    root = Path(args.root)
    orc = Orchestrator(graph=g, root=root)
    iid = orc.create_instance()
    print(json.dumps({"instance_id": iid}, ensure_ascii=False))
    return 0


def cmd_list(args) -> int:
    from src.kernel import read_state, state_path

    root = Path(args.root)
    sop_dir = root / args.sop
    if not sop_dir.is_dir():
        print("[]")
        return 0
    out = []
    for f in sorted(sop_dir.glob("*.json")):
        data = read_state(f)
        out.append({
            "instance_id": f.stem,
            "current_node": data.get("current_node"),
            "history_len": len(data.get("history", [])),
        })
    print(json.dumps(out, ensure_ascii=False, indent=2))
    return 0


def cmd_show(args) -> int:
    from src.components import StateManager
    from src.kernel import state_path

    root = Path(args.root)
    sf = state_path(root=root, sop_name=args.sop, instance_id=args.instance)
    state = StateManager.read(sf)
    print(json.dumps({
        "sop_name": state.sop_name,
        "instance_id": state.instance_id,
        "graph_name": state.graph_name,
        "current_node": state.current_node,
        "history_len": len(state.history),
        "triggered_edges": dict(state.triggered_edges),
        "enter_cnt": dict(state.enter_cnt),
        "exit_cnt": dict(state.exit_cnt),
    }, ensure_ascii=False, indent=2))
    return 0


def cmd_record_completion(args) -> int:
    from src.components import StateManager
    from src.kernel import state_path

    root = Path(args.root)
    sf = state_path(root=root, sop_name=args.sop, instance_id=args.instance)
    state = StateManager.read(sf)
    new_state = StateManager.record_completion(state, args.node, tuple(args.files or ()))
    StateManager.write(sf, new_state)
    print(json.dumps({"ok": True}, ensure_ascii=False))
    return 0


def cmd_trigger_edge(args) -> int:
    from src.components import Orchestrator
    from src.kernel import load_graph

    g = load_graph(_config_path(args.sop))
    root = Path(args.root)
    orc = Orchestrator(graph=g, root=root)
    orc.trigger_edge(args.sop, args.instance, args.edge, args.approver or "anonymous")
    print(json.dumps({"ok": True}, ensure_ascii=False))
    return 0


def cmd_rollback(args) -> int:
    from src.components.state_manager import StateManager
    from src.kernel import state_path

    root = Path(args.root)
    sf = state_path(root=root, sop_name=args.sop, instance_id=args.instance)
    state = StateManager.read(sf)
    new_state = StateManager.rollback(state, args.node, args.reason, base_dir=sf.parent / args.instance)
    StateManager.write(sf, new_state)
    print(json.dumps({"ok": True, "current_node": new_state.current_node}, ensure_ascii=False))
    return 0


def cmd_validate(args) -> int:
    from src.kernel import load_graph, validate

    g = load_graph(Path(args.config))
    issues = validate(g, existing_sop_names=args.existing.split(",") if args.existing else None)
    print(json.dumps(
        [{"code": i.code, "level": i.level, "message": i.message, "path": i.path} for i in issues],
        ensure_ascii=False, indent=2,
    ))
    return 0


def cmd_help(args) -> int:
    print(__doc__)
    return 0


def _config_path(sop_name: str) -> Path:
    return ROOT / "config" / f"{sop_name}_graph.json"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="graph_base_cli")
    sub = parser.add_subparsers(dest="cmd")

    p_tick = sub.add_parser("tick")
    p_tick.add_argument("--sop", required=True)
    p_tick.add_argument("--instance", required=True)
    p_tick.add_argument("--root", required=True)
    p_tick.set_defaults(func=cmd_tick)

    p_create = sub.add_parser("create")
    p_create.add_argument("--sop", required=True)
    p_create.add_argument("--root", required=True)
    p_create.set_defaults(func=cmd_create)

    p_list = sub.add_parser("list")
    p_list.add_argument("--sop", required=True)
    p_list.add_argument("--root", required=True)
    p_list.set_defaults(func=cmd_list)

    p_show = sub.add_parser("show")
    p_show.add_argument("--sop", required=True)
    p_show.add_argument("--instance", required=True)
    p_show.add_argument("--root", required=True)
    p_show.set_defaults(func=cmd_show)

    p_rc = sub.add_parser("record-completion")
    p_rc.add_argument("--sop", required=True)
    p_rc.add_argument("--instance", required=True)
    p_rc.add_argument("--node", required=True)
    p_rc.add_argument("--files", nargs="*", default=[])
    p_rc.add_argument("--root", required=True)
    p_rc.set_defaults(func=cmd_record_completion)

    p_te = sub.add_parser("trigger-edge")
    p_te.add_argument("--sop", required=True)
    p_te.add_argument("--instance", required=True)
    p_te.add_argument("--edge", required=True)
    p_te.add_argument("--approver", default="anonymous")
    p_te.add_argument("--root", required=True)
    p_te.set_defaults(func=cmd_trigger_edge)

    p_rb = sub.add_parser("rollback")
    p_rb.add_argument("--sop", required=True)
    p_rb.add_argument("--instance", required=True)
    p_rb.add_argument("--node", required=True)
    p_rb.add_argument("--reason", default="manual")
    p_rb.add_argument("--root", required=True)
    p_rb.set_defaults(func=cmd_rollback)

    p_v = sub.add_parser("validate")
    p_v.add_argument("--config", required=True)
    p_v.add_argument("--existing", default="")
    p_v.set_defaults(func=cmd_validate)

    sub.add_parser("help").set_defaults(func=cmd_help)

    ns = parser.parse_args(argv)
    if not ns.cmd:
        return cmd_help(None)
    return ns.func(ns)


if __name__ == "__main__":
    sys.exit(main())