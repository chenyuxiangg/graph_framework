"""测试 fixture：握手后向 stdout 写非法 UTF-8 帧（验证协议错误）。"""

import json
import sys


def send(kind: str, payload: dict) -> None:
    sys.stdout.write(json.dumps({"v": 1, "kind": kind, "payload": payload}) + "\n")
    sys.stdout.flush()


def main() -> int:
    send("hello", {"protocol_version": 1, "python_version": sys.version.split()[0], "sdk_version": "fixture", "worker_id": "bad-utf8"})
    for _ in range(2):
        line = sys.stdin.readline()
        if not line:
            return 3
        if json.loads(line).get("kind") == "run":
            break
    sys.stdout.buffer.write(b"\xff\xfe\n")
    sys.stdout.buffer.flush()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
