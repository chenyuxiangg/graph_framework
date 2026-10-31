"""``python/workers`` 示例 worker 单测 + ``zgraph_worker.runtime`` 子进程端到端（M3 / REV-013）。

覆盖：全部示例 worker 的直接调用与 L3 契约（ISSUE-022）、fixture 图模块名/函数名对齐、
SDK 签名约束（spec 04 §4.3.5），以及 echo / parallel / cycle / software_team 四条真实
子进程 NDJSON 往返。
"""

from __future__ import annotations

import importlib
import inspect
import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path
from typing import Any, Callable

import pytest

from workers._inputs import unwrap_port
from workers.cycle import FIRST_ITERATION, RETRY_ITERATION, looping
from workers.echo import run as echo_run
from workers.parallel import PARALLEL_SLEEP_S, a as parallel_a, b as parallel_b
from workers.slow import CHECK_INTERVAL_S, DEFAULT_SECONDS, run as slow_run
from workers.software_team.archer import run as archer_run
from workers.software_team.parse_verdict import run as parse_verdict_run
from workers.software_team.planer import run as planer_run
from zgraph_worker.ctx import WorkerContext
from zgraph_worker.errors import WorkerCancelled
from zgraph_worker.protocol import encode_frame

PYTHON_DIR: Path = Path(__file__).resolve().parents[1]
REPO_ROOT: Path = PYTHON_DIR.parent

WORKER_MODULES: tuple[str, ...] = (
    "workers",
    "workers.echo",
    "workers.slow",
    "workers.parallel",
    "workers.cycle",
    "workers.software_team",
    "workers.software_team.planer",
    "workers.software_team.archer",
    "workers.software_team.parse_verdict",
)

# (module, function, 可调用对象)：与 fixture 图 config 及包 __init__ 清单一致。
WORKER_CALLABLES: tuple[tuple[str, str, Callable[..., Any]], ...] = (
    ("workers.echo", "run", echo_run),
    ("workers.slow", "run", slow_run),
    ("workers.parallel", "a", parallel_a),
    ("workers.parallel", "b", parallel_b),
    ("workers.cycle", "looping", looping),
    ("workers.software_team.planer", "run", planer_run),
    ("workers.software_team.archer", "run", archer_run),
    ("workers.software_team.parse_verdict", "run", parse_verdict_run),
)

L3_DOCSTRING_MARKERS: tuple[str, ...] = ("输入：", "输出：", "幂等性：", "外部副作用：")

# 与示例模块名绑定的 fixture 图（e2e 模块所有；此处只读用于对齐断言）。
FIXTURE_GRAPHS: tuple[str, ...] = (
    "tests/fixtures/graphs/valid_parallel.json",
    "tests/fixtures/graphs/valid_cycle.json",
)


def _ctx(
    emitted: list[dict[str, Any]] | None = None,
    cancel: threading.Event | None = None,
) -> WorkerContext:
    """构造真实 WorkerContext（sink 指向本地列表/空实现）。"""
    return WorkerContext(
        run_id="r",
        instance_id="i",
        node_id="n",
        attempt=0,
        user_id="u",
        global_vars={},
        params={},
        deadline_ms=0,
        seed=7,
        emit_sink=(emitted if emitted is not None else []).append,
        log_sink=lambda _event: None,
        is_cancelled=(cancel if cancel is not None else threading.Event()).is_set,
    )


def test_example_modules_are_importable() -> None:
    for module_name in WORKER_MODULES:
        assert importlib.import_module(module_name).__name__ == module_name


def test_fixture_graph_python_node_symbols_are_importable() -> None:
    # 图里的 module/function 必须能按 PYTHONPATH=python 直接导入/取属性（D7：运行期校验）。
    for relative in FIXTURE_GRAPHS:
        graph: dict[str, Any] = json.loads((REPO_ROOT / relative).read_text(encoding="utf-8"))
        symbols: list[tuple[str, str]] = [
            (node["config"]["module"], node["config"]["function"])
            for node in graph["nodes"]
            if node["type"] == "python_node"
        ]
        assert symbols, f"{relative} 没有 python_node"
        for module_name, function_name in symbols:
            module = importlib.import_module(module_name)
            assert callable(getattr(module, function_name)), (
                f"{relative}: {module_name}.{function_name} 不可调用"
            )


def test_worker_functions_are_module_level_sync_with_two_params() -> None:
    for module_name, function_name, func in WORKER_CALLABLES:
        module = importlib.import_module(module_name)
        candidate = getattr(module, function_name)
        assert inspect.isfunction(candidate), f"{module_name}.{function_name} 必须是模块级函数"
        assert not inspect.iscoroutinefunction(candidate)
        params = list(inspect.signature(candidate).parameters.values())
        assert len(params) == 2, f"{module_name}.{function_name} 必须恰好 2 个参数"
        for param in params:
            assert param.kind in (
                inspect.Parameter.POSITIONAL_ONLY,
                inspect.Parameter.POSITIONAL_OR_KEYWORD,
            )
            assert param.default is inspect.Parameter.empty
        assert func is candidate


