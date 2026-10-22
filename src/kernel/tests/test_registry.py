"""registry 测试。"""

import threading

import pytest

from src.kernel import (
    DuplicateRegistrationError,
    RegistryKeyError,
    register_cmd,
    register_op,
    reset_registry,
    resolve_cmd,
    resolve_op,
)


def _reset():
    reset_registry()


def test_register_and_resolve_op():
    """测试名：test_register_and_resolve_op

    测试场景：register_op 注册的函数能被 resolve_op 原样取出。
    前置条件：reset_registry 已清空。
    是否使用 mock：No。
    测试步骤：1. register_op("plus_one", fn)；2. resolve_op("plus_one")。
    预期结果：resolve_op 返回的函数 is fn（同一对象）。
    测试后清理：reset_registry。
    """
    _reset()
    fn = lambda x: x + 1
    register_op("plus_one", fn)
    assert resolve_op("plus_one") is fn


def test_register_duplicate_op_raises():
    """测试名：test_register_duplicate_op_raises

    测试场景：同名 op 二次注册抛 DuplicateRegistrationError。
    前置条件：reset_registry 已清空。
    是否使用 mock：No。
    测试步骤：连续两次 register_op("dup", ...)。
    预期结果：第二次注册抛 DuplicateRegistrationError。
    测试后清理：reset_registry。
    """
    _reset()
    register_op("dup", lambda: 1)
    with pytest.raises(DuplicateRegistrationError):
        register_op("dup", lambda: 2)


def test_resolve_missing_op_raises():
    """测试名：test_resolve_missing_op_raises

    测试场景：resolve 未注册的 op 名抛 RegistryKeyError。
    前置条件：reset_registry 已清空；"ghost" 未注册。
    是否使用 mock：No。
    测试步骤：调 resolve_op("ghost")。
    预期结果：抛 RegistryKeyError。
    测试后清理：reset_registry。
    """
    _reset()
    with pytest.raises(RegistryKeyError):
        resolve_op("ghost")


def test_register_and_resolve_cmd():
    """测试名：test_register_and_resolve_cmd

    测试场景：register_cmd 注册的函数能被 resolve_cmd 原样取出。
    前置条件：reset_registry 已清空。
    是否使用 mock：No。
    测试步骤：1. register_cmd("do_it", fn)；2. resolve_cmd("do_it")。
    预期结果：resolve_cmd 返回的函数 is fn（同一对象）。
    测试后清理：reset_registry。
    """
    _reset()
    fn = lambda: "ok"
    register_cmd("do_it", fn)
    assert resolve_cmd("do_it") is fn


def test_op_and_cmd_namespaces_independent():
    """测试名：test_op_and_cmd_namespaces_independent

    测试场景：op 与 cmd 是两个独立命名空间，同名互不覆盖。
    前置条件：reset_registry 已清空。
    是否使用 mock：No。
    测试步骤：1. register_op("same_name", op_fn)；2. register_cmd("same_name", cmd_fn)；
      3. resolve_op("same_name")() == "op"；4. resolve_cmd("same_name")() == "cmd"。
    预期结果：两个命名空间各自返回自己的实现。
    测试后清理：reset_registry。
    """
    _reset()
    register_op("same_name", lambda: "op")
    register_cmd("same_name", lambda: "cmd")
    assert resolve_op("same_name")() == "op"
    assert resolve_cmd("same_name")() == "cmd"


def test_reset_clears():
    """测试名：test_reset_clears

    测试场景：reset_registry 同时清空 op 与 cmd 两个表。
    前置条件：reset_registry 已清空；注册 1 op + 1 cmd。
    是否使用 mock：No。
    测试步骤：1. register_op("x", ...)+register_cmd("y", ...)；2. reset_registry()；
      3. resolve_op("x") / resolve_cmd("y")。
    预期结果：两次 resolve 均抛 RegistryKeyError。
    测试后清理：reset_registry（重复保险）。
    """
    _reset()
    register_op("x", lambda: 1)
    register_cmd("y", lambda: 2)
    reset_registry()
    with pytest.raises(RegistryKeyError):
        resolve_op("x")
    with pytest.raises(RegistryKeyError):
        resolve_cmd("y")


def test_concurrent_register_resolve():
    """测试名：test_concurrent_register_resolve

    测试场景：10 写线程 + 5 读线程并发跑，registry 不抛错。
    前置条件：reset_registry 已清空。
    是否使用 mock：No。
    测试步骤：1. 起 10 个 register 线程（每人 register 100 个不同名）；
      2. 起 5 个 resolve 线程（每人 resolve_op 1000 次，找 "op_0_50"）；
      3. join 后取 errors 列表。
    预期结果：errors 为空列表——注册与解析无竞态错误。
    测试后清理：reset_registry。
    """
    _reset()
    errors: list[str] = []

    def register_many(start: int):
        try:
            for i in range(100):
                name = f"op_{start}_{i}"
                register_op(name, lambda v=i: v)
        except Exception as exc:
            errors.append(f"reg: {exc}")

    def resolve_many():
        try:
            for _ in range(1000):
                try:
                    resolve_op("op_0_50")
                except RegistryKeyError:
                    pass  # 可能还没注册完
        except Exception as exc:
            errors.append(f"resolve: {exc}")

    threads = [
        threading.Thread(target=register_many, args=(i,)) for i in range(10)
    ] + [threading.Thread(target=resolve_many) for _ in range(5)]
    for t in threads:
        t.start()
    for t in threads:
        t.join()

    assert errors == []
