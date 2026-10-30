"""坏 fixture：模块导入即抛错（discover → tool_module_import）。"""

from __future__ import annotations

raise RuntimeError("broken tool module: import must fail")
