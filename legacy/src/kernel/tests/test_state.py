"""state 测试（基础读写 + 锁）。"""

import os
import time
from pathlib import Path

import pytest

from src.kernel import StateLock, claim, clear_claim, new_instance_id, pid_alive, read_state, state_dir, state_path, write_state


def test_state_dir_creates_dir(tmp_path: Path):
    """测试名：test_state_dir_creates_dir

    测试场景：state_dir 路径计算并自动建子目录。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：调 state_dir(tmp_path, "my_sop")。
    预期结果：返回 tmp_path/my_sop，且该子目录已被创建。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = state_dir(tmp_path, "my_sop")
    assert p == tmp_path / "my_sop"
    assert p.is_dir()


def test_state_path_construction(tmp_path: Path):
    """测试名：test_state_path_construction

    测试场景：state_path 路径拼接规则——root/sop/instance.json。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：调 state_path(tmp_path, "my_sop", "abc123")。
    预期结果：等于 tmp_path/my_sop/abc123.json。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = state_path(tmp_path, "my_sop", "abc123")
    assert p == tmp_path / "my_sop" / "abc123.json"


def test_new_instance_id_length():
    """测试名：test_new_instance_id_length

    测试场景：new_instance_id 返回 12 位 hex 字符串。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：调 new_instance_id()。
    预期结果：长度 == 12 且字符全在 [0-9a-f] 内。
    测试后清理：无。
    """
    iid = new_instance_id()
    assert len(iid) == 12
    assert all(c in "0123456789abcdef" for c in iid)


def test_read_state_missing_returns_empty(tmp_path: Path):
    """测试名：test_read_state_missing_returns_empty

    测试场景：read_state 对不存在的文件返回空 dict。
    前置条件：pytest tmp_path 下无 missing.json。
    是否使用 mock：No。
    测试步骤：调 read_state(tmp_path/missing.json)。
    预期结果：返回 {}。
    测试后清理：pytest tmp_path 自动清理。
    """
    assert read_state(tmp_path / "missing.json") == {}


def test_write_and_read_state(tmp_path: Path):
    """测试名：test_write_and_read_state

    测试场景：write_state + read_state 往返一致。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：1. write_state(p, {"k":1, "msg":"hi"})；2. read_state(p)。
    预期结果：读出字典与写入字典完全相等。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "s.json"
    write_state(p, {"k": 1, "msg": "hi"})
    assert read_state(p) == {"k": 1, "msg": "hi"}


def test_write_state_atomic(tmp_path: Path):
    """测试名：test_write_state_atomic

    测试场景：连续两次 write_state 不留临时文件，最终值是最后一次。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：1. write_state(p, {"a":1})；2. write_state(p, {"a":2})；3. 扫 .s.json.*.tmp 残留。
    预期结果：无 .tmp 残留；read_state == {"a":2}。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "s.json"
    write_state(p, {"a": 1})
    write_state(p, {"a": 2})
    leftovers = list(tmp_path.glob(".s.json.*.tmp"))
    assert leftovers == []
    assert read_state(p) == {"a": 2}


def test_state_lock_blocks(tmp_path: Path):
    """测试名：test_state_lock_blocks

    测试场景：持锁期间第二次拿不到锁（短 timeout 抛 TimeoutError）。
    前置条件：pytest tmp_path 已建；p.parent 已建。
    是否使用 mock：No。
    测试步骤：1. StateLock(p, timeout=1.0) 进入；2. 在其作用域内再开 StateLock(p, timeout=0.2)。
    预期结果：内层抛 TimeoutError。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "s.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    with StateLock(p, timeout=1.0):
        with pytest.raises(TimeoutError):
            with StateLock(p, timeout=0.2):
                pass


def test_state_lock_releases(tmp_path: Path):
    """测试名：test_state_lock_releases

    测试场景：退出作用域后锁释放，下次进入不抛错。
    前置条件：pytest tmp_path 已建；p.parent 已建。
    是否使用 mock：No。
    测试步骤：1. 进入 StateLock(p, 1.0)；2. 正常退出；3. 再次进入 StateLock(p, 0.5)。
    预期结果：第二次进入不抛错（锁已释放）。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "s.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    with StateLock(p, timeout=1.0):
        pass
    # 第二次进入不应抛错
    with StateLock(p, timeout=0.5):
        pass


