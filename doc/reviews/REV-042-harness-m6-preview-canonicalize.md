# REV-042 · harness · ISSUE-075 preview 脱敏 + ISSUE-077 canonicalize

- **分支**：`fix/harness-m6-preview-canonicalize`
- **作者**：`harness-dev`（提交 `c2958f6`）
- **评审人**：`lead`（**≠ 作者**；独立评审）
- **变更范围**：`origin/main(a031269)..c2958f6`，3 文件（`crates/zgraph-harness/src/{tools.rs,config.rs,constants.rs}`，+293/−10）；worktree clean
- **评审时间**：2026-10-31
- **关联**：ISSUE-075 / ISSUE-077；`doc/spec-v1.2/05` §5.2.1/§5.6.2/§5.8.5/§5.13.3；DR-003 §1/§3

## 1. 结论

**结论：通过**

## 2. 四维评审

| 维度 | 结论 | 依据（文件/行/测试） |
|---|---|---|
| 功能正确性 | ✅ | ISSUE-075：`args_summary/result_summary.preview` 改为结构摘要——标量只给类型/尺寸（`string(N chars)`/`number`/`boolean`/`null`），对象保留键、数组最多展开 4 项、深度 >2 降级 `object(N keys)`/`array(N items)`；敏感键（`api_key`/`token`/`password`/`secret`/`authorization`/`cookie`/`credential`/`session`/`privatekey` 等，键名归一化后匹配）值替换 `***`，任意深度生效；最终按字符边界截断 ≤256（含 `…`），ISSUE-074 上界保持。ISSUE-077：词法 `..` 越界仍 `path_escape`；随后 `canonicalize` 解析全部符号链接，用真实路径重新做资产根/CWD 前缀校验，symlink 逃逸（文件与中间目录）→ `path_escape`；`canonicalize` 失败退回词法路径，`file_missing`/`file_unreadable`/`file_too_large` 契约不变。新增 1 个函数（`canonical_or_lexical`）与 8 个测试（4 preview + 4 symlink/缺失）。 |
| 代码结构合理性 | ✅ | 脱敏/结构摘要集中在 `tools.rs`（`is_sensitive_key`/`structural_preview`/`preview_value`），常量进 `constants.rs`；`config.rs` 仅在 `resolve_asset_file` 内加 canonicalize 段，未改函数签名与错误码映射。 |
| 逻辑性 | ✅ | 词法判定先行（快速拒绝且保持既有 reason），canonicalize 仅在有真实路径时补充校验；敏感键匹配对键名做「小写 + 去非字母数字」，覆盖 `accessToken`/`client_secret`/`Authorization` 等变体；截断仍走 `truncate_chars` 字符边界。 |
| 可读性 | ✅ | 每个新函数带「为什么」注释（06/05 章节、ISSUE 号）；测试命名表达语义（`args_summary_redacts_sensitive_keys_and_values`、`symlinked_directory_escaping_assets_root_is_rejected`）。 |

## 3. 发现清单

| # | 级别 | 位置 | 问题 | 建议 | 状态 |
|---|---|---|---|---|---|
| 1 | 建议（后置） | `tools.rs summarize_result` 失败路径 | `{ok:false,error:{message,traceback}}` 内文本仍可能含敏感信息（本轮只按 ISSUE-075 收紧 preview） | 登记台账（M6 wrapup，建议 ISSUE-090）→ 后续评估 error 文本脱敏 | open（转台账） |
| 2 | 建议（后置） | `config.rs resolve_asset_file` | `agents/*.json` 索引文件本身为 symlink 指向根外时未拦（ISSUE-077 仅要求 prompt/constraints） | 如认为需要，另立任务；当前不阻塞 | 无需处理 |
| 3 | 备注 | `05 §5.8.5` | spec 正文仍写「preview=紧凑 JSON 前缀、M4 不脱敏」，需 docs 回写 | REV-056 回写 05 §5.8.5 / §5.13.3 | open（转 REV-056） |

无阻塞 / 重要项。

## 4. 验证证据

```bash
# 评审人在干净 target（/tmp/zgraph-merge-target，先 cargo clean 全部 workspace 成员）上执行
CARGO_HOME=<repo>/.cargo-home CARGO_NET_OFFLINE=true
cargo clean -p zgraph-dsl -p zgraph-kernel -p zgraph-trace -p zgraph-worker -p zgraph-harness -p zgraph-cli -p zgraph-e2e
cargo test -p zgraph-harness --quiet
```

- 结果：`zgraph-harness` **137 passed / 0 failed / 1 ignored**（ignored = 既有真模型 smoke），含新增敏感参数不泄露与 symlink 反例/正例。
- 作者另报告：`cargo fmt --all --check` OK、`check.sh` PASS。

## 5. 遗留问题

- 转台账：`summarize_result` 失败路径错误文本脱敏（建议 ISSUE-090）；spec 05 回写（REV-056）。
- 无阻塞项。
