"""qa_reviewer worker：跑 qa_reviewer_proc；产出 qa_pass_1.json。"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from src.components.state_manager import StateManager
from src.kernel import load_graph, resolve_op, state_path
from src.worker import PROC_BYPASS_NAME


def worker_run(args) -> int:
    sop_name = args["sop"]
    instance_id = args["instance"]
    node_name = args["node"]
    proc_name = args["proc"] or PROC_BYPASS_NAME
    base_dir = Path(args["base_dir"])

    graph = load_graph(_find_config(sop_name))
    sf = state_path(sop_name=sop_name, instance_id=instance_id)
    state = StateManager.read(sf)

    # proc 入口：enter_cnt++
    state = StateManager.record_enter(state, node_name)
    StateManager.write(sf, state)

    proc_fn = resolve_op(proc_name)
    try:
        output_files = proc_fn(
            base_dir=base_dir, state=state, graph=graph, node_name=node_name,
        )
    except Exception as exc:
        print(f"proc {proc_name} failed: {exc}", file=sys.stderr)
        return 1
    new_state = StateManager.record_completion(
        state, node_name, output_files=tuple(output_files or ())
    )
    # proc 出口：exit_cnt++
    new_state = StateManager.record_exit(new_state, node_name)
    StateManager.write(sf, new_state)
    return 0


def _find_config(sop_name: str) -> Path:
    base = Path(__file__).resolve().parents[3]
    return base / "config" / f"{sop_name}_graph.json"


def main(argv=None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--sop", required=True)
    parser.add_argument("--instance", required=True)
    parser.add_argument("--node", required=True)
    parser.add_argument("--proc", default="")
    parser.add_argument("--base-dir", required=True)
    ns = parser.parse_args(argv)
    return worker_run(vars(ns))


if __name__ == "__main__":
    sys.exit(main())
