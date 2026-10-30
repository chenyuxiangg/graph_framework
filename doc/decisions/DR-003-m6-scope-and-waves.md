# DR-003 · M6 范围、波次与收口决策

> **状态**：生效（M6 开发期决策记录；实现结论由 M6 评审记录与 spec 回写固化）
> **日期**：2026-10-30 · **作者**：lead
> **关联**：`doc/progress.md` M6 清单、`doc/issues.md`（ISSUE-033/036/059/068/073/075/077/078/079/080/081/082/085/086/087/088/089）、`doc/decisions/DR-002-m5-cli-replay-recording.md` §7/§9、`doc/spec-v1.2/05`（§5.2.1/§5.8.5）、`06`（§6.4.1/§6.4.5/§6.7/§6.8/§6.10）、`07`（AC-06/AC-12）、`08`（§8.7）、`doc/architecture-v2.md` §2.2/§5.2
> **评审**：REV-039（process/docs，独立评审人）；实现分支评审见 §6 波次表。
> **模块声明（跨模块）**：本分支改 `doc/**`（docs 模块：DR-003、progress、index）与根 `README.md`（process 模块，Lead 为 process 负责人），按 AGENTS §2 属跨模块改动，由非作者评审人在 REV-039 声明并评审。

本文件冻结 M6 的范围、波次、REV 编号与关键落地口径，供各模块开发/评审引用。**不改变 v1.0 契约**：spec 正文优先；本文件只记录 spec 未写死或 M6 才收口的部分。

## 1. M6 范围（M5 顺延项全集：17 个 ISSUE + DR-002 §9 #11 CLI 偏差 + REV-037 + AC-12 加强）

| ISSUE | 级别 | 模块 | 内容 | 波次 / REV |
|---|---|---|---|---|
| ISSUE-033 | P2 | trace | 双通道（无损 8192 + streaming 1024） | W1 / REV-041 |
| ISSUE-036 | P2 | trace+kernel+cli | trace 写失败 → fail-stop：取消 run → exit 4 | W2 / REV-046（跨模块双评审） |
| ISSUE-059 | P3 | cli | `validate` 用法退出码统一（2 vs 3） | W1 / REV-043 |
| ISSUE-068 | P3 | process | `nix` 提升 workspace.dependencies（含 e2e dev-dep 重复声明） | W1 / REV-045 |
| ISSUE-073 | P2 | process/harness | genai `preserve_order` 副作用评估 + CI clippy 固化 | W1 / REV-045 |
| ISSUE-075 | P3 | harness | `args/result_summary.preview` 脱敏 | W1 / REV-042 |
| ISSUE-077 | P3 | harness | prompt/constraints 路径 canonicalize（symlink 逃逸） | W1 / REV-042 |
| ISSUE-078 | P3 | process | `futures-util` 提升 workspace.dependencies | W1 / REV-045 |
| ISSUE-079 | P3 | harness(+cli) | 工具并发纳入全局 worker 上限（architecture §8 D4；`HarnessCtx` 无 run_id） | W2 / REV-051 |
| ISSUE-080 | P3 | harness/docs | tool discover failure reason 闭集复核/文档化 | W2 / REV-050（docs 主责） |
| ISSUE-081 | P3 | harness / cli | 剩余 >60 行生产函数与测试函数拆分 | W2 / REV-052（harness）、REV-053（cli） |
| ISSUE-082 | P2 | harness+cli+trace | `runs.worker_versions_json/model_params_json` 接线 | W2 / REV-047（跨模块双评审） |
| ISSUE-085 | P3 | cli | human 模式生命周期行实时输出（非结束后回读） | W2 / REV-049 |
| ISSUE-086 | P3 | kernel | `agent_call` 自报超时路径补 `set_instance_error` | W1 / REV-040（e2e 断言 W3 / REV-054） |
| ISSUE-087 | P3 | trace+cli | L2 记录/重放补 `global_vars`（schema v3） | W2 / REV-048（跨模块） |
| ISSUE-088 | P3 | cli(+trace) | 崩溃标定多机/EPERM/host 加固 | W3 / REV-055 |
| ISSUE-089 | P3 | e2e | L2 只读断言含 WAL/SHM、测试函数拆分、description、`__pycache__` | W1 / REV-044 |
| REV-028 #6（DR-002 §9 #11） | — | cli | pre-existing CLI 偏差：clap 决策/`-v`/`--version`/`NO_COLOR`（`--version` 字面按 06 目标口径复核） | W1 / REV-043 |
| REV-037 顺延 | — | process | 依赖治理 + CI clippy 固化（= 068/078/073 的载体） | W1b / REV-045 |
| AC-12 加强 | — | e2e | 同一图 10 并发 run（07 AC-12 原文字面形态） | W1 / REV-044 |

