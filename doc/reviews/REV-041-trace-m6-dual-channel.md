# REV-041 · trace · ISSUE-033 双通道（8192 无损 + 1024 流式）

- **分支**：`fix/trace-m6-dual-channel`
- **作者**：`trace-dev`（提交 `e7b144d`）
- **评审人**：`lead`（**≠ 作者**；独立评审）
- **变更范围**：`origin/main(a031269)..e7b144d`，7 文件（`crates/zgraph-trace/**`，+579/−76）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-033；`doc/spec-v1.2/06` §6.4.1/§6.4.2/§6.5.2；DR-003 §1/§3

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | `TraceHandle::send` → 无损通道 8192（阻塞背压）；`send_streaming` → 流式通道 1024（`try_send`，满/断开丢弃并累计 `streaming_dropped`）；写线程按发送端序号合并两条通道（`writer/thread.rs`），同一发送端「先流式后无损」顺序不被破坏；`Flush`/`Shutdown`/恢复类控制消息先排空流式通道再提交（barrier 覆盖两通道，关停不丢已入队事件）。新增 `tests/channels.rs` 7 用例覆盖顺序、容量、丢弃计数、flush 屏障（含 1024 条 > 单批 256 行）、close 排空。 |
| 代码结构合理性 | ✅ | 通道拓扑集中在 `writer/mod.rs`；写线程状态机新增 `lossless_head`/`stream_head` 两个队头，职责清晰；`TraceHandle` 公开 API 未变（仅新增公开常量 `STREAMING_CHANNEL_CAP`）；删除重构后无用的 `Aggregator::is_empty`。 |
| 逻辑性 | ✅ | 序号合并消除「先排空流式再处理无损」导致的 FK/顺序缺陷（`RunStarted` 之后流式段先落库触发 `events.run_id` FK 失败），已在 `channels.rs` 用 1024 条 progress flush 用例回归保护；fatal 错误语义、关停路径、断开检测保持原状。 |
| 可读性 | ✅ | 模块文档与常量注释说明「为什么」用 10ms 短切片轮询（std mpsc 无法 `select!` 两通道）；测试命名描述语义。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议 | `06 §6.4.1` / `writer/thread.rs` 模块文档 | 双通道未定义**跨发送端**全序；当前实现保证「同一发送端序号有序、跨发送端各通道 FIFO」 | 由 docs 回写（REV-056）在 06 §6.4.1 明确该口径；同时登记台账（见 §5） | open（转 REV-056/台账） |
| 2 | 备注 | `TraceHandle` | `trace_backpressure` 指标仍无采集（原 ISSUE-045） | 不在 ISSUE-033 范围；保持台账 ISSUE-045 | 无需处理 |
| 3 | 备注 | — | 单通道→双通道后写线程复杂度上升（+196 行） | 已由 channels 用例覆盖；后续如需可再抽 `OrderedMerge` 类型 | 后置建议 |

无阻塞 / 重要项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-trace --quiet
```

- 结果：`zgraph-trace` 全部 test target **0 failed**（lib 3 / api 6 / channels 7 / reader 7 / recovery 6 / roundtrip 8 / streaming 6 / reader-roundtrip 9 / doctest 1）。
- 作者另报告：`cargo fmt --all --check` OK、`check.sh` PASS。

## 5. 遗留问题

- 需登记台账：① 06 §6.4.1「跨发送端无序」口径由 REV-056 回写；② 共享 `CARGO_TARGET_DIR` 串产物属 M6 环境教训（由 Lead 在 M6 wrapup 记录，不改产品行为）。
- 无阻塞项。
