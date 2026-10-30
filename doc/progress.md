# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M6（进行中，2026-10-31）——**W1 全部完成并合入 main**（kickoff/process/kernel/trace/harness/cli/e2e 共 7 分支，REV-039–045）；M6 范围/波次见 [`decisions/DR-003-m6-scope-and-waves.md`](decisions/DR-003-m6-scope-and-waves.md)

| 波次 | 任务 | 状态 | REV |
|---|---|---|---|
| 0 | M6 kickoff：DR-003 + 进度/索引/README | done | REV-039 |
| 1 | kernel ISSUE-086 `fail_timeout` 补 `set_instance_error` | done | REV-040 |
| 1 | trace ISSUE-033 双通道（8192+1024，按发送序号有序） | done | REV-041 |
| 1 | harness ISSUE-075/077 preview 脱敏 + canonicalize | done | REV-042 |
| 1 | cli ISSUE-059 退出码统一 + REV-028 #6 CLI 偏差 | done | REV-043 |
| 1 | e2e ISSUE-089 测试质量 + AC-12 同图 10× | done | REV-044 |
| 1b | process ISSUE-068/078/073 依赖治理 + CI `--locked`（REV-037 顺延） | done | REV-045 |

**下一步（M6 W2/W3，W1 收口 REV-046 后继续）**
1. 跨模块：ISSUE-036 fail-stop 接线（REV-047）、ISSUE-082 runs 版本/模型参数列（REV-048）、ISSUE-087 `global_vars`（REV-049）。
2. cli/harness/docs：ISSUE-085 实时生命周期（REV-050）、ISSUE-080 reason 闭集（REV-051）、ISSUE-079 工具并发上限（REV-052）、ISSUE-081 函数拆分（REV-053/054）。
3. 收口：ISSUE-086 e2e 断言（REV-055）、ISSUE-088 多机加固（REV-056）、spec 回写（REV-057）、wrapup（REV-058，含 AC-12 连续 10 次重跑）。

**阻塞**
- 无硬阻塞。环境：cargo 必须 `CARGO_HOME=<repo>/.cargo-home` + 离线；**不得共用一个 `CARGO_TARGET_DIR` 并发构建**（跨 worktree 串产物，ISSUE-093），合并门禁前先 `cargo clean` workspace 成员；外部 CPU 争用会使时序测试 flaky（ISSUE-091，失败先重试/查争用）。
