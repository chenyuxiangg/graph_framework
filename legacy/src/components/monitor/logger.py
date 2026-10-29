"""zlog 包装：取 system logger（声明在 scripts/zlog/config/output_config.json）。"""

from __future__ import annotations

import os
import sys

_HERE = os.path.dirname(__file__)
# _HERE = monitor/  →  components/  →  src/  →  graph_base/  →  scripts/
_SCRIPTS = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(_HERE))))
_ZLOG_DIR = os.path.join(_SCRIPTS, "zlog")
_ZLOG_CONFIG = os.path.join(_ZLOG_DIR, "config", "output_config.json")

# zlog 包位于 scripts/zlog/，需要 scripts/ 在 sys.path 才能 import
if _SCRIPTS not in sys.path:
    sys.path.insert(0, _SCRIPTS)


def _ensure_zlog_initialized() -> None:
    """确保 zlog 已用 config 初始化。"""
    from zlog import init_logging

    init_logging(_ZLOG_CONFIG)


def get_monitor():
    """返回 system logger（zlog 单实例，level=DEBUG 兜底）。"""
    _ensure_zlog_initialized()
    from zlog import get_logger

    return get_logger("system")