"""M3 示例 worker 包（从 ``legacy/src/worker/`` 迁移，见 ``doc/architecture-v2.md`` §2.1）。

模块/函数名与验收 fixture 图的 ``python_node.config`` 对齐：

- ``workers.parallel`` 的 ``a`` / ``b``：``tests/fixtures/graphs/valid_parallel.json``（AC-09）
- ``workers.cycle`` 的 ``looping``：``tests/fixtures/graphs/valid_cycle.json``
- ``workers.echo`` 的 ``run``：SDK 往返示例（AC-01）
- ``workers.slow`` 的 ``run``：``slow`` 睡眠示例（AC-06 超时 / 取消）
- ``workers.software_team.planer`` / ``workers.software_team.archer``：legacy software_team 迁移

**L3 契约（ISSUE-022）**：每个 worker 函数的 docstring 必须声明「输入 / 输出 / 幂等性 /
外部副作用」四项。默认要求纯函数：不写文件、不发网络、不写全局状态；确需副作用时必须在
docstring 中显式声明（如 ``workers.slow`` 的可选 PID 文件）。L3 不承诺副作用可回滚，
因此可重试的 worker 应尽量实现幂等。
"""

from __future__ import annotations

__all__ = [
    "cycle",
    "echo",
    "parallel",
    "slow",
    "software_team",
]
