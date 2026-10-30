"""``zgraph_worker.tool_runner`` 单测（M4 / REV-019）。

覆盖 discover/call 的成功与全部错误分支（导入失败、坏声明、重名、非 dict 结果、
用户异常、traceback 截断、tools_dir 缺失）、``sys.path`` 副作用与确定性顺序，
以及两条以上经 ``python -m zgraph_worker.runtime`` 的真实子进程 NDJSON 往返。
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
from pathlib import Path
from types import ModuleType
from typing import Any, Iterator

import pytest

from zgraph_worker.ctx import WorkerContext
from zgraph_worker.protocol import encode_frame
from zgraph_worker.tool_runner import (
    ERROR_DUPLICATE_TOOL,
    ERROR_EXCEPTION,
    ERROR_INVALID_DECLARATION,
    ERROR_RESULT_NOT_DICT,
    ERROR_TOOL_MODULE_IMPORT,
    ERROR_TOOLS_DIR_MISSING,
    MAX_TRACEBACK_BYTES,
    call,
    discover,
)

PYTHON_DIR: Path = Path(__file__).resolve().parents[1]
REPO_ROOT: Path = PYTHON_DIR.parent
TOOLS_FIXTURES: Path = REPO_ROOT / "tests" / "fixtures" / "workers" / "tools"
TOOLS_OK: Path = TOOLS_FIXTURES / "tools_ok" / "tools"
TOOLS_BAD_IMPORT: Path = TOOLS_FIXTURES / "tools_bad_import"
TOOLS_BAD_FORM: Path = TOOLS_FIXTURES / "tools_bad_form"
TOOLS_BAD_DUP: Path = TOOLS_FIXTURES / "tools_bad_dup"

VALID_TOOL: dict[str, Any] = {
    "name": "echo",
    "description": "回显",
    "parameters": {"type": "object", "properties": {}},
}

MODULE_HEADER: str = "from __future__ import annotations\n\nfrom typing import Any\n\n"


def _tool_declaration(value: Any, key: str = "TOOL") -> str:
    """把声明值序列化为模块级 ``TOOL`` / ``TOOLS`` 字面量源码。"""
    return f"{key} = {json.dumps(value, ensure_ascii=False)}\n"


INVALID_TOOL_DECLARATIONS: tuple[tuple[str, str], ...] = (
    (
        "uppercase_name",
        _tool_declaration(
            {"name": "Bad", "description": "d", "parameters": VALID_TOOL["parameters"]}
        ),
    ),
    (
        "empty_description",
        _tool_declaration(
            {"name": "ok", "description": "", "parameters": VALID_TOOL["parameters"]}
        ),
    ),
    (
        "long_description",
        _tool_declaration(
            {"name": "ok", "description": "x" * 1025, "parameters": VALID_TOOL["parameters"]}
        ),
    ),
    (
        "parameters_not_object",
        _tool_declaration({"name": "ok", "description": "d", "parameters": "bad"}),
    ),
    (
        "parameters_type_not_object",
        _tool_declaration(
            {"name": "ok", "description": "d", "parameters": {"type": "array", "properties": {}}}
        ),
    ),
    (
        "properties_not_object",
        _tool_declaration(
            {"name": "ok", "description": "d", "parameters": {"type": "object", "properties": []}}
        ),
    ),
    ("tool_not_dict", _tool_declaration("bad")),
    ("tools_not_list", _tool_declaration(VALID_TOOL, key="TOOLS")),
)


def _make_ctx() -> WorkerContext:
    """构造真实 WorkerContext（sink 指向空实现）。"""
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
        emit_sink=lambda _event: None,
        log_sink=lambda _event: None,
        is_cancelled=threading.Event().is_set,
    )


def _discover(tools_dir: Path) -> dict[str, Any]:
    """直接调用 ``discover``（inputs 形状与 harness 契约一致）。"""
    return discover({"tools_dir": str(tools_dir)}, _make_ctx())


def _call(tools_dir: Path, module: str, name: str, args: dict[str, Any]) -> dict[str, Any]:
    """直接调用 ``call``（inputs 形状与 harness 契约一致）。"""
    return call(
        {"tools_dir": str(tools_dir), "module": module, "name": name, "args": args},
        _make_ctx(),
    )


def _module_source(declaration: str, call_definition: str | None) -> str:
    """拼装工具模块源码（模块头 + 声明 + 可选 call）。"""
    parts: list[str] = [MODULE_HEADER, declaration]
    if call_definition is not None:
        parts.append(call_definition)
    return "".join(parts)


def _call_definition(body: str = "return {\"ok\": True}") -> str:
    """形式 B 的 ``call(args)`` 定义源码。"""
    return (
        "\n\ndef call(args: dict[str, Any]) -> dict[str, Any]:\n"
        "    \"\"\"返回固定结果。\"\"\"\n"
        f"    {body}\n"
    )


def _write_module(directory: Path, stem: str, source: str) -> None:
    """在 ``directory`` 下写入 ``<stem>.py``（必要时建目录）。"""
    directory.mkdir(parents=True, exist_ok=True)
    (directory / f"{stem}.py").write_text(source, encoding="utf-8")


def _module_under(module: ModuleType, roots: list[str]) -> bool:
    """判断模块（含 namespace package）是否来自本次用例新增的 sys.path 目录。"""
    locations: list[str] = []
    file_name: Any = getattr(module, "__file__", None)
    if isinstance(file_name, str):
        locations.append(file_name)
    namespace_paths: Any = getattr(module, "__path__", None)
    if namespace_paths is not None:
        locations.extend(str(item) for item in namespace_paths)
    return any(location.startswith(root) for location in locations for root in roots)


@pytest.fixture(autouse=True)
def _isolate_tool_imports() -> Iterator[None]:
    """用例结束后恢复 sys.path 并卸载本用例导入的工具模块（清理导入副作用）。"""
    original_path: list[str] = list(sys.path)
    original_names: set[str] = set(sys.modules)
    try:
        yield
    finally:
        added_roots: list[str] = [item for item in sys.path if item not in original_path]
        stale_names: list[str] = [
            name
            for name in sys.modules
            if name not in original_names and _module_under(sys.modules[name], added_roots)
        ]
        for name in stale_names:
            del sys.modules[name]
        sys.path[:] = original_path


def test_discover_returns_tools_in_file_then_declaration_order() -> None:
    result: dict[str, Any] = _discover(TOOLS_OK)
    assert result["ok"] is True
    tools: list[dict[str, Any]] = result["tools"]
    assert [tool["name"] for tool in tools] == ["now", "add", "explode", "echo", "sleepy"]
    assert [tool["source"] for tool in tools] == [
        "tools.builtin",
        "tools.builtin",
        "tools.failing",
        "tools.single",
        "tools.slow",
    ]
    assert all(set(tool) >= {"name", "description", "parameters", "source"} for tool in tools)
    allowed_keys: set[str] = {"name", "description", "parameters", "source", "timeout_ms"}
    assert all(set(tool) <= allowed_keys for tool in tools)
    assert all(tool["description"] for tool in tools)
    assert all(tool["parameters"]["type"] == "object" for tool in tools)


def test_discover_passes_through_optional_timeout_ms() -> None:
    tools: dict[str, dict[str, Any]] = {
        tool["name"]: tool for tool in _discover(TOOLS_OK)["tools"]
    }
    assert tools["add"]["timeout_ms"] == 30000
    assert tools["sleepy"]["timeout_ms"] == 300
    assert "timeout_ms" not in tools["now"]
    assert "timeout_ms" not in tools["echo"]


@pytest.mark.parametrize(
    "timeout_ms",
    [0, -1, 600001, True, "300", 1.5, None],
    ids=["zero", "negative", "too_large", "bool", "string", "float", "null"],
)
def test_discover_invalid_timeout_ms_is_rejected(tmp_path: Path, timeout_ms: Any) -> None:
    tools_dir: Path = tmp_path / "tools_timeout"
    declaration: dict[str, Any] = {
        "name": "ok",
        "description": "d",
        "parameters": VALID_TOOL["parameters"],
        "timeout_ms": timeout_ms,
    }
    source: str = MODULE_HEADER + "TOOL = " + repr(declaration) + "\n" + _call_definition()
    _write_module(tools_dir, "tool", source)
    result: dict[str, Any] = _discover(tools_dir)
    assert result["ok"] is False
    assert result["error"]["reason"] == ERROR_INVALID_DECLARATION


def test_discover_accepts_timeout_ms_boundaries(tmp_path: Path) -> None:
    for index, timeout_ms in enumerate((1, 600000)):
        tools_dir: Path = tmp_path / f"tools_bound_{index}"
        declaration: dict[str, Any] = {
            "name": f"ok{index}",
            "description": "d",
            "parameters": VALID_TOOL["parameters"],
            "timeout_ms": timeout_ms,
        }
        source: str = MODULE_HEADER + "TOOL = " + repr(declaration) + "\n" + _call_definition()
        _write_module(tools_dir, "tool", source)
        result: dict[str, Any] = _discover(tools_dir)
        assert result["ok"] is True
        assert result["tools"][0]["timeout_ms"] == timeout_ms


def test_discover_is_deterministic_and_does_not_duplicate_sys_path() -> None:
    first: dict[str, Any] = _discover(TOOLS_OK)
    second: dict[str, Any] = _discover(TOOLS_OK)
    assert first == second
    parent: str = str(TOOLS_OK.parent)
    assert sys.path.count(parent) == 1


def test_discover_inserts_tools_dir_parent_into_sys_path() -> None:
    parent: str = str(TOOLS_OK.parent)
    assert parent not in sys.path
    assert _discover(TOOLS_OK)["ok"] is True
    assert sys.path[0] == parent


def test_discover_skips_underscore_entries_and_pycache(tmp_path: Path) -> None:
    tools_dir: Path = tmp_path / "tools_skip"
    tools_dir.mkdir()
    nested_declaration: str = _tool_declaration(
        [{"name": "inner", "description": "inner", "parameters": VALID_TOOL["parameters"]}],
        key="TOOLS",
    )
    nested_call: str = (
        "\n\ndef call(name: str, args: dict[str, Any]) -> dict[str, Any]:\n"
        "    \"\"\"返回工具名。\"\"\"\n"
        "    return {\"name\": name}\n"
    )
    top_declaration: str = _tool_declaration(
        {"name": "top", "description": "top", "parameters": VALID_TOOL["parameters"]}
    )
    _write_module(tools_dir / "nested", "inner", _module_source(nested_declaration, nested_call))
    _write_module(tools_dir, "top", _module_source(top_declaration, _call_definition()))
    _write_module(tools_dir, "_hidden", "raise RuntimeError(\"must not be imported\")\n")
    _write_module(tools_dir / "_private", "skip", "raise RuntimeError(\"skip\")\n")
    _write_module(tools_dir / "__pycache__", "cached", "raise RuntimeError(\"skip\")\n")
    result: dict[str, Any] = _discover(tools_dir)
    assert result["ok"] is True
    assert [tool["name"] for tool in result["tools"]] == ["inner", "top"]
    assert [tool["source"] for tool in result["tools"]] == [
        "tools_skip.nested.inner",
        "tools_skip.top",
    ]


def test_discover_missing_tools_dir_returns_structured_error(tmp_path: Path) -> None:
    missing: dict[str, Any] = _discover(tmp_path / "missing")
    assert missing["ok"] is False
    assert missing["error"]["reason"] == ERROR_TOOLS_DIR_MISSING
    assert missing["error"]["message"]
    assert missing["error"]["traceback"] is None
    for inputs in ({}, {"tools_dir": ""}, {"tools_dir": 42}):
        result: dict[str, Any] = discover(inputs, _make_ctx())
        assert result["error"]["reason"] == ERROR_TOOLS_DIR_MISSING


def test_discover_import_failure_returns_tool_module_import() -> None:
    result: dict[str, Any] = _discover(TOOLS_BAD_IMPORT)
    assert result["ok"] is False
    assert result["error"]["reason"] == ERROR_TOOL_MODULE_IMPORT
    assert "RuntimeError" in result["error"]["traceback"]
    assert "broken tool module" in result["error"]["message"]


def test_discover_forms_declared_together_is_invalid() -> None:
    result: dict[str, Any] = _discover(TOOLS_BAD_FORM)
    assert result["error"]["reason"] == ERROR_INVALID_DECLARATION
    assert "both TOOL and TOOLS" in result["error"]["message"]


def test_discover_missing_form_is_invalid() -> None:
    result: dict[str, Any] = _discover(TOOLS_BAD_FORM / "neither")
    assert result["error"]["reason"] == ERROR_INVALID_DECLARATION
    assert "neither TOOL nor TOOLS" in result["error"]["message"]


@pytest.mark.parametrize(
    "declaration",
    [case[1] for case in INVALID_TOOL_DECLARATIONS],
    ids=[case[0] for case in INVALID_TOOL_DECLARATIONS],
)
def test_discover_invalid_tool_fields_are_rejected(tmp_path: Path, declaration: str) -> None:
    tools_dir: Path = tmp_path / "tools_case"
    _write_module(tools_dir, "tool", _module_source(declaration, _call_definition()))
    result: dict[str, Any] = _discover(tools_dir)
    assert result["ok"] is False
    assert result["error"]["reason"] == ERROR_INVALID_DECLARATION


def test_discover_requires_callable_call(tmp_path: Path) -> None:
    tools_dir: Path = tmp_path / "tools_no_call"
    declaration: str = _tool_declaration(VALID_TOOL)
    _write_module(tools_dir, "tool", _module_source(declaration, None))
    result: dict[str, Any] = _discover(tools_dir)
    assert result["error"]["reason"] == ERROR_INVALID_DECLARATION
    assert "missing callable call()" in result["error"]["message"]


def test_discover_duplicate_tool_across_modules_is_rejected() -> None:
    result: dict[str, Any] = _discover(TOOLS_BAD_DUP)
    assert result["ok"] is False
    assert result["error"]["reason"] == ERROR_DUPLICATE_TOOL
    assert "tools_bad_dup.first" in result["error"]["message"]
    assert "tools_bad_dup.second" in result["error"]["message"]


def test_discover_accepts_form_a_with_empty_tool_list(tmp_path: Path) -> None:
    tools_dir: Path = tmp_path / "tools_empty"
    declaration: str = _tool_declaration([], key="TOOLS")
    _write_module(tools_dir, "empty", _module_source(declaration, _call_definition()))
    result: dict[str, Any] = _discover(tools_dir)
    assert result == {"ok": True, "tools": []}


def test_call_form_a_dispatches_tool_and_returns_dict() -> None:
    added: dict[str, Any] = _call(TOOLS_OK, "tools.builtin", "add", {"a": 1, "b": 2})
    assert added == {"ok": True, "result": {"sum": 3}}
    now: dict[str, Any] = _call(TOOLS_OK, "tools.builtin", "now", {})
    assert now["ok"] is True
    assert isinstance(now["result"]["now"], str)


def test_call_form_b_passes_args_dict() -> None:
    echoed: dict[str, Any] = _call(TOOLS_OK, "tools.single", "echo", {"text": "hi"})
    assert echoed == {"ok": True, "result": {"echo": "hi"}}


def test_call_slow_tool_returns_result() -> None:
    slept: dict[str, Any] = _call(TOOLS_OK, "tools.slow", "sleepy", {"seconds": 0.01})
    assert slept == {"ok": True, "result": {"slept": 0.01}}


def test_call_unknown_tool_returns_exception() -> None:
    result: dict[str, Any] = _call(TOOLS_OK, "tools.builtin", "missing", {})
    assert result["ok"] is False
    assert result["error"]["reason"] == ERROR_EXCEPTION
    assert result["error"]["type"] == "KeyError"
    assert "missing" in result["error"]["message"]


def test_call_tool_exception_is_structured() -> None:
    result: dict[str, Any] = _call(TOOLS_OK, "tools.failing", "explode", {})
    assert result["ok"] is False
    error: dict[str, Any] = result["error"]
    assert error["reason"] == ERROR_EXCEPTION
    assert error["type"] == "ValueError"
    assert error["message"] == "boom"
    assert "ValueError: boom" in error["traceback"]


def test_call_non_dict_result_returns_result_not_dict(tmp_path: Path) -> None:
    tools_dir: Path = tmp_path / "tools_not_dict"
    declaration: str = _tool_declaration(VALID_TOOL)
    source: str = _module_source(declaration, _call_definition("return [1, 2, 3]"))
    _write_module(tools_dir, "tool", source)
    result: dict[str, Any] = _call(tools_dir, "tools_not_dict.tool", "echo", {})
    assert result["ok"] is False
    assert result["error"]["reason"] == ERROR_RESULT_NOT_DICT
    assert result["error"]["type"] == "list"
    assert "traceback" not in result["error"]


def test_call_traceback_is_truncated_to_8kib(tmp_path: Path) -> None:
    tools_dir: Path = tmp_path / "tools_long_tb"
    declaration: str = _tool_declaration(VALID_TOOL)
    body: str = "raise RuntimeError(\"x\" * 20000)"
    source: str = _module_source(declaration, _call_definition(body))
    _write_module(tools_dir, "tool", source)
    result: dict[str, Any] = _call(tools_dir, "tools_long_tb.tool", "echo", {})
    traceback_text: str = result["error"]["traceback"]
    assert result["error"]["type"] == "RuntimeError"
    assert 0 < len(traceback_text.encode("utf-8")) <= MAX_TRACEBACK_BYTES


def test_call_invalid_declaration_returns_exception() -> None:
    result: dict[str, Any] = _call(TOOLS_BAD_FORM / "both", "both.tool", "both", {})
    assert result["ok"] is False
    assert result["error"]["reason"] == ERROR_EXCEPTION
    assert result["error"]["type"] == "ToolDeclarationError"


def test_call_missing_tools_dir_returns_exception() -> None:
    result: dict[str, Any] = call({}, _make_ctx())
    assert result["ok"] is False
    assert result["error"]["reason"] == ERROR_EXCEPTION
    assert result["error"]["type"] == "ValueError"


def _runtime_env() -> dict[str, str]:
    """子进程环境：SDK 从仓库 ``python/`` 导入。"""
    env: dict[str, str] = dict(os.environ)
    env["PYTHONPATH"] = str(PYTHON_DIR)
    env["PYTHONHASHSEED"] = "0"
    return env


def _spawn_runtime() -> subprocess.Popen[bytes]:
    """启动真实 worker 子进程，并后台排空 stderr（协议 §4.2）。"""
    proc: subprocess.Popen[bytes] = subprocess.Popen(
        [sys.executable, "-m", "zgraph_worker.runtime"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        cwd=REPO_ROOT,
        env=_runtime_env(),
    )

    def _drain() -> None:
        assert proc.stderr is not None
        while proc.stderr.readline():
            continue

    threading.Thread(target=_drain, daemon=True).start()
    return proc


def _readline(proc: subprocess.Popen[bytes]) -> dict[str, Any]:
    """读一行 NDJSON 帧。"""
    assert proc.stdout is not None
    line: bytes = proc.stdout.readline()
    assert line, "worker closed stdout without a frame"
    return json.loads(line)


def _run_frame(module: str, function: str, inputs: dict[str, Any]) -> dict[str, Any]:
    """构造协议 ``run`` 帧。"""
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
    function: str, inputs: dict[str, Any]
) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    """真实子进程：hello → hello_ack → run → 收集到 result/error 后返回。"""
    proc: subprocess.Popen[bytes] = _spawn_runtime()
    try:
        hello: dict[str, Any] = _readline(proc)
        assert hello["kind"] == "hello"
        assert proc.stdin is not None
        proc.stdin.write(encode_frame("hello_ack", {"accepted": True}))
        proc.stdin.write(
            json.dumps(
                _run_frame("zgraph_worker.tool_runner", function, inputs), ensure_ascii=False
            ).encode("utf-8")
            + b"\n"
        )
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


def test_runtime_discover_end_to_end() -> None:
    output, _frames = _execute_in_runtime("discover", {"tools_dir": str(TOOLS_OK)})
    assert output["ok"] is True
    assert [tool["name"] for tool in output["tools"]] == [
        "now",
        "add",
        "explode",
        "echo",
        "sleepy",
    ]


def test_runtime_call_end_to_end() -> None:
    output, _frames = _execute_in_runtime(
        "call",
        {
            "tools_dir": str(TOOLS_OK),
            "module": "tools.builtin",
            "name": "add",
            "args": {"a": 2, "b": 40},
        },
    )
    assert output == {"ok": True, "result": {"sum": 42}}


def test_runtime_tool_exception_does_not_use_error_frame() -> None:
    # 工具异常是业务失败：worker 必须以 result 帧 + exit 0 结束（不得以 error 帧退出）。
    output, _frames = _execute_in_runtime(
        "call",
        {
            "tools_dir": str(TOOLS_OK),
            "module": "tools.failing",
            "name": "explode",
            "args": {},
        },
    )
    assert output["ok"] is False
    assert output["error"]["reason"] == ERROR_EXCEPTION
    assert output["error"]["type"] == "ValueError"
