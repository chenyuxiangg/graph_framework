#!/usr/bin/env bash
# v0.4/D4 真沙箱验证脚本（macOS）。
#
# 必须在**未被 Seatbelt 沙箱化的普通终端**运行（Terminal.app/iTerm 直接启动，
# 不要从 DSH/Xcode/IDE/CI wrapper 内运行）：嵌套 Seatbelt 下 `sandbox-exec`
# 会硬失败（exit 71），产品行为是 fail-closed `sandbox_nested`，无法验证隔离。
#
# 用法:
#   bash crates/zgraph-harness/scripts/verify-sandbox.sh
#
# 该脚本运行 `real_sandbox_preflight_and_isolation_environment_aware`：
#   - 嵌套环境：断言分类为 sandbox_nested/sandbox_unavailable（fail-closed，显式记录）；
#   - 普通终端：真实断言 workspace 写成功、/etc/passwd 读拒、/tmp 写拒、网络 deny。
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../.." && pwd)"
cd "$REPO_ROOT"

export CARGO_HOME="${CARGO_HOME:-$REPO_ROOT/.cargo-home}"

echo "== zgraph v0.4/D4 sandbox verification =="
echo "repo: $REPO_ROOT"
if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "!! 非 macOS：v0.4 仅实现 macOS Seatbelt 沙箱（Linux/Windows fail-closed）" >&2
  exit 2
fi
echo "sandbox-exec: $(/usr/bin/sandbox-exec 2>&1 | head -1 || true)"
echo "-- 运行真沙箱测试（--nocapture 显示环境分类/隔离断言）--"
cargo test -p zgraph-harness --lib \
  builtin::sandbox::tests::real_sandbox_preflight_and_isolation_environment_aware \
  -- --nocapture --test-threads=1
echo "PASS: 真沙箱验证完成（若上方输出 sandbox_nested，表示当前终端仍是嵌套沙箱；"
echo "      请在普通终端重试以验证隔离断言）"
