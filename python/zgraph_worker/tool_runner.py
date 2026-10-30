"""工具注册表发现与执行（M4 Harness 工具执行层，spec 05 §5.6.2/§5.6.5）。

本模块把 ``discover`` / ``call`` 暴露为普通 worker 函数（签名 ``(inputs, ctx) -> dict``），
由 harness 通过 worker 协议 ``run`` 帧调用：module 固定 ``zgraph_worker.tool_runner``，
function 为 ``discover`` 或 ``call``。

v2/D3 约定：harness 每次工具调用都 spawn 一个全新 worker 进程，因此本模块不改协议、
也不依赖跨调用的进程内模块缓存；单次调用内只为导入工具模块而扩展 ``sys.path``。
工具级问题一律以结构化 ``{"ok": false, "error": {...}}`` 返回，绝不以非约定退出码退出。
"""

from __future__ import annotations

import importlib
import re
import sys
import traceback
from pathlib import Path
from types import ModuleType
from typing import Any

from .ctx import WorkerContext

TOOL_NAME_PATTERN: re.Pattern[str] = re.compile(r"^[a-z][a-z0-9_]{0,63}$")
MAX_DESCRIPTION_CHARS: int = 1024
MAX_TRACEBACK_BYTES: int = 8 * 1024

DECLARATION_TOOLS: str = "TOOLS"
DECLARATION_TOOL: str = "TOOL"

ERROR_TOOLS_DIR_MISSING: str = "tools_dir_missing"
ERROR_TOOL_MODULE_IMPORT: str = "tool_module_import"
ERROR_DUPLICATE_TOOL: str = "duplicate_tool"
ERROR_INVALID_DECLARATION: str = "invalid_declaration"
ERROR_RESULT_NOT_DICT: str = "result_not_dict"
ERROR_EXCEPTION: str = "exception"


class ToolDeclarationError(Exception):
    """工具声明/导入错误（内部传递，最终转成结构化 error，不抛给 worker 运行时）。"""

    def __init__(self, reason: str, message: str, traceback_text: str | None = None) -> None:
        super().__init__(message)
        self.reason: str = reason
        self.message: str = message
        self.traceback_text: str | None = traceback_text


def _opt_tools_dir(inputs: dict[str, Any]) -> Path | None:
    """从 inputs 解析非空 ``tools_dir`` 字符串；缺失或类型非法返回 ``None``。"""
    raw: Any = inputs.get("tools_dir")
    if not isinstance(raw, str) or not raw:
        return None
    return Path(raw)


def _iter_tool_files(tools_dir: Path) -> list[Path]:
    """递归收集 ``*.py``（跳过 ``_`` 开头文件/目录与 ``__pycache__``），按相对路径排序。"""
    relative_paths: list[Path] = []
    for path in tools_dir.rglob("*.py"):
        if not path.is_file():
            continue
        relative: Path = path.relative_to(tools_dir)
        if _is_ignored(relative):
            continue
        relative_paths.append(relative)
    return [tools_dir / relative for relative in sorted(relative_paths, key=_as_posix)]


def _is_ignored(relative: Path) -> bool:
    """任一路径段以 ``_`` 开头即跳过（同时覆盖 ``__pycache__``）。"""
    return any(part.startswith("_") for part in relative.parts)


def _as_posix(relative: Path) -> str:
    """排序键：POSIX 相对路径（保证跨平台确定性）。"""
    return relative.as_posix()


def _module_name_for(tools_dir: Path, path: Path) -> str:
    """工具模块全名：``f"{tools_dir.name}.{相对点分路径}"``。"""
    relative: Path = path.relative_to(tools_dir).with_suffix("")
    return ".".join((tools_dir.name, *relative.parts))


def _ensure_import_path(tools_dir: Path) -> None:
    """把 ``tools_dir.parent`` 插入 ``sys.path``（已存在则不动，避免重复污染）。"""
    parent: str = str(tools_dir.parent)
    if parent not in sys.path:
        sys.path.insert(0, parent)


def _declaration_form(module: ModuleType, module_name: str) -> str:
    """判定声明形式：恰好 TOOLS=list（形式 A）或 TOOL=dict（形式 B）。"""
    has_tools: bool = hasattr(module, DECLARATION_TOOLS)
    has_tool: bool = hasattr(module, DECLARATION_TOOL)
    if has_tools and has_tool:
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION,
            f"{module_name}: declares both {DECLARATION_TOOL} and {DECLARATION_TOOLS}",
        )
    if not has_tools and not has_tool:
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION,
            f"{module_name}: declares neither {DECLARATION_TOOL} nor {DECLARATION_TOOLS}",
        )
    return DECLARATION_TOOLS if has_tools else DECLARATION_TOOL


