"""反射接口：op / cmd 函数注册与查找。

进程内单例 dict + threading.Lock。内核层只维护名字 → 可调用对象的映射；
不调用这些函数——组件层（调度器）拿到函数后再传 ctx 执行。
"""

from __future__ import annotations

import threading
from typing import Any, Callable

_LOCK = threading.Lock()
_OP_REGISTRY: dict[str, Callable[..., Any]] = {}
_CMD_REGISTRY: dict[str, Callable[..., Any]] = {}


class RegistryKeyError(KeyError):
    """resolve_op / resolve_cmd 找不到对应函数。"""


class DuplicateRegistrationError(ValueError):
    """register_op / register_cmd 同名重复注册。"""


def register_op(name: str, fn: Callable[..., Any]) -> None:
    """注册 op 函数。同 name 已存在抛 DuplicateRegistrationError。"""
    with _LOCK:
        if name in _OP_REGISTRY:
            raise DuplicateRegistrationError(f"op 「{name}」 已注册")
        _OP_REGISTRY[name] = fn


def resolve_op(name: str) -> Callable[..., Any]:
    """按名字找 op 函数；不存在抛 RegistryKeyError。"""
    with _LOCK:
        fn = _OP_REGISTRY.get(name)
    if fn is None:
        raise RegistryKeyError(f"op 「{name}」 未注册")
    return fn


def register_cmd(name: str, fn: Callable[..., Any]) -> None:
    """注册 cmd 函数。同 name 已存在抛 DuplicateRegistrationError。"""
    with _LOCK:
        if name in _CMD_REGISTRY:
            raise DuplicateRegistrationError(f"cmd 「{name}」 已注册")
        _CMD_REGISTRY[name] = fn


def resolve_cmd(name: str) -> Callable[..., Any]:
    """按名字找 cmd 函数；不存在抛 RegistryKeyError。"""
    with _LOCK:
        fn = _CMD_REGISTRY.get(name)
    if fn is None:
        raise RegistryKeyError(f"cmd 「{name}」 未注册")
    return fn


def reset_registry() -> None:
    """清空所有注册。仅用于测试。生产代码不要调用。"""
    with _LOCK:
        _OP_REGISTRY.clear()
        _CMD_REGISTRY.clear()