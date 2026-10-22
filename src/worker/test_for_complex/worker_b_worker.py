"""worker_b worker：跑 worker_b_proc。"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

from src.worker._common import record_lifecycle


def worker_run(args) -> int:
    return record_lifecycle(
        sop_name=args["sop"],
        instance_id=args["instance"],
        node_name=args["node"],
        proc_name=args["proc"],
        base_dir=Path(args["base_dir"]),
    )


def main(argv=None):
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