## 2. 波次与分支（分支命名 `<type>/<module>-<slug>`，worktree `.worktrees/<module>-<agent>`）

| 波次 | 分支 | 模块 | 评审 | 说明 |
|---|---|---|---|---|
| 0 | `docs/process-m6-kickoff` | process/docs | REV-039 | 本文件 + progress/index/README |
| 1 | `fix/kernel-m6-timeout-error` | kernel | REV-040 | ISSUE-086 |
| 1 | `fix/trace-m6-dual-channel` | trace | REV-041 | ISSUE-033 |
| 1 | `fix/harness-m6-preview-canonicalize` | harness | REV-042 | ISSUE-075/077 |
| 1 | `fix/cli-m6-exit-codes` | cli | REV-043 | ISSUE-059 + REV-028 #6 CLI 偏差 |
| 1 | `test/e2e-m6-quality-ac12` | e2e | REV-044 | ISSUE-089 + AC-12 10× |
| 1b | `chore/process-m6-deps` | process | REV-045 | ISSUE-068/078/073（REV-037 顺延） |
| 2 | `fix/trace-m6-failstop` | trace+kernel+cli | REV-046 | ISSUE-036（跨模块，双评审） |
| 2 | `feat/harness-m6-run-columns` | harness+cli+trace | REV-047 | ISSUE-082（跨模块，双评审） |
| 2 | `feat/trace-m6-global-vars` | trace+cli | REV-048 | ISSUE-087（schema v3，单独评审） |
| 2 | `fix/cli-m6-human-realtime` | cli | REV-049 | ISSUE-085 |
| 2 | `docs/docs-m6-reason-closed-set` | docs | REV-050 | ISSUE-080 |
| 2 | `fix/harness-m6-tool-limits` | harness(+cli) | REV-051 | ISSUE-079 |
| 2 | `refactor/harness-m6-split` | harness | REV-052 | ISSUE-081 harness 侧 |
| 2 | `refactor/cli-m6-split` | cli | REV-053 | ISSUE-081 cli 侧 |
| 3 | `test/e2e-m6-final-asserts` | e2e | REV-054 | ISSUE-086 实例 error_code e2e + 回归 |
| 3 | `fix/cli-m6-host-hardening` | cli(+trace) | REV-055 | ISSUE-088 |
| 3 | `docs/docs-m6-writeback` | docs | REV-056 | spec 05/06/07/08 + architecture §5.2 + PROVENANCE 回写 |
| 3 | `docs/process-m6-wrapup` | process/docs | REV-057 | 台账/进度/README/index/评审索引/基线收口 |

> REV-037 从未创建（M5 顺延 M6）；M6 起评审编号从 **REV-039** 连续分配（REV-038 已用）。上表编号为预留，若某分支合并顺序变化**不改编号**，以实际评审记录为准。

## 3. 关键决策（spec 未写死或 M5 遗留口径）

