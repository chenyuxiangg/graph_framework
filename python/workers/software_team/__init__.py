"""software_team 示例 worker 子包（迁移自 ``legacy/src/worker/software_team/``）。

legacy 的 ``planer_proc`` / ``archer_proc`` 直接写 ``base_dir/doc/*.md``；迁移后按 worker
SDK 契约改为**纯函数 + dict 返回**：文档路径与内容作为 ``output`` 返回，由框架/调用方决定
是否落盘（L3：不在 worker 内产生文件副作用）。
"""

from __future__ import annotations

__all__ = [
    "archer",
    "parse_verdict",
    "planer",
]
