# REV-060 · e2e · 适配工具并发共享全局 worker 上限（ISSUE-079 后续）

- **分支**：`test/e2e-m6-tool-limits`
- **作者**：`e2e-dev`（提交 `cd3f225`）
- **评审人**：`lead`（**≠ 作者**；独立评审；e2e 单模块、无需跨模块第二评审）
- **变更范围**：`main(398bf76)..cd3f225`，6 文件（全部 `tests/e2e/**`，3 改 + 3 新增 fixture，+194/−50）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-079；REV-052（工具并发共享上限）；DR-003 D7；`07` AC-12/AC-08

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 删除 ISSUE-079 旧口径（「工具走独立进程级 semaphore、不计入 with_limits」共 4 处注释/doc）；`routing_executor` 改 `HarnessExecutor::with_tool_limits(settings, worker.limits())`，工具调用与 python 节点共享同一 `WorkerLimits`（与生产 CLI 组合根同口径）；峰值度量改为**工具 + 节点合计**：agent 工具改用 AC-12 本地 `add` 副本（窗口 ~0.2s + 进程区间日志，run_tag/env 注入），`AGENT_INTERVALS_PER_RUN` 1→2，新增 `assert_tool_intervals_logged` 显式断言每个 agent run 有工具区间，区间总数断言 26→28（8×3 节点 + 2×(echo+tool)）；**峰值断言未放宽**（全局 ≤4 且 ≥2、每 run ≤2 且观测到 ==2），只是统计口径扩大。`harness_ac08` 两组合执行器同改 `with_tool_limits`。 |
| 代码结构合理性 | ✅ | 只改 `tests/e2e/**`；新 fixture 独立目录（`fixtures/ac12/agents|tools`），避免复用 AC-08 agent 的 add/explode/slow 三工具注册；`executor_with_limits` 文档明确注入方式；与 M5 旧基线分开记录，便于对照。 |
| 逻辑性 | ✅ | 工具区间与节点区间写入同一日志，峰值计算同时覆盖两类 worker；`run_tag` 经 env 注入保证跨 run 不串；窗口 0.2s 保证重叠可观测；连续 2 次通过（8+2：区间总 28、峰值 4、每 run `[2×8,1,1]`；10×同图：区间总 30、峰值 4、每 run `2×10`），总耗时 2.80s / 3.34s，比值 <10。 |
| 可读性 | ✅ | 模块文档区分 M5 旧口径（26 段）与 REV-060 新口径（28 段）；新增断言失败信息给出区间组/峰值；fixture 注释说明用途。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 备注 | `05 §5.6.5` / `§5.13 D13` | spec 仍写「工具不占每 run 许可」旧口径 | REV-057 回写（与 REV-052 记录一致） | open（转 REV-057） |
| 2 | 备注 | 耗时 | 工具副本 0.2s 窗口使 8+2 总耗时较 M5（2.46–2.54s）略增至 2.80–2.98s | 仍在比值 <10 上限内；可接受 | 无需处理 |

无阻塞 / 重要项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-e2e --quiet
```

- 结果：`zgraph-e2e` 全部 test target **0 failed / 1 ignored**（ac12 2 / cancel 2 / cli_ac01 4 / cli_replay 9 / concurrency 4 / fault_injection 7 / harness_ac08 5+1 / parallel 4 / timeout_kill 2 等，合计 39 passed）。
- 作者另报告：ac12 连续 2 次通过（峰值数据见 §2）、`cargo clippy -p zgraph-e2e --all-targets --locked -- -D warnings` clean、`cargo fmt --check` + `check.sh` PASS。

## 5. 遗留问题

- spec 05 §5.6.5/§5.13 D13 旧口径 → REV-057 回写。
- 无阻塞项。
