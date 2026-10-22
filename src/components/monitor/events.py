"""事件常量（统一 SCH_EV_ 前缀）。"""

from __future__ import annotations

from typing import Final, Literal

Level = Literal["debug", "info", "warning", "error"]


class _Event:
    """单个事件项：name + level。"""

    __slots__ = ("name", "level")

    def __init__(self, name: str, level: Level) -> None:
        self.name = name
        self.level = level


class Event:
    """事件常量类。自动按 level 调 log.debug / info / warning / error。"""

    SCH_EV_NODE_START: Final = _Event("sch_ev_node_start", "info")
    SCH_EV_NODE_DONE: Final = _Event("sch_ev_node_done", "info")
    SCH_EV_NODE_FAIL: Final = _Event("sch_ev_node_fail", "warning")
    SCH_EV_GATE_DECISION: Final = _Event("sch_ev_gate_decision", "info")
    SCH_EV_TICK: Final = _Event("sch_ev_tick", "debug")
    SCH_EV_AWAITING_IPORT: Final = _Event("sch_ev_awaiting_iport", "debug")
    SCH_EV_OPORT_PRODUCED: Final = _Event("sch_ev_oport_produced", "info")
    SCH_EV_STATE_IO: Final = _Event("sch_ev_state_io", "debug")
    SCH_EV_CLAIM_RECLAIM: Final = _Event("sch_ev_claim_reclaim", "warning")
    SCH_EV_ERROR: Final = _Event("sch_ev_error", "error")
    SCH_EV_TIMEOUT: Final = _Event("sch_ev_timeout", "error")
    SCH_EV_SOP_DONE: Final = _Event("sch_ev_sop_done", "info")
    SCH_EV_SOP_ABORTED: Final = _Event("sch_ev_sop_aborted", "warning")
    SCH_EV_CYCLE_DETECTED: Final = _Event("sch_ev_cycle_detected", "info")
    SCH_EV_ROLLBACK: Final = _Event("sch_ev_rollback", "info")
    SCH_EV_EDGE_ADMITTED: Final = _Event("sch_ev_edge_admitted", "debug")
    SCH_EV_EDGE_REJECTED: Final = _Event("sch_ev_edge_rejected", "info")
    SCH_EV_MANUAL_PENDING: Final = _Event("sch_ev_manual_pending", "warning")