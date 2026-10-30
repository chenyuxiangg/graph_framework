# REV-050 · cli · ISSUE-085 human 模式生命周期实时输出

- **分支**：`fix/cli-m6-human-realtime`
- **作者**：`cli-dev`（提交 `050fcd4`）
- **评审人**：`lead`（**≠ 作者**；独立评审）
- **变更范围**：`main(908c87e)..050fcd4`，3 文件（`crates/zgraph-cli/src/{bridge.rs,run_cmd.rs}`、`tests/run.rs`，+571/−129）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-085；`doc/spec-v1.2/06` §6.10.3/§6.10.6；DR-003 §3 D6

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | bridge 新增只读旁路 `LifecycleObserver`/`BridgeObservers`：映射后事件在 `send` 之前回调一次，**不写库、不改变 trace 语义**；`run_cmd` 组合根在 human 模式（非 `--json`、非 `-q`）订阅旁路，经 mpsc + 独立打印线程逐行输出 `[seq N] Kind ...`，`created` 行之后启动打印、run 结束用哨兵 + join 收尾，**删除结束后 trace 回读**；`:memory:` 同样走旁路；`--json` stdout 契约与 `-q` 抑制不变；`--stream` 仍负责 token 原文。新增测试证明真实 CLI 慢图（3s slow worker）下 `[seq 1] RunStarted`/Transition 行在**子进程仍运行**时已出现；`--json` 无生命周期行；bridge 旁路不改变落库 events 行数与顺序。 |
| 代码结构合理性 | ✅ | 旁路是「观察者」而非新数据通道：Observer trait/struct 集中在 bridge；打印线程与生命周期格式化从 run_cmd 抽象为 `LifecyclePrinter` + 格式化函数；`start_traced_run`/`run_in_process` 公开签名不变（e2e 无影响），仅内部启动变体改收 `BridgeObservers`。 |
| 逻辑性 | ✅ | 启动期事件先排队、`created` 行后启动打印线程保证行序；停止哨兵 + join 在 run_output/摘要前完成，避免 stderr 交错收尾；不订阅时零开销；token/reasoning 增量不逐块成行（trace 侧按 100ms/4096 聚合成段，旁路拿不到段边界）——`--stream` 提供原文，属可接受口径，已入 crate 文档；`[seq N]` 为 CLI 侧事件序号，单 run 新库无 token 段时与 trace `events.seq` 一致，含流式增量时可能小于聚合段编号（已注明）。 |
| 可读性 | ✅ | crate 文档与提交说明写明两处口径（聚合段、seq 语义）；测试名描述语义（实时性、`--json` 无行、`:memory:`、落库不变）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 备注（口径） | `06 §6.10.3` / 旁路 | token/reasoning 增量不逐块成行（trace writer 聚合段旁路不可见），`--stream` 提供原文 | REV-057 回写 06 §6.10.3 注明该口径；若要求聚合段行需 trace writer 侧旁路（另立任务） | open（转 REV-057） |
| 2 | 备注（口径） | 生命周期行 `[seq N]` | CLI 侧事件序号：单 run 新库且无 token 段时与 trace `events.seq` 一致；含流式增量时可能小于聚合段编号 | REV-057 在 06 §6.10.3 注明「seq 为 CLI 侧事件序号」 | open（转 REV-057） |
| 3 | 备注（交互） | human + `--stream` | 原始 token 文本与生命周期行共用 stderr，可能交错（两者均实时） | 可接受；`--json`/`-q` 无此问题 | 无需处理 |

无阻塞 / 重要项；未改 trace/kernel（D6 按「bridge 内存旁路」落地，无跨模块）。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-cli --quiet
cargo clippy -p zgraph-cli --all-targets --locked --offline -- -D warnings
```

- 结果：`zgraph-cli` **129 passed / 0 failed**（lib 62 + acceptance_m2 10 + replay 8 + run 27 + trace 11 + validate 11 + 其它）；clippy `-D warnings` 通过（3.11s）。
- 作者另报告：`cargo fmt --all` OK、`check.sh` PASS、真二进制手动 smoke（human 行序 / `--json` stderr 空 / `-q` 0 字节）。

## 5. 遗留问题

- 06 §6.10.3 回写「human 实时 + token 聚合段由 `--stream` 提供、seq 为 CLI 侧序号」→ REV-057。
- ISSUE-085 可置 fixed（REV-050，REV-059 wrapup 时更新台账）。
