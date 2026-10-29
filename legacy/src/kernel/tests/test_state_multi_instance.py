"""state 多实例演练：5 进程并发同一 SOP 不同 instance_id。"""

from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

from src.kernel import StateLock, new_instance_id, read_state, state_dir, state_path, write_state


def _run_one(args):
    root, sop, instance_id, value = args
    p = state_path(root, sop, instance_id)
    with StateLock(p, timeout=2.0):
        current = read_state(p)
        current["counter"] = current.get("counter", 0) + value
        write_state(p, current)
    return p, read_state(p).get("counter")


def _bump(args):
    root, sop, instance_id = args
    p = state_path(root, sop, instance_id)
    with StateLock(p, timeout=2.0):
        current = read_state(p)
        current["counter"] = current.get("counter", 0) + 1
        write_state(p, current)


def test_multi_instance_independent(tmp_path: Path):
    """测试名：test_multi_instance_independent

    测试场景：5 进程各自跑独立 instance_id，互不干扰。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：1. 生成 5 个独立 instance_id；2. ProcessPoolExecutor(5) 并发跑 _run_one；
      3. 每个进程 StateLock 自己 instance 文件后 counter+=value。
    预期结果：5 个 path 都存在；每个 path 的 counter >= 1 且为 int。
    测试后清理：pytest tmp_path 自动清理。
    """
    sop = "my_sop"
    sd = state_dir(tmp_path, sop)
    args = [(tmp_path, sop, new_instance_id(), i + 1) for i in range(5)]
    with ProcessPoolExecutor(max_workers=5) as ex:
        results = list(ex.map(_run_one, args))
    assert len(results) == 5
    for path, counter in results:
        assert path.exists()
        assert isinstance(counter, int) and counter >= 1


def test_multi_instance_paths_isolated(tmp_path: Path):
    """测试名：test_multi_instance_paths_isolated

    测试场景：3 个 instance_id 各自的 state 文件互不覆盖。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：1. 生成 3 个 instance_id + 3 个 state_path；2. 每个路径写入 {"iid": iid}；
      3. 读回校验每个文件的 iid 字段等于自己的 iid。
    预期结果：每个 state.json["iid"] == 自己 instance_id。
    测试后清理：pytest tmp_path 自动清理。
    """
    sop = "iso_sop"
    ids = [new_instance_id() for _ in range(3)]
    paths = {iid: state_path(tmp_path, sop, iid) for iid in ids}
    for iid, p in paths.items():
        with StateLock(p, timeout=1.0):
            write_state(p, {"iid": iid})
    for iid, p in paths.items():
        assert read_state(p)["iid"] == iid


def test_concurrent_same_instance_serialized(tmp_path: Path):
    """测试名：test_concurrent_same_instance_serialized

    测试场景：同一 instance_id 被 10 进程并发写，StateLock 串行化保证 counter 不丢。
    前置条件：pytest tmp_path 已建。
    是否使用 mock：No。
    测试步骤：1. 1 个 instance_id + 10 个任务；2. ProcessPoolExecutor(10) 并发跑 _bump；
      3. 每个 _bump 内 StateLock 拿锁 → counter+=1 → write。
    预期结果：最终 counter == 10（无丢失）。
    测试后清理：pytest tmp_path 自动清理。
    """
    sop = "race"
    instance = new_instance_id()
    p = state_path(tmp_path, sop, instance)
    n = 10
    args = [(tmp_path, sop, instance) for _ in range(n)]
    with ProcessPoolExecutor(max_workers=n) as ex:
        list(ex.map(_bump, args))
    final = read_state(p).get("counter")
    assert final == n


def test_state_dir_makes_sop_subdir(tmp_path: Path):
    """测试名：test_state_dir_makes_sop_subdir

    测试场景：state_dir 自动建 SOP 子目录。
    前置条件：pytest tmp_path 已建；子目录尚未存在。
    是否使用 mock：No。
    测试步骤：调 state_dir(tmp_path, "create_me")。
    预期结果：返回路径等于 tmp_path/create_me 且 is_dir()。
    测试后清理：pytest tmp_path 自动清理。
    """
    sd = state_dir(tmp_path, "create_me")
    assert sd.is_dir()
    assert sd == tmp_path / "create_me"