def test_worker_docstrings_declare_l3_contract() -> None:
    for module_name, function_name, func in WORKER_CALLABLES:
        docstring: str = func.__doc__ or ""
        for marker in L3_DOCSTRING_MARKERS:
            assert marker in docstring, f"{module_name}.{function_name} 缺少 L3 声明 {marker}"
        side_effect_decl: str = docstring.split("外部副作用：", 1)[1].strip()
        assert side_effect_decl.startswith(("无", "1)")), (
            f"{module_name}.{function_name} 的副作用声明必须显式以 无/1) 开头"
        )


def test_echo_run_returns_message_and_all_inputs() -> None:
    emitted: list[dict[str, Any]] = []
    output: dict[str, Any] = echo_run({"message": "hi", "extra": 1}, _ctx(emitted))
    assert output["echo"] == "hi"
    assert output["inputs"] == {"message": "hi", "extra": 1}
    assert output["node_id"] == "n"
    assert emitted[0]["type"] == "progress"


def test_slow_run_sleeps_requested_seconds() -> None:
    start: float = time.monotonic()
    output: dict[str, Any] = slow_run({"seconds": 0.02}, _ctx())
    elapsed: float = time.monotonic() - start
    assert output == {"slept": 0.02, "cancelled": False}
    assert elapsed >= 0.02
    assert output["cancelled"] is False


def test_slow_run_uses_default_seconds_when_absent(monkeypatch: pytest.MonkeyPatch) -> None:
    # 默认 1s 太慢：只在测试内把模块常量调小，验证「缺省走 DEFAULT_SECONDS」路径。
    monkeypatch.setattr("workers.slow.DEFAULT_SECONDS", 0.01)
    output: dict[str, Any] = slow_run({}, _ctx())
    assert output["slept"] == 0.01
    assert DEFAULT_SECONDS == 1.0
    assert CHECK_INTERVAL_S == 0.05


def test_slow_run_writes_pid_file_when_requested(tmp_path: Path) -> None:
    pid_file: Path = tmp_path / "worker.pid"
    slow_run({"seconds": 0.0, "pid_file": str(pid_file)}, _ctx())
    assert pid_file.read_text(encoding="utf-8") == str(os.getpid())


def test_slow_run_reads_seconds_from_upstream_output_package() -> None:
    output: dict[str, Any] = slow_run({"seconds": {"seconds": 0.02, "ignored": True}}, _ctx())
    assert output["slept"] == 0.02


def test_slow_run_raises_worker_cancelled() -> None:
    cancel: threading.Event = threading.Event()
    cancel.set()
    start: float = time.monotonic()
    with pytest.raises(WorkerCancelled):
        slow_run({"seconds": 5.0}, _ctx(cancel=cancel))
    assert time.monotonic() - start < 1.0


def test_parallel_branches_return_marker_and_sleep() -> None:
    for func, marker in ((parallel_a, "a"), (parallel_b, "b")):
        start: float = time.monotonic()
        output: dict[str, Any] = func({"x": "value"}, _ctx())
        elapsed: float = time.monotonic() - start
        assert output == {"worker": marker, "x": "value"}
        assert elapsed >= PARALLEL_SLEEP_S


def test_cycle_looping_first_iteration_requests_retry() -> None:
    output: dict[str, Any] = looping({"seed": {"seed": "s"}, "again": None}, _ctx())
    assert output == {"continue": True, "iteration": FIRST_ITERATION, "seed": {"seed": "s"}}


def test_cycle_looping_retry_iteration_exits() -> None:
    first: dict[str, Any] = looping({"seed": {"seed": "s"}, "again": None}, _ctx())
    second: dict[str, Any] = looping({"seed": {"seed": "s"}, "again": first}, _ctx())
    assert second["continue"] is False
    assert second["iteration"] == RETRY_ITERATION
    assert second["seed"] == {"seed": "s"}


def test_planer_returns_default_document() -> None:
    output: dict[str, Any] = planer_run({}, _ctx())
    assert output == {
        "documents": {"doc/specification.md": "# specification (stub)\n"},
        "document_paths": ["doc/specification.md"],
    }


def test_planer_uses_custom_title_and_path() -> None:
    output: dict[str, Any] = planer_run(
        {"specification_path": "out/spec.md", "specification_title": "需求"},
        _ctx(),
    )
    assert output["documents"] == {"out/spec.md": "# 需求 (stub)\n"}
    assert output["document_paths"] == ["out/spec.md"]


def test_planer_reads_upstream_output_package_fields() -> None:
    output: dict[str, Any] = planer_run(
        {"specification_title": {"specification_title": "from-upstream", "other": 1}},
        _ctx(),
    )
    assert output["documents"]["doc/specification.md"] == "# from-upstream (stub)\n"


