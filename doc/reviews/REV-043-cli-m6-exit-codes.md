# REV-043 · cli · ISSUE-059 退出码统一 + REV-028 #6 CLI 偏差

- **分支**：`fix/cli-m6-exit-codes`
- **作者**：`cli-dev`（提交 `a995fb3`）
- **评审人**：`lead`（**≠ 作者**；独立评审）
- **变更范围**：`origin/main(a031269)..a995fb3`，3 文件（`crates/zgraph-cli/src/{lib.rs,validate.rs}`、`tests/validate.rs`，+287/−90）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-059；REV-028 #6 / DR-002 §9 #11；`doc/spec-v1.2/06` §6.10.1/§6.10.5、`02` §2.7.4；DR-003 §3 D1

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | `validate` 统一为 06 §6.10.5 权威口径：**0 合法 / 2 DSL·资产·用法 / 1 IO·内部 / 70 panic**，删除 M1 临时口径 `EXIT_USAGE_VALIDATE=3` 与 `EXIT_USAGE_TRACE` 别名（`run/replay/trace` 不变：入口输入 3、REPLAY_* 3、存储 4、SIGINT 130）。评审人实测：合法图=0、坏图=2、文件不可读=1、`--bogus`=2。REV-028 #6：`-v/--verbose` 全局参数映射 `RUST_LOG`（-v=debug、-vv=trace，显式 RUST_LOG 不覆盖）；颜色/`NO_COLOR` 决策为「v0.1 全输出纯文本、无 ANSI」，并有输出扫描测试守卫（实测 `--help` 无 ESC 字符）；clap 明确 v0.1 不引入（crate 文档写明理由）；`--version` 保持 `zgraph 0.1.0 (DSL 1.0)`。 |
| 代码结构合理性 | ✅ | 退出码常量收敛（删除两个临时常量）；`CliError` 构造器统一为 `usage()→2` 并新增 IO 分类；`-v` 解析/计数/映射拆成独立函数并带 5 个单测；未改其他 crate、未加依赖。 |
| 逻辑性 | ✅ | 未知子命令/缺参/非法选项 → 2；文件读取失败 → 1；校验失败（`DSL_*`）→ 2；`catch_unwind` → 70 保持。`-v` 可出现在子命令前后任意位置（实测 `-v --version` 正常），显式 `RUST_LOG` 优先。 |
| 可读性 | ✅ | 模块文档新增「v0.1 CLI 决策」段，说明 clap/-v/NO_COLOR/--version 的取舍与理由；常量注释标注 spec 章节。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（后置） | `doc/spec-v1.2/06 §6.10.1/§6.10.2`、`02 §2.7.4` | 上述 spec 注记仍写 M1 临时口径/「-v、NO_COLOR、clap 未实现」，已过时 | REV-056 回写；ISSUE-059 置 fixed 并注明 REV-043 | open（转 REV-056/台账） |
| 2 | 备注 | `USAGE` 文本 | `-v, --verbose` 行插入在子命令列表后，格式与子命令行不同（缩进差异） | 可接受；如后续统一排版可顺手调 | 无需处理 |
| 3 | 备注 | ISSUE-029 | `validate --structure-only`/`--print-hash` 仍未实现 | 既有台账项，不在 M6 范围 | 无需处理 |

无阻塞 / 重要项。e2e 无 validate 退出码断言，未受影响（评审人核对 `tests/e2e/**` 无相关断言）。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-cli --quiet
cargo build -p zgraph-cli
# 手工退出码（fixture 见 tests/fixtures/graphs/customer_ticket.json）
/tmp/zgraph-merge-target/debug/zgraph validate <合法图>            # 0
/tmp/zgraph-merge-target/debug/zgraph validate /tmp/badgraph.json  # 2
/tmp/zgraph-merge-target/debug/zgraph validate /nonexistent.json   # 1
/tmp/zgraph-merge-target/debug/zgraph validate --bogus             # 2
/tmp/zgraph-merge-target/debug/zgraph -v --version                 # zgraph 0.1.0 (DSL 1.0)；无 ANSI
```

- 结果：`zgraph-cli` 全部 test target **0 failed**（lib 60 / acceptance_m2 10 / replay 8 / run 24 / trace 11 / validate 11 / USAGE 集成 0 等，合计 124 用例）。
- 作者另报告：`cargo fmt --all --check` OK、`check.sh` PASS；未改 e2e。

## 5. 遗留问题

- 转台账：ISSUE-059 → fixed（REV-043）；spec 回写在 REV-056。
- 无阻塞项。
