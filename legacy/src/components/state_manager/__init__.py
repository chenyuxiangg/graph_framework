"""state_manager 子包：State dataclass + StateManager 公开方法。"""

from .core import StateManager
from .schema import State
from .terminal import is_sop_done, run_post_handle

__all__ = ["State", "StateManager", "is_sop_done", "run_post_handle"]