def test_pid_alive_self():
    """测试名：test_pid_alive_self

    测试场景：pid_alive 对当前进程返回 True，对非法 pid 返回 False。
    前置条件：无。
    是否使用 mock：No。
    测试步骤：pid_alive(os.getpid()) / pid_alive(-1) / pid_alive(0)。
    预期结果：True / False / False。
    测试后清理：无。
    """
    assert pid_alive(os.getpid()) is True
    assert pid_alive(-1) is False
    assert pid_alive(0) is False


def test_claim_and_clear(tmp_path: Path):
    """测试名：test_claim_and_clear

    测试场景：claim 写字段 → clear_claim 清字段。
    前置条件：pytest tmp_path 已建；p.parent 已建。
    是否使用 mock：No。
    测试步骤：1. claim(p, pid=os.getpid(), owner="tester")；2. read_state 校验字段；
      3. clear_claim(p)；4. 再次 read_state 校验字段已清。
    预期结果：claim 后含 claim_pid/claim_owner；clear 后两者皆消失。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "s.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    assert claim(p, pid=os.getpid(), owner="tester") is True
    state = read_state(p)
    assert state.get("claim_pid") == os.getpid()
    assert state.get("claim_owner") == "tester"
    clear_claim(p)
    state = read_state(p)
    assert "claim_pid" not in state


def test_claim_alive_pid_blocks(tmp_path: Path):
    """测试名：test_claim_alive_pid_blocks

    测试场景：同 pid 再次 claim 失败（pid 仍存活）。
    前置条件：pytest tmp_path 已建；p.parent 已建。
    是否使用 mock：No。
    测试步骤：1. claim(p, pid=os.getpid(), owner="tester")；2. 再次 claim(p, pid=os.getpid(), ...)。
    预期结果：第二次 claim 返回 False。
    测试后清理：clear_claim(p)。
    """
    p = tmp_path / "s.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    assert claim(p, pid=os.getpid(), owner="tester") is True
    # 同样 pid 再认领应失败（同一进程）
    assert claim(p, pid=os.getpid(), owner="tester") is False
    clear_claim(p)


def test_claim_dead_pid_succeeds(tmp_path: Path):
    """测试名：test_claim_dead_pid_succeeds

    测试场景：claim 一个不存在 pid 必然成功（compare 端直接判定为 stale）。
    前置条件：pytest tmp_path 已建；p.parent 已建。
    是否使用 mock：No。
    测试步骤：claim(p, pid=999_999, owner="ghost")。
    预期结果：返回 True。
    测试后清理：clear_claim(p)。
    """
    p = tmp_path / "s.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    # 用一个肯定不存在的 pid
    assert claim(p, pid=999_999, owner="ghost") is True
    clear_claim(p)


def test_claim_stale_after(tmp_path: Path):
    """测试名：test_claim_stale_after

    测试场景：旧 claim_ts 超过 stale_after 时，新 claim 可接管（同 pid 也行）。
    前置条件：pytest tmp_path 已建；p.parent 已建；state 中预置 claim_ts=now-10。
    是否使用 mock：No。
    测试步骤：1. write_state 写入旧 claim；2. claim(p, pid=os.getpid(), owner="new", stale_after=0.1)。
    预期结果：claim 返回 True（旧的已 stale）。
    测试后清理：pytest tmp_path 自动清理。
    """
    p = tmp_path / "s.json"
    p.parent.mkdir(parents=True, exist_ok=True)
    # 写入一个旧的 claim（pid 仍存活，但 ts 很久以前）
    write_state(p, {"claim_pid": os.getpid(), "claim_owner": "old", "claim_ts": time.time() - 10})
    # stale_after=0.1s → 10s 前的 claim 视为 stale，新 claim 可接管
    assert claim(p, pid=os.getpid(), owner="new", stale_after=0.1) is True
