# REV-054 · cli · ISSUE-081 cli 侧 >60 行函数拆分（纯重构）

- **分支**：`refactor/cli-m6-cli-split`
- **作者**：`cli-dev`（提交 `570c1bd`）
- **评审人**：`lead`（**≠ 作者**；独立评审；cli 单模块，无跨模块第二评审）
- **变更范围**：`main(3d227fc)..570c1bd`，5 文件（`crates/zgraph-cli/**`，+938/−555，纯私有 helper 与测试重排）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-081（cli 侧）；`architecture-v2.md` §2.2 等价约束；REV-053（harness 侧先例）；ISSUE-097/096（process 侧已知问题，wrapup 处理）

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | cli 内 >60 行函数 **13→0**（脚本按 `fn` 花括号配对、去注释/字符串统计；拆分后最大 60 行 = 既有 `bridge_maps_sink_calls_into_trace_rows`，未动）；`run_in_runtime` 132→8 helper（最大 47）、`parse` 79→`ParseState::{consume,set_graph,finish}`（最大 49）、`build_plan` 75→4 helper（最大 37）；8 个长测试拆为 setup/断言 helper 或多测试（全部断言保留，失败 run 与 filters 各拆 2 个测试）。公开 API/CLI 解析结果/退出码/stderr+stdout 文案不变；trace 落库语义不变；未改依赖。新增 helper 单测（ParseState、ensure_replayable、index_recorded_outputs（hash/重复/缺 output_json）、lifecycle_detail 两组）。评审人复跑 `cargo test -p zgraph-cli` 全绿（160/160，较拆分前 +7 测试）。 |
| 代码结构合理性 | ✅ | 拆分点选在解析状态机（ParseState）、运行生命周期阶段（open_writer/load/prepare/start/wait）、replay 记录索引与校验；helper 全部私有、职责单一；测试 helper 与断言分离，减少重复。 |
| 逻辑性 | ✅ | 状态机 consume/set_graph/finish 与原子 `parse` 语义等价（新增单测覆盖顺序/非法转换）；运行阶段顺序（recover→open→load→prepare→start→wait→metadata→report）保持；replay 记录索引不改变比较语义。 |
| 可读性 | ✅ | 主函数变为高层流程、helper 命名表达职责；测试名保持原语义。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 重要（登记） | `scripts/dev/check.sh` 第 5 项分支白名单 | 白名单缺 `refactor/*`（`worktree.sh` 与 AGENTS §1 均已支持 refactor），导致 refactor 分支自检恒红 | 登记 **ISSUE-097**（P2 process）；REV-061 wrapup 修 `check.sh`（case 加 `refactor/*`） | open（转 REV-061） |
| 2 | 重要（登记） | merge 门禁 / e2e 裸 bin | e2e 依赖 `<target>/debug/zgraph`，`cargo test --workspace` 不保证重建（ISSUE-096） | REV-061 在 merge.sh 测试前 `cargo build -p zgraph-cli --bin zgraph --locked`；本分支为纯重构，行为不变 | open（转 REV-061） |
| 3 | 备注 | `architecture §2.2` | ISSUE-081 全量（harness REV-053 + cli 本分支）拆分完成后，architecture §2.2 可标注 M6 已收敛 | REV-057 回写/§2.2 注记由 wrapup 或 docs 处理（若未覆盖则记 DI） | 后置 |

无阻塞项。`check.sh` 失败项为 process 白名单缺陷，非本分支内容；`merge.sh` 不调用 `check.sh`，其余 check 项与本分支测试/clippy 全绿。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-cli --quiet
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- 结果：`zgraph-cli` **160 passed / 0 failed**（lib 81 + acceptance_m2 10 + replay 13 + run 31 + trace 14 + validate 11）；workspace clippy `-D warnings` 通过（4.40s）。
- 作者另报告：`cargo fmt --all` 通过；check.sh 除分支白名单外全 PASS。

## 5. 遗留问题

- ISSUE-097（check.sh refactor 白名单）与 ISSUE-096（merge 门禁 build bin）→ REV-061 wrapup。
- ISSUE-081 可置 fixed（harness REV-053 + cli 本分支）→ wrapup 台账。
- 无阻塞项。
