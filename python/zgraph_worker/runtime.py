"""worker 运行时：握手 → 等待 run → 调用用户函数 → 写 result/error 后退出。

入口：``python -m zgraph_worker.runtime``（协议见 ``schemas/worker-protocol-v1.md``）。
"""

from __future__ import annotations

import importlib
import os
import random
import sys
import threading
import traceback
from queue import Queue
from typing import Any, BinaryIO, Callable

from .ctx import WorkerContext
from .errors import (
    CODE_IMPORT,
    CODE_PROTOCOL,
    WorkerCancelled,
    WorkerError,
    WorkerImportError,
    ReturnNotDictError,
)
from .protocol import SDK_VERSION, ProtocolError, read_frame, write_frame

PROTOCOL_VERSION_NUMBER: int = 1
EXIT_OK: int = 0
EXIT_ERROR: int = 2
EXIT_PROTOCOL_ERROR: int = 3


class _DisabledStdin:
    """禁用 ``input()``：stdin 是 worker 协议通道，用户代码不得读取。"""

    def read(self, *args: object, **kwargs: object) -> str:
        raise RuntimeError("input() is disabled: stdin carries the worker protocol")

    def readline(self, *args: object, **kwargs: object) -> str:
        raise RuntimeError("input() is disabled: stdin carries the worker protocol")


def _isolate_stdio() -> int:
    """保存协议 fd，把用户 stdout 重定向到 stderr。"""
    protocol_fd: int = os.dup(1)
    os.set_inheritable(protocol_fd, False)
    sys.stdout = sys.stderr
    return protocol_fd


def _seed_rng(seed: int) -> None:
    """按 seed 初始化 random（以及可选的 numpy）。"""
    random.seed(seed)
    try:
        import numpy as np  # type: ignore[import-not-found]
    except ImportError:
        return
    np.random.seed(seed % (2**32))


def _load_callable(module_name: str, function_name: str) -> Callable[..., Any]:
    """导入 ``module_name.function_name`` 并确认可调用。"""
    try:
        module = importlib.import_module(module_name)
        func = getattr(module, function_name)
    except Exception as exc:  # noqa: BLE001 - 统一转协议错误
        raise WorkerImportError(f"import {module_name}.{function_name}: {exc}") from exc
    if not callable(func):
        raise WorkerImportError(f"{module_name}.{function_name} is not callable")
    return func


def _cancel_reader(stream: BinaryIO, cancel: threading.Event, runs: Queue[dict[str, Any]]) -> None:
    """后台读 stdin：处理 cancel 与 run 帧（避免阻塞同步用户函数）。"""
    while True:
        try:
            frame: dict[str, Any] = read_frame(stream)
        except Exception:  # noqa: BLE001 - EOF/非法帧即结束
            return
        kind: str = frame["kind"]
        if kind == "cancel":
            cancel.set()
        elif kind == "run":
            runs.put(frame)


def _error_payload(code: str, message: str, tb: str | None) -> dict[str, Any]:
    """构造结构化错误载荷。"""
    payload: dict[str, Any] = {"code": code, "message": message}
    if tb is not None:
        payload["traceback"] = tb
    return payload


def _build_context(
    protocol_fd: int,
    payload: dict[str, Any],
    cancel: threading.Event,
) -> WorkerContext:
    """由 run 载荷构造 WorkerContext。"""
    return WorkerContext(
        run_id=str(payload.get("run_id", "")),
        instance_id=str(payload.get("instance_id", "")),
        node_id=str(payload.get("node_id", "")),
        attempt=int(payload.get("attempt", 0)),
        user_id=str(payload.get("user_id", "local")),
        global_vars=payload.get("global_vars") or {},
        params=payload.get("params") or {},
        deadline_ms=int(payload.get("deadline_ms", 0)),
        seed=int(payload.get("seed", 0)),
        emit_sink=lambda event: write_frame(protocol_fd, "emit", event),
        log_sink=lambda event: write_frame(protocol_fd, "log", event),
        is_cancelled=cancel.is_set,
    )


def _execute(protocol_fd: int, frame: dict[str, Any], cancel: threading.Event) -> int:
    """执行一次 run，返回进程退出码。"""
    payload: dict[str, Any] = frame["payload"]
    seed: int = int(payload.get("seed", 0))
    _seed_rng(seed)
    ctx: WorkerContext = _build_context(protocol_fd, payload, cancel)
    try:
        func = _load_callable(str(payload["module"]), str(payload["function"]))
        output: Any = func(payload.get("inputs") or {}, ctx)
        if not isinstance(output, dict):
            raise ReturnNotDictError(
                f"run() must return dict, got {type(output).__name__}",
                details={"expected": "dict"},
            )
        write_frame(protocol_fd, "result", {"output": output})
        return EXIT_OK
    except WorkerCancelled as exc:
        write_frame(protocol_fd, "error", _error_payload(exc.code, exc.message, None))
        return EXIT_ERROR
    except WorkerError as exc:
        tb: str = traceback.format_exc() if exc.code == CODE_IMPORT else ""
        write_frame(protocol_fd, "error", _error_payload(exc.code, exc.message, tb or None))
        return EXIT_ERROR
    except ProtocolError as exc:
        write_frame(protocol_fd, "error", _error_payload(CODE_PROTOCOL, str(exc), None))
        return EXIT_PROTOCOL_ERROR
    except Exception as exc:  # noqa: BLE001 - 用户代码任意异常
        tb = traceback.format_exc()
        write_frame(protocol_fd, "error", _error_payload("NODE_PYTHON_EXCEPTION", str(exc), tb))
        return EXIT_ERROR


def main() -> int:
    """worker 主流程：hello → hello_ack → 等 run → 执行 → 退出。"""
    protocol_fd: int = _isolate_stdio()
    hello: dict[str, Any] = {
        "protocol_version": PROTOCOL_VERSION_NUMBER,
        "python_version": sys.version.split()[0],
        "sdk_version": SDK_VERSION,
        "worker_id": f"pid-{os.getpid()}",
    }
    write_frame(protocol_fd, "hello", hello)

    stdin: BinaryIO = sys.stdin.buffer
    try:
        ack: dict[str, Any] = read_frame(stdin)
    except Exception:  # noqa: BLE001 - 握手失败直接退出
        return EXIT_PROTOCOL_ERROR
    if ack["kind"] != "hello_ack":
        return EXIT_PROTOCOL_ERROR

    cancel: threading.Event = threading.Event()
    runs: Queue[dict[str, Any]] = Queue(maxsize=1)
    threading.Thread(target=_cancel_reader, args=(stdin, cancel, runs), daemon=True).start()
    sys.stdin = _DisabledStdin()  # type: ignore[assignment]

    frame: dict[str, Any] = runs.get()
    return _execute(protocol_fd, frame, cancel)


def _shutdown(code: int) -> None:
    """flush stderr 后强制退出（跳过 atexit/sys.exit；见协议文档 §4 的退出约定）。"""
    try:
        sys.stderr.flush()
    finally:
        os._exit(code)


if __name__ == "__main__":
    # 后台 cancel reader 线程会阻塞在 stdin 读取；用 os._exit 跳过解释器 shutdown，
    # 避免 "could not acquire lock for BufferedReader at interpreter shutdown" 致命错误。
    _shutdown(main())