def _require_call(module: ModuleType, module_name: str) -> Any:
    """要求模块暴露可调用的 ``call``；否则声明非法。"""
    call_fn: Any = getattr(module, "call", None)
    if not callable(call_fn):
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION,
            f"{module_name}: missing callable call()",
        )
    return call_fn


def _validate_tool(raw: Any, module_name: str, index: int | None) -> dict[str, Any]:
    """校验单个工具声明，返回 ``{name, description, parameters}``。"""
    location: str = module_name if index is None else f"{module_name}[{index}]"
    if not isinstance(raw, dict):
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION, f"{location}: tool declaration must be an object"
        )
    name: Any = raw.get("name")
    if not isinstance(name, str) or TOOL_NAME_PATTERN.fullmatch(name) is None:
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION, f"{location}: invalid tool name {name!r}"
        )
    description: Any = raw.get("description")
    if (
        not isinstance(description, str)
        or not description
        or len(description) > MAX_DESCRIPTION_CHARS
    ):
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION,
            f"{location}: description must be a non-empty string <= {MAX_DESCRIPTION_CHARS} chars",
        )
    parameters: Any = raw.get("parameters")
    if not isinstance(parameters, dict):
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION, f"{location}: parameters must be a JSON object"
        )
    if parameters.get("type") != "object":
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION, f"{location}: parameters.type must be \"object\""
        )
    if not isinstance(parameters.get("properties"), dict):
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION, f"{location}: parameters.properties must be an object"
        )
    return {"name": name, "description": description, "parameters": parameters}


def _tool_definitions(module: ModuleType, module_name: str) -> list[dict[str, Any]]:
    """按声明形式取出并校验工具定义（保持声明数组顺序）。"""
    form: str = _declaration_form(module, module_name)
    _require_call(module, module_name)
    if form == DECLARATION_TOOL:
        return [_validate_tool(getattr(module, DECLARATION_TOOL), module_name, None)]
    raw_tools: Any = getattr(module, DECLARATION_TOOLS)
    if not isinstance(raw_tools, list):
        raise ToolDeclarationError(
            ERROR_INVALID_DECLARATION, f"{module_name}: {DECLARATION_TOOLS} must be a list"
        )
    return [_validate_tool(raw, module_name, index) for index, raw in enumerate(raw_tools)]


def _import_tool_module(module_name: str) -> ModuleType:
    """导入工具模块；任何导入异常 → ``tool_module_import``（带 traceback）。"""
    try:
        return importlib.import_module(module_name)
    except Exception as exc:  # noqa: BLE001 - 用户模块导入可抛任意异常
        raise ToolDeclarationError(
            ERROR_TOOL_MODULE_IMPORT,
            f"import {module_name} failed: {exc}",
            traceback.format_exc(),
        ) from exc


def _discover_tools(tools_dir: Path) -> list[dict[str, Any]]:
    """扫描并校验全部工具；返回带 ``source`` 的工具列表（文件序 + 声明序）。"""
    _ensure_import_path(tools_dir)
    discovered: list[dict[str, Any]] = []
    seen: dict[str, str] = {}
    for path in _iter_tool_files(tools_dir):
        module_name: str = _module_name_for(tools_dir, path)
        module: ModuleType = _import_tool_module(module_name)
        for tool in _tool_definitions(module, module_name):
            name: str = tool["name"]
            previous: str | None = seen.get(name)
            if previous is not None:
                raise ToolDeclarationError(
                    ERROR_DUPLICATE_TOOL,
                    f"duplicate tool {name!r} in {previous} and {module_name}",
                )
            seen[name] = module_name
            discovered.append({**tool, "source": module_name})
    return discovered


def _discover_error(reason: str, message: str, traceback_text: str | None = None) -> dict[str, Any]:
    """discover 失败载荷（固定含 ``traceback`` 键，可为 null）。"""
    return {
        "ok": False,
        "error": {"reason": reason, "message": message, "traceback": traceback_text},
    }


