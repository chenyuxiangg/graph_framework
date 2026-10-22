"""worker 集合根目录。

按 SOP 分组：每 SOP 一个子包，子包内每个 node 一个 worker 模块（含 worker_run 函数）。
ProcessDispatcher 通过 worker_cmd 模板拉起对应 worker；worker 内部:
  1. 解析 argv
  2. 读 state.json
  3. resolve_op(proc or "proc_bypass")
  4. 跑 proc 拿 output_files
  5. record_completion 推进 state

__init__.py 自动发现 src/worker/ 下所有子包并 import — 触发各 SOP worker
模块的 register_op / register_cmd（让 gate op / proc / cmd 全局可见）。
"""

from __future__ import annotations

import pkgutil

# 注册公共 proc_bypass
from src.kernel import register_op


def proc_bypass(args) -> tuple[str, ...]:
    """proc 字段为空时使用：不执行任何逻辑，返回空产物 tuple。"""
    return ()


PROC_BYPASS_NAME = "proc_bypass"
register_op(PROC_BYPASS_NAME, proc_bypass)


# 自动 import 所有子包（src/worker/<sop>/__init__.py 里的 register_* 会触发）
for _info in pkgutil.iter_modules(__path__, prefix=f"{__name__}."):
    __import__(_info.name)