def test_archer_returns_two_documents() -> None:
    output: dict[str, Any] = archer_run({}, _ctx())
    assert output["documents"] == {
        "doc/architectural_design.md": "# architectural_design (stub)\n",
        "doc/organizational_structure.md": "# organizational_structure (stub)\n",
    }
    assert output["document_paths"] == [
        "doc/architectural_design.md",
        "doc/organizational_structure.md",
    ]


def test_archer_uses_custom_paths() -> None:
    output: dict[str, Any] = archer_run({"design_path": "a.md", "org_path": "b.md"}, _ctx())
    assert output["document_paths"] == ["a.md", "b.md"]


def test_unwrap_port_returns_field_from_upstream_package() -> None:
    assert unwrap_port({"x": {"x": 42, "y": 1}}, "x", "x") == 42


def test_unwrap_port_returns_value_when_not_package() -> None:
    assert unwrap_port({"x": 42}, "x", "x") == 42
    assert unwrap_port({"x": {"y": 1}}, "x", "x") == {"y": 1}
    assert unwrap_port({}, "x", "x") is None


def _runtime_env() -> dict[str, str]:
    """子进程环境：``workers`` 包与 SDK 均从仓库 ``python/`` 导入。"""
    env: dict[str, str] = dict(os.environ)
    env["PYTHONPATH"] = str(PYTHON_DIR)
    env["PYTHONHASHSEED"] = "0"
    return env


def _spawn_runtime() -> subprocess.Popen[bytes]:
    proc: subprocess.Popen[bytes] = subprocess.Popen(
        [sys.executable, "-m", "zgraph_worker.runtime"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=REPO_ROOT,
        env=_runtime_env(),
    )

    def _drain() -> None:
        # 必须持续排空 stderr，否则 pipe 满会阻塞 worker（协议 §4.2）。
        assert proc.stderr is not None
        while proc.stderr.readline():
            continue

    threading.Thread(target=_drain, daemon=True).start()
    return proc


def _readline(proc: subprocess.Popen[bytes]) -> dict[str, Any]:
    assert proc.stdout is not None
    line: bytes = proc.stdout.readline()
    assert line, "worker closed stdout without a frame"
    return json.loads(line)


def _run_frame(module: str, function: str, inputs: dict[str, Any]) -> dict[str, Any]:
    return {
        "v": 1,
        "kind": "run",
        "payload": {
            "run_id": "r",
            "instance_id": "i",
            "node_id": "n",
            "attempt": 0,
            "module": module,
            "function": function,
            "inputs": inputs,
            "params": {},
            "user_id": "u",
            "global_vars": {},
            "seed": 0,
            "deadline_ms": 0,
            "trace_id": "t",
        },
    }


def _execute_in_runtime(
    module: str, function: str, inputs: dict[str, Any]
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """真实子进程：hello → hello_ack → run → 收集到 result/error 后返回。"""
    proc: subprocess.Popen[bytes] = _spawn_runtime()
    try:
        hello: dict[str, Any] = _readline(proc)
        assert hello["kind"] == "hello"
        assert hello["payload"]["protocol_version"] == 1
        assert proc.stdin is not None
        proc.stdin.write(encode_frame("hello_ack", {"accepted": True}))
        proc.stdin.write(json.dumps(_run_frame(module, function, inputs)).encode("utf-8") + b"\n")
        proc.stdin.flush()
        frames: list[dict[str, Any]] = []
        while True:
            frame: dict[str, Any] = _readline(proc)
            frames.append(frame)
            if frame["kind"] in {"result", "error"}:
                break
        assert frames[-1]["kind"] == "result", frames[-1]
        assert proc.wait(timeout=10) == 0
        return frames[-1]["payload"]["output"], frames
    finally:
        proc.kill()


def test_runtime_echo_end_to_end() -> None:
    output, frames = _execute_in_runtime("workers.echo", "run", {"message": "hi"})
    assert output["echo"] == "hi"
    assert any(frame["kind"] == "emit" for frame in frames)


def test_runtime_parallel_branch_end_to_end() -> None:
    output, _frames = _execute_in_runtime("workers.parallel", "a", {"x": "value"})
    assert output == {"worker": "a", "x": "value"}


def test_runtime_cycle_end_to_end() -> None:
    output, _frames = _execute_in_runtime(
        "workers.cycle", "looping", {"seed": {"seed": "s"}, "again": None}
    )
    assert output["continue"] is True
    assert output["iteration"] == FIRST_ITERATION


def test_runtime_software_team_planer_end_to_end() -> None:
    output, frames = _execute_in_runtime("workers.software_team.planer", "run", {})
    assert output["documents"]["doc/specification.md"] == "# specification (stub)\n"
    assert any(
        frame["kind"] == "log" and "planer produced" in frame["payload"]["message"]
        for frame in frames
    )
