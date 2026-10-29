"""e2e 验收专用 worker fixture 包（``tests/e2e/fixtures/py/e2e_fixtures``）。

刻意与 ``python/workers/*`` 对外示例分开（AGENTS §2：``py-worker`` 模块拥有
``python/workers/**``，``e2e`` 模块只拥有 ``tests/e2e/**``）：

- 对外示例 ``python/workers/{echo,slow,parallel,cycle}.py`` 由 py-worker 维护，
  e2e 直接**复用**它们（AC-06 的 ``workers.slow``、AC-09 的 ``workers.parallel``）；
- 测试专用行为（写 pid 文件、派孙进程、写并发区间日志、抛异常/自杀）放在本包，
  避免把测试钩子塞进对外示例。

v1 投递是「整包广播」：``start`` 输出 ``{"x": payload}`` 被整体投递到下游端口，
``_payload.unwrap_start`` 负责取出 payload（也兼容上游原样透传）。
"""

from __future__ import annotations

__all__ = ["concurrency", "descendants", "faults", "steps"]
