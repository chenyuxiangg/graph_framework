"""节点上下文 ``ctx``：emit / log / get_var / user_id / 取消与截止时间。

与 spec 04 §4.2 的 NodeContext 对应；跨进程差异见 ``doc/architecture-v2.md`` §8 D2。
"""

from __future__ import annotations

import random
import time
from typing import Any, Callable, Mapping

from .errors import WorkerCancelled
from .protocol import ALLOWED_EMIT_TYPES, ProtocolError

EMIT_MAX_PERCENT: int = 100


class WorkerContext:
    """worker 执行上下文（只读视图 + 只写事件通道）。"""

    def __init__(
        self,
        *,
        run_id: str,
        instance_id: str,
        node_id: str,
        attempt: int,
        user_id: str,
        global_vars: Mapping[str, Any],
        params: Mapping[str, Any],
        deadline_ms: int,
        seed: int,
        emit_sink: Callable[[dict[str, Any]], None],
        log_sink: Callable[[dict[str, Any]], None],
        is_cancelled: Callable[[], bool],
    ) -> None:
        self.run_id: str = run_id
        self.instance_id: str = instance_id
        self.node_id: str = node_id
        self.attempt: int = attempt
        self.user_id: str = user_id
        self.params: Mapping[str, Any] = params
        self.deadline_ms: int = deadline_ms
        self.seed: int = seed
        self._global_vars: Mapping[str, Any] = global_vars
        self._emit_sink = emit_sink
        self._log_sink = log_sink
        self._is_cancelled = is_cancelled
        self._rng: random.Random = random.Random(seed)
        self._start_mono: float = time.monotonic()
        self._start_epoch: float = time.time()

    @property
    def cancel_requested(self) -> bool:
        """是否已收到取消请求（快照语义）。"""
        return self._is_cancelled()

    def check_cancelled(self) -> None:
        """已取消则抛 :class:`WorkerCancelled`；供长循环安全点调用。"""
        if self._is_cancelled():
            raise WorkerCancelled("worker cancelled by supervisor")

    def emit(self, event: dict[str, Any]) -> None:
        """发送流式事件（可丢）；非法事件抛 :class:`ProtocolError`。"""
        self._validate_emit(event)
        self._emit_sink(event)

    def log(self, message: str, level: str = "info") -> None:
        """发送结构化日志（写 stderr 语义，由 supervisor 落 trace）。"""
        self._log_sink({"level": level, "message": message})

    def get_var(self, key: str) -> Any | None:
        """读取 run 级全局变量快照。"""
        return self._global_vars.get(key)

    def deadline_remaining_ms(self) -> int:
        """距实例 deadline 的剩余毫秒；未设 deadline 返回 ``2**63-1``。"""
        if self.deadline_ms <= 0:
            return 2**63 - 1
        now_ms: int = int(time.time() * 1000)
        return max(0, self.deadline_ms - now_ms)

    def now(self) -> float:
        """当前 Unix 时间（L2 可复现需由运行期记录；见架构 §5）。"""
        return self._start_epoch + (time.monotonic() - self._start_mono)

    def random(self) -> float:
        """由 seed 驱动的确定性随机数。"""
        return self._rng.random()

    def _validate_emit(self, event: dict[str, Any]) -> None:
        """校验 emit 事件的最小契约（type 必须合法）。"""
        if not isinstance(event, dict):
            raise ProtocolError("emit event must be a dict")
        kind: Any = event.get("type")
        if kind not in ALLOWED_EMIT_TYPES:
            raise ProtocolError(f"invalid emit type: {kind!r}")
        if kind == "progress":
            percent: Any = event.get("percent")
            if not isinstance(percent, int) or isinstance(percent, bool):
                raise ProtocolError("progress.percent must be int")
            if percent < 0 or percent > EMIT_MAX_PERCENT:
                raise ProtocolError("progress.percent out of range")
            if not isinstance(event.get("message"), str):
                raise ProtocolError("progress.message must be str")
        if kind == "token" and not isinstance(event.get("text"), str):
            raise ProtocolError("token.text must be str")