- **D1 · ISSUE-059 退出码**：以 06 §6.10.5 权威表为准——`validate`：**0** = 合法、**2** = DSL/资产错误**与用法错误**、**1** = IO/内部错误、`--help` = 0；02 §2.7.4 的 3 为 M1 临时口径，M6 统一后由 docs 回写（REV-056）。若统一与 e2e 既有断言冲突，由 e2e 分支（REV-054）同步，**不得**为迁就测试保留双口径。
- **D2 · ISSUE-073 genai**：保持 `genai 0.7.2`；**不**为规避 `preserve_order` 做 feature-gate（会改变默认构建的 provider 能力语义）；缓解 = `WorkerFailure::Worker(Box<WorkerError>)`（REV-021）+ CI clippy `-D warnings` + 尺寸回归关注。ISSUE-073 以「接受现状 + 守卫」收口（wontfix，理由入台账）；升级/替代另立 v0.2 任务。
- **D3 · ISSUE-082 版本/模型参数列**：`worker_versions_json` 的数据源为 worker 握手 `hello`（协议 v1 已含 `python_version`/`sdk_version`），由 supervisor 汇总 → CLI 去重排序后写；`model_params_json` 由 CLI 从 AgentConfig `model` 段摘要（provider/model/温度/上限等，**不含密钥**）写。trace 只新增「更新 `runs` 这两列」的原语；写入时点为 run 结束（失败 run 也尽力写）。
- **D4 · ISSUE-087 global_vars**：trace schema 升 **v3**（`user_version` 2→3，forward-only），新增 `runs.global_vars_json`（canonical JSON）；`run` 启动写、`replay --mode=outputs` 用它重建运行环境。迁移按 06 §6.7 执行：**每次迁移前 `VACUUM INTO '<db>.bak.v{old}-{ts}'` 备份**、单迁移单事务、失败恢复备份 + exit 4（即 ISSUE-039 的「迁移前备份」子项随 REV-048 顺带关闭，权限 0600/0700 子项仍留 v0.2）；schema 变更单独评审（REV-048）。
- **D5 · ISSUE-036 fail-stop**：kernel 在事件循环内轮询 trace 写端错误（`TraceHandle::take_error`），命中 → 内存态 `Failed`（`error_code` 保持 NULL，`STORAGE_ERROR` 属 CLI/storage 命名空间，见 06 §6.4.5）→ 尽力写终态 → CLI 退出码 **4**。故障注入用测试钩子（不依赖真实 ENOSPC）。
- **D6 · ISSUE-085 human 实时**：CLI 组合根订阅 bridge 的**内存事件旁路**（mpsc），逐行输出生命周期；不再 run 结束后回读 trace。`--json` 的 stdout 契约不变；`:memory:` 同样走旁路。
- **D7 · ISSUE-079 工具并发上限**：`HarnessCtx` 增 `run_id` 与可选全局 limiter 引用；工具调用 permit 与 `WorkerExecutor` 全局 permit **共享同一 limiter 实例**（CLI 组合根注入；默认容量仍 CPU×2，每 run 8 与节点执行共享）。不改 worker 协议。
- **D8 · ISSUE-088 多机加固**：`current_host()` 改用 OS hostname（Unix `libc::gethostname` 或等价；非 Unix 回落现有 env）；host 未知 → 走 heartbeat 判活路径；`pid_alive` 对 **EPERM 视为存活**；`trace` 的 pid 提示校验 `owner_host`；跨机 heartbeat 未陈旧 → 跳过 + 写入 `--json.warnings`（06 §6.8.1 字面要求）。若需新增 `libc` 依赖，随 process 治理提升到 workspace.dependencies。
- **D9 · ISSUE-081 拆分**：纯重构、行为不变；生产函数与测试函数均 ≤60 行；harness 侧（`parse_agent_config`/`validate_schema_node`/`transport::chat_stream`/`fake::chat_stream`/`executor::execute`/`parse_base_url`/`execute_tool`）与 cli 侧（`parse`/`run_in_runtime`/`replay::build_plan`）分别独立分支与评审。
- **D10 · ISSUE-080**：05 §5.6.2.1（REV-023）已冻结 `discover` 的实况闭集，M6 只做**复核并收敛**到 05 §5.2.7 / 00 §0.4 的 ConfigLoad reason 闭集（差异则补注 05），不新增错误码。
- **D11 · AC-12 加强**：**新增**「同一图 10 并发 run」用例（不替换 M5 的 8+2 组合）；断言 run 间 instances/gate_results/events 隔离、global_vars/user_id 不串、全局与每 run 上限生效；沿用 M5 超时预算与峰值采样口径，至少连续 2 次通过。
- **D12 · M6 排除（不在本里程碑处理）**：ISSUE-014（自定义 Python 门控）、017（worker 池化）、028（fixture 路径改名）、029（JSON Schema 资产/CLI 选项）、030/031/032（依赖与 DSL 定位细则）、035/037–041（trace 细节；039 仅「迁移备份」子项随 REV-048 关闭）、044/045（kernel 细节）、047（M2 验收口径）、049/051/055–058（cli/trace 细节）、060、062–065（e2e 已知边界）、069–072（worker/SDK 细节）、083。这些留 v0.2 或后续专项，M6 **不**顺手改。本表只列与 M6 相关的排除项；其余 `open/in-progress` 项（如 003/005/006/009/011/020）见 `doc/issues.md`，不在 M6 承诺范围。
- **D13 · 流程**：每分支必须独立评审（评审人 ≠ 作者）；评审记录 `doc/reviews/REV-0NN-<module>-<slug>.md`（四维 + 发现清单 + 验证命令）；合并只用 `scripts/dev/merge.sh <branch> --push`；合并后 Lead 更新 `doc/progress.md` 与 `doc/issues.md`。跨模块口径：`+`（036/047/048/082）与 `(+x)`（079/088，仅在必需时触及 x）= **同分支多模块改动**，均由 Lead 在评审记录声明并指定第二评审人；若最终只改单模块，则在记录中注明无需第二评审。

