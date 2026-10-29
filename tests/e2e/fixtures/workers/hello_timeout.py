"""握手超时 fixture（raw NDJSON，绕过 SDK）：启动后**不发 hello**，直接长睡。

supervisor 必须在 `hello_timeout` 内判 `NODE_WORKER_HANDSHAKE` 并 SIGKILL 进程组
（协议 §2 / §8.2）。
"""

import time


def main() -> int:
    """不写任何帧（也不写 hello），等待 supervisor 握手超时后 kill。"""
    time.sleep(60)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
