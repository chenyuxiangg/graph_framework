"""测试 fixture：写两个 result 帧（验证重复 result 检测）。"""

import json
import sys


def send(kind: str, payload: dict) -> None:
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    send("hello", {"protocol_version": 1, "python_version": sys.version.split()[0], "sdk_version": "fixture", "worker_id": "dup"})
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        frame = json.loads(line)
        if frame.get("kind") == "run":
            break
    send("result", {"output": {"first": True}})
    send("result", {"output": {"duplicate": True}})
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
