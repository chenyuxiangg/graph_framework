# 遗留问题台账

> 规则（见 `AGENTS.md` §7）：评审无法当场修复的问题、已知缺陷、环境阻塞、设计分歧必须登记；不允许用代码注释/TODO 代替。
> 级别：`P0` 阻塞开发 / `P1` 高 / `P2` 中 / `P3` 低。状态：`open` / `in-progress` / `fixed` / `wontfix`。

| ID | 级别 | 模块 | 问题 | 影响 | 建议 | 状态 | 登记日期 |
|---|---|---|---|---|---|---|---|
| ISSUE-001 | P0 | legacy | `scripts/zlog/` 资产缺失（`monitor/logger.py` 通过 `sys.path` 导入本地 `zlog`；仓库从未包含该目录） | 仅影响 legacy：3 个 scheduler 单测 + 14+ e2e 失败 | **不再修复**：DR-001 后 legacy 冻结只读，新架构（worker SDK）使用标准 `logging` | wontfix | 2026-10-28 |
| ISSUE-002 | P0 | legacy | `config/software_team_graph.json` 边引用 `planer_gate` 等，而 `gates` 定义是 `check_pm_gate` 等 | 仅影响 legacy：`E_BAD_GATE_REF`，software_team 图不可用 | **不直接修复 legacy**：M1 把三张图改写为 DSL v1.2 fixture 时统一命名 | wontfix | 2026-10-28 |
| ISSUE-003 | P1 | process | 仓库原先无依赖清单；本机未安装 `uv`（CLAUDE.md 要求 uv 管理依赖） | 新人无法按文档一键起环境；CI 无依据 | 已补 `pyproject.toml`（dev 依赖 pytest）；需安装 uv 并提交 `uv.lock` | in-progress | 2026-10-28 |
| ISSUE-004 | P1 | e2e | 测试基线非全绿：全量 187 个 test function，当前 **25 项失败**（4 单测 + 21 e2e，见 `doc/test-baseline.txt`） | 无法用"全绿"作为合并门槛；当前 merge 门槛为"不得新增失败" | 先修 ISSUE-001/002，再逐步清空基线；禁止改测试绕过 | open | 2026-10-28 |
| ISSUE-005 | P2 | process | 无 CI、无分支保护、无自动化校验 | 评审/测试约束只能靠脚本自觉 | 增加 CI（pytest + check.sh）；main 分支保护 | open | 2026-10-28 |
| ISSUE-006 | P2 | docs | `README.md` 原为空；`doc/index.md` 仅 1 条；`doc/graph_base_design.md` 与代码实现的一致性未验证 | 上手成本高，设计/实现易漂移 | 本次已补 README 与文档索引；后续每次模块变更同步设计文档 | in-progress | 2026-10-28 |
| ISSUE-007 | P2 | process | 两套设计材料并存（Python 实现 vs Rust/PyO3 详细设计） | 技术路线不清会重复建设 | **已决策**：DR-001 = Rust 内核 + Python 子进程 worker，v1.1 语义为核心；legacy 冻结 | fixed | 2026-10-28 |
| ISSUE-008 | P2 | monitor | `monitor/logger.py` 用 `sys.path.insert` 加载本地包，属隐式依赖、无法打包 | 部署/测试环境依赖路径约定，易碎 | 统一为显式包（`src/…` 或 pyproject 依赖），消除 sys.path 注入 | open | 2026-10-28 |
| ISSUE-009 | P3 | e2e | 无覆盖率配置/门槛；187 个 test function 的实际覆盖未知 | 质量不可度量 | 引入 coverage（pytest-cov），定 P2 阶段门槛（如核心 kernel ≥80%） | open | 2026-10-28 |
| ISSUE-010 | P3 | cli | `scripts/graph_base_cli.py` 未作为 pyproject 入口点，只能 `python scripts/...` 调用 | 使用不便、难以在 PATH 中分发 | 增加 `[project.scripts] graph-base = ...`（需把 CLI 移入包或提供 shim） | open | 2026-10-28 |
| ISSUE-011 | P3 | config/workers | `config/index.md` 自述：除 planer/archer 外各节点 `proc` 为空（走 `proc_bypass`），software_team 产物需用户自建 worker，尚未内置 | software_team 图只能跑框架、不能产出真实业务结果 | 明确 worker 交付范围；按模块补充 worker 与单测 | open | 2026-10-28 |
| ISSUE-012 | P1 | process | `merge.sh --dry-run` 会删除作者的 worktree（自验证发现） | 误执行 dry-run 会丢失未合并工作区（可恢复但危险） | 已修复：dry-run 只检查不清理；REV-002 | fixed | 2026-10-28 |
| ISSUE-013 | P1 | process | legacy Python 实现与新 Rust 代码尚未物理隔离 | 目录/CI 混淆，误改风险 | **已修**：M0 迁入 `legacy/`（src/config/tests/cli/设计文档），`legacy/README.md` 标只读，pytest testpaths 仅 `python/` | fixed | 2026-10-29 |
| ISSUE-014 | P1 | kernel | 自定义 Python 门控方案未定（Rust 内置 op vs 常驻 gate worker） | 影响 DSL 门控能力与性能 | **v1 默认内置 op**（check_output/compare/manual）；Python 门控 M1 前定稿（DR-001 O-1） | open | 2026-10-29 |
| ISSUE-015 | P1 | worker | worker 协议 v1 仅有草案，无兼容测试 | 协议演进导致内核/worker 不兼容 | M0 冻结协议文档 + 双方版本校验 + 兼容用例 | open | 2026-10-29 |
| ISSUE-016 | P1 | trace | 可复现 L2 的记录粒度未定 | 影响结果重算能力与存储量 | **已定**：`--record` 默认 `hash`；L2 需运行期 `--record=full`（architecture §5.2/§5.3） | fixed | 2026-10-29 |
| ISSUE-017 | P2 | worker | worker 每节点冷启动约 50–100ms，未实测、未池化 | 高频小节点吞吐受限 | M3 基准后决定是否池化（O-2） | open | 2026-10-29 |
| ISSUE-018 | P2 | process | 双语言工具链（cargo/rustfmt/clippy + uv/pytest）尚无 CI | 质量门禁不可自动化 | **已修**：`.github/workflows/ci.yml`（rust fmt/clippy/test + uv/pytest）；AGENTS/worktree 模块表同步 | fixed | 2026-10-29 |
| ISSUE-019 | P1 | process | `CLAUDE.md` 冻结且写明 Python 技术栈，与 Rust 宿主冲突（文件禁止修改） | 规范适用边界不清，Rust 代码可能被误按 Python 规范检查 | **已确认**（用户 2026-10-29「直接开工」授权）：解释为 Python 代码规范（`python/**`、`legacy/**`）；Rust 用 rustfmt/clippy + 等价约束；CI 双栈检查 | fixed | 2026-10-29 |
| ISSUE-020 | P1 | docs | spec v1.2 与 v2 的 D1–D12 偏差、以及 5 个新错误码（`NODE_WORKER_HANDSHAKE/PROTOCOL/IMPORT/CRASH`、`REPLAY_RECORD_MISSING`）尚未回写 spec 正文（当前由 architecture §8/§8.2 覆盖） | 后续实现者可能误读 spec 原文 | M0–M2 逐条回写 spec 00/03/04/05/06，并更新 PROVENANCE hash | open | 2026-10-29 |
| ISSUE-021 | P2 | process | legacy 测试/`doc/test-baseline.txt` 与新的 Rust 双栈门禁如何退役未定义 | 合并门禁语义混乱 | **已修**：pytest 只跑 `python/`；基线清零；merge.sh 增加 cargo test（有 Cargo.toml 时必跑）；legacy 测试随目录归档 | fixed | 2026-10-29 |
| ISSUE-022 | P2 | worker | worker 幂等/外部副作用声明尚未写入节点契约与验收 | 影响 L3 诚实性 | M3 前写入 worker SDK 契约与 AC | open | 2026-10-29 |
| ISSUE-023 | P1 | process | `merge.sh` 用 `git branch -d` 删除分支，在 push 前会因 upstream 未更新而误报“未完全合并”，导致合并门禁在最后一步失败 | 每次合并需手工收尾，门禁不可靠 | **修复待合并**：ff 合并后校验 HEAD 祖先并用 `-D`；顺序改为 worktree→push→删分支；新增 `scripts/dev/selftest-merge.sh` 自测。**自举两阶段**：本分支修改了 merge.sh，合并它时 main 上仍是旧脚本 → 首次会停在“删分支”步骤（main 已合并、分支/worktree 已清理），重跑新脚本即完成（幂等）。REV-003 复审 | in-progress | 2026-10-29 |
| ISSUE-024 | P3 | docs | REV-001 #22：5 处文字问题 | 不影响语义，影响可读性 | **已修**：00 行数 264、执行模型行加 v2 限定、PROVENANCE banner 范围、ISSUE-020 计数 5、architecture §8.1/§8.2 顺序 | fixed | 2026-10-29 |
| ISSUE-025 | P1 | worker | M0 协议检查单未覆盖（`schemas/worker-protocol-v1.md` §6/§7）：supervisor 侧 cancel→TERM→KILL 进程组、result 后不退出分支、帧空闲超时、流式读取上限、进程组 kill（孙进程）、全局/每 run 并发上限 | 部分协议分支/资源回收在 M3 才闭环 | M0 已覆盖（附测试）：hello 超时/版本（`hello_timeout_kills_and_reaps_process`）、hello_ack 前不执行（`test_no_execution_before_hello_ack`）、重复 result（`duplicate_result_is_rejected`）、exit-0-无-result（`exit_without_result_is_rejected`）、半行帧（`decode_frame_line` 单测）/非法 UTF-8（`invalid_utf8_frame_is_rejected`）/8MiB（帧解码）、stderr 排空+内容（`noisy_worker_stderr_is_captured`）、print/input 隔离、Python 协作取消、run/hello 超时 kill+wait/reap（PID 断言） | open | 2026-10-29 |
| ISSUE-026 | P1 | dsl | D7 下 `DSL_SIGNATURE_MISMATCH` / `DSL_SYMBOL_CONFLICT` 的触发语义是 M1 解读（内置 gate op 的 params 契约不符 / gate op canonical `builtin.<op>` 与 python_node 符号跨类同名）；spec 02 §2.5 仍按「import + Python 签名」描述 | 门控符号错误码语义未冻结；`bad_signature`/`bad_symbol_conflict` fixtures 依赖该解读，若变更需同步实现与 fixtures | 回写 spec 02 §2.5 的 D7 口径（并入 ISSUE-020 范围）；评审确认后回填 | open | 2026-10-29 |
| ISSUE-027 | P2 | dsl | R14c 在未提供 `--assets-dir` / `ZGRAPH_ASSETS` 时**跳过** agent 资产校验，与 02 §2.6.3「资产根缺省 `.`」有偏差（否则 `zgraph validate <graph>` 单文件不可用） | agent 资产缺失可能延迟到运行期（M4 Harness Loading）才暴露；`validate` 结果受 CLI 参数影响 | spec 回写 02 §2.6.3（显式声明「未提供则跳过」），并在 06 CLI 章节固定该行为 | open | 2026-10-29 |
| ISSUE-028 | P2 | e2e | M1 fixtures 路径/命名与 07 AC-04 不一致：07 为 `tests/fixtures/bad_graphs/{parse_error.json,…}` 与 `examples/customer_ticket/graph.json`；M1 落在 `tests/fixtures/graphs/bad_*.json`（task-9 指定） | M2/e2e 直接按 07 路径引用会找不到文件；AC-04 无法原样执行 | M2/e2e 建立映射或统一改名为 07 口径；改名时同步本仓 dsl 测试与 fixtures 断言 | open | 2026-10-29 |
| ISSUE-029 | P2 | dsl | 未实现项：`schemas/graph.schema.json`、`zgraph validate --structure-only` / `--print-hash`、`inputs` 类型检查词表之外的语义校验、`input_mapping` 语义级校验（M1 仅结构/键值校验） | JSON Schema 资产缺失；CLI 选项与 02 §2.7.4 未全量对齐；类型/映射语义未冻结 | 单列后续 M1.x 任务（dsl + process 边界）；在 08/02 中标注未覆盖 | open | 2026-10-29 |
| ISSUE-030 | P3 | process | `blake3` 依赖与 `Cargo.lock` 变更属 process 边界，本次随 dsl（02 §2.8 BLAKE3 要求）引入 | 依赖治理边界模糊，后续模块可能重复引入同一依赖 | M1 评审确认（REV-005）；如需收敛，由 process 模块统一提升到 `workspace.dependencies` | open | 2026-10-29 |
| ISSUE-031 | P3 | dsl | 子图 `id` 缺失/空/非 string 时没有可用的 `SubgraphId`，按 02 §2.7.3 只能给文档级 `location: {}`（路径 `subgraphs[i]` 写入 message），与「除 R1 与 R2/R3 外每条错误 MUST 含 node_id 或 edge_index」存在措辞张力 | 复审/后续 spec 回写时若要求子图头错误也带定位，需扩展契约（如允许 `subgraph_index`）或明确例外 | 建议 02 §2.7.3 增补例外：无 SubgraphId 的子图定义错误为文档级、路径入 message；本次 REV-005 修复按此实现 | open | 2026-10-29 |
