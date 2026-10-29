"""测试 fixture：执行时写标记文件（验证 hello_ack 前不执行用户代码）。"""

from __future__ import annotations

from pathlib import Path
from typing import Any


def run(inputs: dict[str, Any], ctx: Any) -> dict[str, Any]:
    """写 ``inputs.marker`` 文件并返回。"""
    Path(str(inputs["marker"])).write_text("executed", encoding="utf-8")
    return {"ok": True}
