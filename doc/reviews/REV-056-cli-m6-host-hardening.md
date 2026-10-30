# REV-056 · cli · ISSUE-088 崩溃标定多机/EPERM/host 加固

- **分支**：`fix/cli-m6-host-hardening`
- **作者**：`cli-dev`（提交 `db22f3a`）
- **评审人**：`lead`（**≠ 作者**；独立评审；cli 单模块，无跨模块第二评审）
- **变更范围**：`main(5192168)..db22f3a`，7 文件（`crates/zgraph-cli/**`，+589/−102；含 `Cargo.toml` cfg(unix) nix 加 `hostname` feature）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-088；`06` §6.8.1/§6.8.3；REV-030 #2–#5；ISSUE-048/084（M5 落地）

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | `current_host()` 改 **OS hostname**（Unix `nix::unistd::gethostname`，cli 的 cfg(unix) nix 依赖加 `hostname` feature；失败/非 Unix 回落 `HOSTNAME`/`COMPUTERNAME`，最后 `unknown`）；本机判定兼容环境变量别名（kernel M5 仍用 env 写 `owner_host`），忽略大小写。判活顺序：同机（host 已知且命中本机）→ 本地 pid；**跨机或 host 未知（None/空/字面 `unknown`）→ heartbeat 60s 陈旧性**（未陈旧 → 跳过 + 告警；陈旧/缺失/不可解析 → 判死），去掉「unknown 当同机」误判。`pid_alive`：Ok/EPERM → 存活、ESRCH → 不存在、其它 errno 保守存活（纯函数 `pid_alive_from_kill` 可注入 errno 单测）；非 Unix 保守 true。`trace` 进行中提示先校验 `owner_host`：仅同机且 Unix 才 `kill(pid,0)` 判活；跨机/未知 host 提示「可能在其他主机」不误报；非 Unix 不打印存活提示。`recover_interrupted` 返回 `{recovered, warnings}`，跨机/未知 host 且心跳新 → `warning: run <id> skipped recovery (...)`；`--json.warnings[]` 由恒空改为实际告警。**bridge 关键设计**：`RunStarted.owner_host` 落库前把 kernel 的 `unknown`/空替换为 OS hostname（已知值原样），避免本机（非交互 shell 下 `HOSTNAME` 未导出）被误判跨机、kill -9 恢复链路失效；未改 kernel/trace。 |
| 代码结构合理性 | ✅ | 判活/分类抽成纯函数（`pid_alive_from_kill`、`normalize_host`、`host_is_known`、`is_local_host`），可注入 errno/pid 探针/本机 host 集合；`Recovery { recovered, warnings }` 替代裸 HashMap 返回，CLI 组合根统一输出；bridge 归一化限定在 CLI 组合边界，未侵入 kernel 契约。 |
| 逻辑性 | ✅ | 判活顺序与 06 §6.8.1 一致；unknown 走 heartbeat 避免同号 pid 误杀；EPERM 视为存活（多用户共享机）；跨机新 heartbeat 跳过 + 告警符合 spec 字面；旧库遗留 `unknown` 行仍走 heartbeat；`:memory:` 等既有路径未受影响。 |
| 可读性 | ✅ | 提交正文与模块注释说明「为什么」在 bridge 兜底 host、兼容 env 别名的原因；测试命名表达语义（EPERM/ESRCH/unknown host/跨机新心跳/提示三种形态）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（登记） | kernel `host_name()` | kernel 仍只读环境变量（非交互 shell 下写 `unknown`）；CLI 路径已兜底，非 CLI 消费者/旧库仍走 heartbeat | 登记 ISSUE-095（P3 kernel）：`host_name` 迁移 OS hostname（后续专项）；在 06 §6.8.1 注明 CLI 兜底 | open（转 wrapup 台账/REV-057） |
| 2 | 重要（登记） | merge 门禁 / e2e 二进制发现 | e2e `cli_support` 优先用 `<target>/debug/zgraph` 裸 bin；`cargo test --workspace` 不保证重建它，可能复测旧 bin（作者实测踩到；本次定向 e2e 前手动 build 后通过） | 登记 ISSUE-096（P2 process）：merge.sh 在跑测试前 `cargo build -p zgraph-cli --bin zgraph --locked`；wrapup 分支实现 | open（转 REV-061） |
| 3 | 备注（行为变更） | CLI | `--json.warnings[]` 不再是恒空；跨机 Running run 的 trace stderr 提示文案变化；新 run 的 `runs.owner_host` 在 kernel 给 `unknown` 时写 OS hostname | 属 ISSUE-088 预期；REV-057 回写 06 §6.8.1/§6.8.3 | open（转 REV-057） |

无阻塞项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-cli --quiet
cargo clippy --workspace --all-targets --locked -- -D warnings
```

- 结果：`zgraph-cli` **153 passed / 0 failed**（lib 76 + acceptance_m2 10 + replay 13 + run 30 + trace 13 + validate 11）；workspace clippy `-D warnings` 通过（3.99s）。
- 作者另报告：定向 e2e `cli_ac01` 4/4、`cli_replay` 9/9（含 kill -9 崩溃标定恢复链路）通过（先 `cargo build -p zgraph-cli --bin zgraph`）；`cargo fmt --all`、`check.sh` PASS。

## 5. 遗留问题

- 06 §6.8.1/§6.8.3 回写（OS hostname/unknown→heartbeat/跨机跳过+告警/warnings[]/pid 提示 host 校验）→ REV-057。
- ISSUE-095（kernel host_name）与 ISSUE-096（merge 门禁 bin 构建）→ wrapup（REV-061）登记并修复 096。
- 无阻塞项。
