"""zgraph worker SDK（M0）。

协议契约：``schemas/worker-protocol-v1.md``；设计依据：``doc/architecture-v2.md`` §3。
最小用法::

    from zgraph_worker import WorkerContext

    def run(inputs: dict, ctx: WorkerContext) -> dict:
        ctx.emit({"type": "progress", "percent": 10, "message": "start"})
        return {"echo": inputs.get("message")}
"""

from __future__ import annotations

from .ctx import WorkerContext
from .errors import BusinessError, WorkerCancelled, WorkerError
from .protocol import PROTOCOL_VERSION, SDK_VERSION, ProtocolError

__all__ = [
    "BusinessError",
    "PROTOCOL_VERSION",
    "ProtocolError",
    "SDK_VERSION",
    "WorkerCancelled",
    "WorkerContext",
    "WorkerError",
]
