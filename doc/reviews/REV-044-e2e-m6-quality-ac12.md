# REV-044 · e2e · ISSUE-089 测试质量 + AC-12 同图 10 并发加强

- **分支**：`test/e2e-m6-quality-ac12`
- **作者**：`e2e-dev`（提交 `75c4f70`）
- **评审人**：`lead`（**≠ 作者**；独立评审）
- **变更范围**：`origin/main(a031269)..75c4f70`，6 文件（`tests/e2e/**`，含 Cargo.toml）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-089（REV-032 发现）；`doc/spec-v1.2/07` AC-12/§7.2/§7.3#3；DR-003 §3 D11/DoD#2

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ISSUE-089 四项全处理：① `DbSnapshot` 改为**先取主文件 + `-wal` + `-shm` 字节、再开任何连接**，`assert_replay_is_read_only` 同序比较三文件（作者实测 `db=192512B / -wal=0B / -shm=32768B` 前后逐字节一致，说明确实覆盖 WAL/SHM）；② 三个 >60 行测试函数拆分（91→24、65→9、66→18），全 e2e 扫描无 >60 行函数；③ `tests/e2e/Cargo.toml` description 更新为 M6 + AC-01..AC-12；④ `zgraph_command()` / `common::worker_config` 统一注入 `PYTHONDONTWRITEBYTECODE=1`，并新增断言验证 env 真传到 worker（fixtures 目录无 `__pycache__`）。AC-12 加强：新增 `ac12_ten_same_graph_runs_are_isolated_and_bounded`（10 个同一图并发 run、同库不同 run_id），断言逐 run instances(5)/gate_results(6)/events 形状、跨 run `instance_id` 互斥、`user_id`/`global_vars` 不串、10 run `graph_hash` 一致（同图证据）、全局峰值 4 / 每 run 峰值 2、总耗时/基线比 <10；既有 8+2 用例保留并通过。 |
| 代码结构合理性 | ✅ | 新增 helper 收敛到 `cli_support/mod.rs`（`zgraph_command`/`db_files_snapshot`），`ac12.rs` 的 `launch_runs` 参数化为 plan 工厂、`assert_worker_limits` 按 runs 计算，避免硬编码；只改 `tests/e2e/**`，未触碰产品代码。 |
| 逻辑性 | ✅ | 只读断言顺序正确（快照先于 RW 查询），避免自证；10×用例沿用 M5 超时预算与峰值采样口径；`--test-threads=1` 在作者私有 target 下连续 2 次通过（总耗时/基线比 4.04/4.15）。 |
| 可读性 | ✅ | 模块文档补充两用例与实测基线；helper 命名自解释；新增用例名描述语义。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 备注 | `07 §7.2` DoD | spec 要求「连续 10 次重跑无 flaky」；本分支开发期验证为连续 2 次 | 收口时（REV-057）对新增 AC-12 用例执行连续 10 次重跑并记录；开发期 2 次仅快速门禁（DR-003 DoD#2） | open（转 REV-057） |
| 2 | 备注 | 共享 target 串产物 | e2e-dev 初期的 `FOREIGN KEY` 偶发失败经私有 target 干净构建不复现，确认属跨 worktree 产物污染，非 main 缺陷 | 已在 M6 环境处置（Lead 删除共享 target、改用带 clean 的合并 target）；wrapup 记录教训 | 无需处理 |

无阻塞 / 重要项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-e2e --quiet
```

- 结果：`zgraph-e2e` 全部 test target **0 failed / 1 ignored**（ac12 2、cancel 2、cli_ac01 3、cli_replay 9、concurrency 4、fault_injection 7、harness_ac08 5+1 ignored、parallel 4、timeout_kill 2）。
- 作者另报告：`cargo fmt --check` OK、`check.sh` PASS；私有 target 下 `cargo test --workspace` 553 passed / 0 failed。

## 5. 遗留问题

- AC-12 连续 10 次重跑 → REV-057 收口执行。
- 无阻塞项。
