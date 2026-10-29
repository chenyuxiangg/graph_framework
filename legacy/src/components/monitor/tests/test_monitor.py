"""Monitor.emit / reap_stale / detect_progress 测试。"""

from __future__ import annotations

from pathlib import Path

from src.components.monitor import Event, Monitor


class _FakeLogger:
    """记录调用的 method 名称 + 消息 + 字段。"""

    def __init__(self) -> None:
        self.calls: list[tuple[str, str, dict]] = []

    def debug(self, msg: str, *pairs, **kw) -> None:
        self.calls.append(("debug", msg, kw))

    def info(self, msg: str, *pairs, **kw) -> None:
        self.calls.append(("info", msg, kw))

    def warn(self, msg: str, *pairs, **kw) -> None:
        self.calls.append(("warn", msg, kw))

    def error(self, msg: str, *pairs, **kw) -> None:
        self.calls.append(("error", msg, kw))


def test_emit_routes_to_correct_level() -> None:
    """测试名：test_emit_routes_to_correct_level

    测试场景：Monitor.emit 按 Event.level 路由到 logger 的 debug/info/warn/error。
    前置条件：_FakeLogger 记录调用。
    是否使用 mock：Yes（_FakeLogger 自录方法调用）。
    测试步骤：1. 依次 emit SCH_EV_TICK/SCH_EV_SOP_DONE/SCH_EV_ERROR/SCH_EV_NODE_FAIL；
      2. 收集 log.calls 的 method 名。
    预期结果：methods == ["debug", "info", "error", "warn"]。
    测试后清理：无。
    """
    log = _FakeLogger()
    m = Monitor(log)
    m.emit(Event.SCH_EV_TICK, phase="enter")
    m.emit(Event.SCH_EV_SOP_DONE, sop="x")
    m.emit(Event.SCH_EV_ERROR, detail="oops")
    m.emit(Event.SCH_EV_NODE_FAIL, node="a")

    methods = [c[0] for c in log.calls]
    assert methods == ["debug", "info", "error", "warn"]


def test_reap_stale_claims_returns_alarms(tmp_path: Path) -> None:
    """测试名：test_reap_stale_claims_returns_alarms

    测试场景：含死亡 pid + 极旧 ts 的 state.json 被 reap_stale_claims 识别为 stale。
    前置条件：tmp_path/sop/abc.json 预写一份带 claim_pid=9999999 / claim_ts=999999999999 的 state。
    是否使用 mock：Yes（_FakeLogger 自录）。
    测试步骤：1. 写 fake state.json；2. m.reap_stale_claims(tmp_path, "sop", stale_after=1.0)。
    预期结果：alarms 含 "abc"。
    测试后清理：pytest tmp_path 自动清理。
    """
    import json
    sop_dir = tmp_path / "sop"
    sop_dir.mkdir()
    sf = sop_dir / "abc.json"
    sf.write_text(json.dumps({
        "sop_name": "sop", "instance_id": "abc", "graph_name": "sop",
        "current_node": "a",
        "history": [],
        "triggered_edges": {},
        "enter_cnt": {},
        "exit_cnt": {},
        "last_output": {},
        "claim_pid": 9_999_999,  # 不可能存在的 pid
        "claim_ts": 999_999_999_999.0,
    }))
    log = _FakeLogger()
    m = Monitor(log)
    alarms = m.reap_stale_claims(tmp_path, "sop", stale_after=1.0)
    assert "abc" in alarms


def test_reap_stale_claims_skips_when_no_claim(tmp_path: Path) -> None:
    """测试名：test_reap_stale_claims_skips_when_no_claim

    测试场景：无 claim 字段时 reap_stale_claims 返回空 alarms。
    前置条件：tmp_path/sop/abc.json 不含 claim_pid/claim_ts。
    是否使用 mock：Yes（_FakeLogger 自录）。
    测试步骤：1. 写无 claim 字段 state；2. m.reap_stale_claims(... stale_after=1.0)。
    预期结果：alarms == []。
    测试后清理：pytest tmp_path 自动清理。
    """
    import json
    sop_dir = tmp_path / "sop"
    sop_dir.mkdir()
    sf = sop_dir / "abc.json"
    sf.write_text(json.dumps({
        "sop_name": "sop", "instance_id": "abc", "graph_name": "sop",
        "current_node": "a",
        "history": [], "triggered_edges": {},
        "enter_cnt": {}, "exit_cnt": {}, "last_output": {},
    }))
    log = _FakeLogger()
    m = Monitor(log)
    alarms = m.reap_stale_claims(tmp_path, "sop", stale_after=1.0)
    assert alarms == []


def test_detect_progress_reports_manual_pending(tmp_path: Path) -> None:
    """测试名：test_detect_progress_reports_manual_pending

    测试场景：history 空 + 有 triggered_edges + claim_ts 极旧 → MANUAL_PENDING 告警。
    前置条件：tmp_path/abc.json 预写带 triggered_edges + 极旧 claim_ts。
    是否使用 mock：Yes（_FakeLogger 自录）。
    测试步骤：1. 写 fake state.json；2. 构造最小 fake Graph(node_index={"a": fake_node})；
      3. m.detect_progress(g, sf, tmp_path)。
    预期结果：alarms 含 "MANUAL_PENDING"。
    测试后清理：pytest tmp_path 自动清理。
    """
    import json
    import time
    sf = tmp_path / "abc.json"
    sf.write_text(json.dumps({
        "sop_name": "sop", "instance_id": "abc", "graph_name": "sop",
        "current_node": "a",
        "history": [],
        "triggered_edges": {"e1": "alice"},
        "enter_cnt": {},
        "exit_cnt": {},
        "last_output": {},
        "claim_ts": time.time() - 1000.0,  # 1000 秒前 → age 远超 60
    }))
    log = _FakeLogger()
    m = Monitor(log)
    from src.kernel import Graph
    n = type("N", (), {"attr": type("A", (), {"is_src": True})()})()
    g = type("G", (), {"node_index": {"a": n}})()
    alarms = m.detect_progress(g, sf, tmp_path)
    assert "MANUAL_PENDING" in alarms
