"""外部 harness 适配器（v0.3/V4；DR-011 §3）。

本包只实现 `zgraph-harness/1` NDJSON over stdio 协议，由 `AgentConfig.harness`
以外部进程方式拉起：

- [`codex`](codex.py)：包装用户自备的 `codex exec --json`（Codex 先做）；
- [`dsh_acp`](dsh_acp.py)：DSH ACP 骨架（best-effort，v0.3 只做协议握手 + 结构化错误）。

硬约束：适配器**不得**写 trace/状态机；只通过 stdout 协议帧返回结果。
"""
