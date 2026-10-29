"""测试 fixture：握手/接收 run 后直接 exit 0，不写 result（验证 exit-0-无-result）。"""

import json
import sys


def send(kind: str, payload: dict) -> None:
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    send("hello", {"protocol_version": 1, "python_version": sys.version.split()[0], "sdk_version": "fixture", "worker_id": "no-result"})
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        frame = json.loads(line)
        if frame.get("kind") == "run":
            break
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
