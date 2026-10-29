"""supervisor 握手 fixture：hello 信封版本不匹配（协议 §2：拒绝并杀进程组）。"""

import json
import os
import sys
import time


def main() -> int:
    """写 hello（protocol_version=9）后长睡，等待 supervisor 拒绝并 kill。"""
    pid_file = os.environ.get("ZGRAPH_FIXTURE_PID_FILE")
    if pid_file:
        with open(pid_file, "w", encoding="utf-8") as handle:
            handle.write(str(os.getpid()))
    hello = {
        "protocol_version": 9,
        "python_version": sys.version.split()[0],
        "sdk_version": "fixture",
        "worker_id": "bad-version",
    }
    sys.stdout.write(json.dumps({"v": 1, "kind": "hello", "payload": hello}) + "\n")
    sys.stdout.flush()
    time.sleep(60)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
