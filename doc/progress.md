# 开发进度

> 更新规则：任务开始 / 完成 / 阻塞 / 交接时更新；只保留当前状态（历史看 git 与 `doc/reviews/`）；全文件 ≤ 30 行。

**当前阶段**：M6（进行中，2026-10-30 开工）——M5 收尾已完成并合入 main；M6 范围/波次/REV 编号见 [`decisions/DR-003-m6-scope-and-waves.md`](decisions/DR-003-m6-scope-and-waves.md)

| 波次 | 模块 | 任务 | 状态 | REV |
|---|---|---|---|---|
| 0 | process/docs | M6 kickoff：DR-003 + progress/index/README | review | REV-039 |
| 1 | kernel | ISSUE-086 `fail_timeout` 补 `set_instance_error` | wip | REV-040 |
| 1 | trace | ISSUE-033 双通道（8192 + 1024） | wip | REV-041 |
| 1 | harness | ISSUE-075/077 preview 脱敏 + canonicalize | wip | REV-042 |
| 1 | cli | ISSUE-059 退出码统一 + REV-028 #6 CLI 偏差 | wip | REV-043 |
| 1 | e2e | ISSUE-089 测试质量 + AC-12 同图 10× | wip | REV-044 |
| 1b | process | ISSUE-068/078/073 依赖治理 + CI clippy（REV-037 顺延） | wip | REV-045 |

**下一步（M6 波次 2）**
1. 跨模块：ISSUE-036 fail-stop 接线（REV-046）、ISSUE-082 runs 版本/模型参数列（REV-047）、ISSUE-087 `global_vars`（REV-048）。
2. cli/harness/docs：ISSUE-085 实时生命周期（REV-049）、ISSUE-080 reason 闭集（REV-050）、ISSUE-079 工具并发上限（REV-051）、ISSUE-081 函数拆分（REV-052/053）。
3. 收口：ISSUE-086 e2e 断言（REV-054）、ISSUE-088 多机加固（REV-055）、spec 回写（REV-056）、wrapup（REV-057）。

**阻塞**
- 无硬阻塞。环境：本机无 `uv`（Python 测试用 `.venv/bin/python`）；`merge.sh` 传 `PYTHON_BIN`；worktree 共用 `CARGO_TARGET_DIR=<repo>/target` 省磁盘。
