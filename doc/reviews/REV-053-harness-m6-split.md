# REV-053 · harness · ISSUE-081 剩余 >60 行函数拆分（纯重构）

- **分支**：`chore/harness-m6-split`（DR-003 计划名 `refactor/harness-m6-split`；`scripts/dev/worktree.sh` 的 type 白名单无 `refactor`，实际用 `chore`，语义为重构、无行为变化）
- **作者**：`harness-dev`（提交 `34d0c54`）
- **评审人**：`lead`（**≠ 作者**；独立评审）
- **变更范围**：`main(67bc49b)..34d0c54`，6 文件（`crates/zgraph-harness/src/**`，+655/−380）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-081（harness 侧）；`doc/architecture-v2.md` §2.2 等价约束；REV-027（M5 首轮拆分）

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | 纯提取重构：9 个 >60 行函数（`parse_agent_config` 113→32、`validate_schema_node` 105→26、`transport::chat_stream` 86→13、`executor::execute` 78→33、`fake::chat_stream` 75→27、`parse_base_url` 64→11、`execute_tool` 63→19、2 个测试 74→30 / 85→16）全部 ≤60；公开 API、错误码/reason/json_path、消息文本、事件顺序、超时/取消检查顺序不变；测试属性数拆分前后均 138（未删/未弱化），评审人复跑 `cargo test -p zgraph-harness` 137 passed / 0 failed / 1 ignored（与拆分前一致）。 |
| 代码结构合理性 | ✅ | 新 helper 全部为模块私有 `fn`/私有 struct（`StreamState`、`AgentCallJob`），职责单一；transport 拆分保留 cancel 预检→建流顺序与 `drain_stream` 的 `Box::pin` Unpin 语义；无新增 `unsafe`/`allow`/TODO。 |
| 逻辑性 | ✅ | 拆分点选在状态机分支/错误映射/超时检查边界，控制流等价；顺带修复 2 处 clippy（`needless_borrows_for_generic_args` 2 处、`unnecessary_lazy_evaluations` 1 处），均无行为变化。 |
| 可读性 | ✅ | helper 命名表达职责（`open_stream`/`drain_stream`/`finish_tool_call`/`validate_*_keyword`）；拆分后主函数读起来是高层流程。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 重要（后置登记） | merge 门禁 / CI | main（67bc49b）上 `cargo clippy -p zgraph-harness --all-targets -- -D warnings` 原本为 **红**（REV-042 引入 2 处 `needless_borrows`），说明 `merge.sh` 合并门禁未跑 clippy，只有 CI 跑；本次分支已修复 harness 侧 | M6 wrapup（REV-058）登记 ISSUE-094：`merge.sh` 增加 `cargo clippy --workspace --all-targets -- -D warnings` 门禁（或 CI 保持并明确「本地门禁不含 clippy」） | open（转 REV-058/台账） |
| 2 | 备注 | ISSUE-081 | cli 侧 `parse` 79 / `run_in_runtime` 91 / `replay::build_plan` 75 未在本分支处理 | 由 REV-054（cli 拆分）关闭；本记录只覆盖 harness 侧 | open（转 REV-054） |
| 3 | 备注 | 分支命名 | DR-003 计划 `refactor/harness-m6-split`，实际 `chore/harness-m6-split` | `worktree.sh` type 白名单缺 `refactor`（AGENTS §1 允许 refactor）→ wrapup 登记流程口径（ISSUE 或脚本修复） | open（转 REV-058） |

无阻塞项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-harness --quiet
cargo clippy -p zgraph-harness --all-targets --offline -- -D warnings
```

- 结果：`zgraph-harness` **137 passed / 0 failed / 1 ignored**；clippy `-D warnings` **通过**（34.26s）。
- 作者另报告：`cargo fmt --all --check` 通过、`check.sh` PASS、全仓 harness >60 行函数 = 0（最大 46）。

## 5. 遗留问题

- 转台账（REV-058）：merge.sh 缺 clippy 门禁（本分支暴露的 main 红）；worktree.sh 不支持 `refactor` 前缀。
- ISSUE-081 cli 侧由 REV-054 处理。
