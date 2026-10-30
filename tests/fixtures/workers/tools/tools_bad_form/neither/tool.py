"""坏 fixture：既没有 TOOL 也没有 TOOLS（discover → invalid_declaration）。"""

from __future__ import annotations

from typing import Any


def call(args: dict[str, Any]) -> dict[str, Any]:
    """不可达：声明形式非法，discover 在调用前即报错。"""
    return {"ok": True}