def _truncate_traceback(text: str) -> str:
    """traceback 截断到 ≤8 KiB（按 UTF-8 字节边界安全截断）。"""
    encoded: bytes = text.encode("utf-8")
    if len(encoded) <= MAX_TRACEBACK_BYTES:
        return text
    return encoded[:MAX_TRACEBACK_BYTES].decode("utf-8", errors="ignore")


def _exception_result(exc: BaseException) -> dict[str, Any]:
    """call 失败载荷（reason=exception，含异常类名 / message / 截断 traceback）。"""
    return {
        "ok": False,
        "error": {
            "reason": ERROR_EXCEPTION,
            "type": type(exc).__name__,
            "message": str(exc),
            "traceback": _truncate_traceback(traceback.format_exc()),
        },
    }


def discover(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """扫描 ``inputs["tools_dir"]`` 下全部工具声明（spec 05 §5.6.2）。

    ``ctx`` 为 worker 协议统一签名的一部分，本函数不使用。

    返回 ``{"ok": true, "tools": [{"name", "description", "parameters", "source"}]}``；
    失败返回带 ``reason`` / ``message`` / ``traceback`` 的 ``{"ok": false, "error": {...}}``。
    """
    tools_dir: Path | None = _opt_tools_dir(inputs)
    if tools_dir is None:
        return _discover_error(
            ERROR_TOOLS_DIR_MISSING, "inputs.tools_dir must be a non-empty path string"
        )
    if not tools_dir.is_dir():
        return _discover_error(
            ERROR_TOOLS_DIR_MISSING, f"tools_dir not found: {tools_dir}"
        )
    try:
        tools: list[dict[str, Any]] = _discover_tools(tools_dir)
    except ToolDeclarationError as exc:
        return _discover_error(exc.reason, exc.message, exc.traceback_text)
    except Exception as exc:  # noqa: BLE001 - 发现阶段任何异常都必须结构化返回
        return _discover_error(
            ERROR_TOOL_MODULE_IMPORT,
            f"unexpected discovery failure: {exc}",
            traceback.format_exc(),
        )
    return {"ok": True, "tools": tools}


def _execute_tool_call(inputs: dict[str, Any]) -> dict[str, Any]:
    """执行一次工具调用（异常由 :func:`call` 统一转结构化 error）。"""
    tools_dir: Path | None = _opt_tools_dir(inputs)
    if tools_dir is None:
        raise ValueError("inputs.tools_dir must be a non-empty path string")
    if not tools_dir.is_dir():
        raise FileNotFoundError(f"tools_dir not found: {tools_dir}")
    module_name: Any = inputs.get("module")
    tool_name: Any = inputs.get("name")
    if not isinstance(module_name, str) or not module_name:
        raise ValueError("inputs.module must be a non-empty string")
    if not isinstance(tool_name, str) or not tool_name:
        raise ValueError("inputs.name must be a non-empty string")
    raw_args: Any = inputs.get("args")
    args: dict[str, Any] = {} if raw_args is None else raw_args
    if not isinstance(args, dict):
        raise ValueError("inputs.args must be a JSON object")
    _ensure_import_path(tools_dir)
    module: ModuleType = importlib.import_module(module_name)
    form: str = _declaration_form(module, module_name)
    call_fn: Any = _require_call(module, module_name)
    result: Any = call_fn(tool_name, args) if form == DECLARATION_TOOLS else call_fn(args)
    if not isinstance(result, dict):
        return {
            "ok": False,
            "error": {
                "reason": ERROR_RESULT_NOT_DICT,
                "type": type(result).__name__,
                "message": f"tool returned {type(result).__name__}, expected dict",
            },
        }
    return {"ok": True, "result": result}


def call(inputs: dict[str, Any], ctx: WorkerContext) -> dict[str, Any]:
    """按 ``source`` 模块声明的形式分派执行一个工具（spec 05 §5.6.5）。

    inputs：``{"tools_dir": abs, "module": source, "name": 工具名, "args": dict}``；
    ``ctx`` 为 worker 协议统一签名的一部分，本函数不使用。

    成功 → ``{"ok": true, "result": <dict>}``；非 dict → ``reason=result_not_dict``；
    任意异常 → ``reason=exception``（含 type/message/≤8 KiB traceback），绝不抛出。
    """
    try:
        return _execute_tool_call(inputs)
    except Exception as exc:  # noqa: BLE001 - 工具执行异常必须结构化返回，不得以 error 帧退出
        return _exception_result(exc)