## 4. 验收（M6 DoD）

1. §1 表内 17 个 ISSUE 全部**收口**：16 个 `fixed`；ISSUE-073 按 D2 记 `wontfix` 并写明理由；REV-037 顺延项与 REV-028 #6 一并关闭；
2. AC-12「同一图 10 并发 run」e2e 通过，且**收口时连续 10 次重跑无 flaky**（07 §7.2/§7.3 #3 口径）；分支开发期以连续 2 次作为快速门禁，不替代收口 10 次；
3. 全量 `cargo test --workspace` **0 failed**（2 ignored 保持：真模型 smoke、手动 AC-08b）、`pytest -q` 84 passed、`doc/test-baseline.txt` 失败名额 0；
4. spec 回写：05（§5.2.1/§5.6.2/§5.8.5）、06（§6.4.1/§6.4.5/§6.7/§6.8/§6.10）、07（AC-06/AC-12）、08（§8.7 注记）+ `architecture-v2.md` §5.2 + `PROVENANCE.md` 登记（REV-056）；
5. `doc/progress.md`（≤30 行）、`doc/issues.md`、`README.md`、`doc/index.md`、`doc/reviews/README.md` 收口一致（REV-057）；
6. 所有分支合并后 worktree 与分支已清理。

## 5. 环境与风险

| 项 | 现状 / 应对 |
|---|---|
| 依赖管理 | 本机 **uv 未安装**：Python 测试用 `.venv/bin/python -m pytest`；`merge.sh` 传 `PYTHON_BIN=<repo>/.venv/bin/python` |
| 磁盘 | 剩余约 6.6 GiB：所有 worktree 与 `merge.sh` 共用 `CARGO_TARGET_DIR=<repo>/target`，构建串行等锁（不删 target） |
| Cargo 缓存 | **必须** `CARGO_HOME=<repo>/.cargo-home` + `CARGO_NET_OFFLINE=true`：默认 `~/.cargo` 缺 181 个依赖缓存，会卡在 crates.io 下载并阻塞全局构建锁；`.cargo-home` 已含 273 个 `.crate`（仓库既有缓存，勿删） |
| Python | 本机无 `uv`：`pytest` 用 `<repo>/.venv/bin/python -m pytest -q`；`merge.sh` 传 `PYTHON_BIN=<repo>/.venv/bin/python` |
| 并发 | 同一模块同一时间只有一个写入者；不同模块 worktree 互不重叠；Lead 统一合并 |
| e2e | 真子进程 + SQLite，耗时长；新增用例必须连续 2 次通过，防 flaky |
| 推送 | `origin` SSH 连通已确认；每个合并用 `--push` 保持 `origin/main` 与本地一致（merge.sh 以 origin/main 为基） |

## 6. 附：M5 顺延来源

- DR-002 §7（明确排除 M6）：ISSUE-033/036/059/073；
- DR-002 §4/§9（顺延/部分修）：ISSUE-075/077/079/080/081/085、084→088；
- ISSUE-082 部分修（台账 `issues.md`；DR-002 §1 表 #3/§3.2；REV-028/030 已接线 `recorded_outputs`/`llm_calls`，版本列仍 NULL）；
- DR-002 §9 #11：REV-028 #6 CLI pre-existing 偏差（clap/`-v`/`--version`/`NO_COLOR`）随 M6，由 REV-043 承接；
- M5 评审新登记（REV-028/030/032）：ISSUE-086/087/088/089；
- REV-036 收尾注记：REV-037（process 依赖治理 + CI clippy）顺延 M6，M6 以 REV-045 承接，REV-037 编号不启用。
