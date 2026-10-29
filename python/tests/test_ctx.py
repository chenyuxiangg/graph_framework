"""WorkerContext 单测：emit 校验、取消、全局变量、截止时间、种子随机。"""

from __future__ import annotations

import threading

import pytest

from zgraph_worker.ctx import WorkerContext
from zgraph_worker.errors import WorkerCancelled
from zgraph_worker.protocol import ProtocolError


def _ctx(emitted: list[dict[str, object]], cancel: threading.Event) -> WorkerContext:
    return WorkerContext(
        run_id="r",
        instance_id="i",
        node_id="n",
        attempt=0,
        user_id="u",
        global_vars={"g": 1},
        params={"p": 2},
        deadline_ms=0,
        seed=42,
        emit_sink=emitted.append,
        log_sink=lambda _event: None,
        is_cancelled=cancel.is_set,
    )


def test_emit_progress_is_accepted() -> None:
    emitted: list[dict[str, object]] = []
    ctx = _ctx(emitted, threading.Event())
    ctx.emit({"type": "progress", "percent": 50, "message": "half"})
    assert emitted[0]["percent"] == 50


def test_emit_invalid_type_is_rejected() -> None:
    ctx = _ctx([], threading.Event())
    with pytest.raises(ProtocolError):
        ctx.emit({"type": "bogus"})


def test_emit_progress_out_of_range_rejected() -> None:
    ctx = _ctx([], threading.Event())
    with pytest.raises(ProtocolError):
        ctx.emit({"type": "progress", "percent": 101, "message": "x"})


def test_cancel_requested_and_check() -> None:
    cancel = threading.Event()
    ctx = _ctx([], cancel)
    ctx.check_cancelled()
    cancel.set()
    assert ctx.cancel_requested is True
    with pytest.raises(WorkerCancelled):
        ctx.check_cancelled()


def test_get_var_reads_snapshot() -> None:
    ctx = _ctx([], threading.Event())
    assert ctx.get_var("g") == 1
    assert ctx.get_var("missing") is None


def test_deadline_remaining_unset_is_large() -> None:
    ctx = _ctx([], threading.Event())
    assert ctx.deadline_remaining_ms() > 10**12


def test_seeded_random_is_deterministic() -> None:
    first = _ctx([], threading.Event())
    second = _ctx([], threading.Event())
    assert first.random() == second.random()
