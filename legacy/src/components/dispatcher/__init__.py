"""dispatcher 子包：Dispatcher Protocol + ProcessDispatcher 实现。"""

from .base import DispatchResult, Dispatcher
from .process import DEFAULT_WORKER_CMD, ProcessDispatcher

__all__ = ["Dispatcher", "DispatchResult", "ProcessDispatcher", "DEFAULT_WORKER_CMD"]