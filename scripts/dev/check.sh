#!/usr/bin/env bash
# 提交前自检：身份、冻结文件、禁用文件、Python 语法、进度文件
# 用法：bash scripts/dev/check.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT}"

fail=0
ok() { echo "OK   $*"; }
bad() { echo "FAIL $*" >&2; fail=1; }

# 1) 身份（要求 4）
NAME="$(git config user.name || true)"
EMAIL="$(git config user.email || true)"
[ "${NAME}" = "chenyuxiangg" ] && ok "user.name=${NAME}" || bad "user.name 必须为 chenyuxiangg（当前: ${NAME}）"
[ "${EMAIL}" = "1007423165@qq.com" ] && ok "user.email=${EMAIL}" || bad "user.email 必须为 1007423165@qq.com（当前: ${EMAIL}）"

# 2) 远程必须 SSH（要求 4）
URL="$(git remote get-url origin 2>/dev/null || true)"
case "${URL}" in
  git@github.com:chenyuxiangg/graph_framework.git) ok "remote=${URL}" ;;
  *) bad "origin 必须为 git@github.com:chenyuxiangg/graph_framework.git（当前: ${URL:-<空>}）" ;;
esac

# 3) 冻结文件 CLAUDE.md 不得被修改（要求：CLAUDE.md 禁止修改）
if git rev-parse --verify HEAD >/dev/null 2>&1; then
  changed_claude="$(git diff --name-only HEAD -- CLAUDE.md || true)"
  [ -z "${changed_claude}" ] && ok "CLAUDE.md 未被修改" || bad "禁止修改 CLAUDE.md"
fi

# 4) 禁用文件不得进入暂存区
staged="$(git diff --cached --name-only || true)"
badfiles="$(echo "${staged}" | grep -E '(^|/)(\.venv|__pycache__|\.worktrees|logs|\.idea|\.vscode)/|\.pyc$|\.db$|\.sqlite3?$|\.env$|\.pem$|\.key$' || true)"
[ -z "${badfiles}" ] && ok "暂存区无禁用文件" || bad "暂存区含禁用文件: ${badfiles}"

# 5) 分支命名（要求在非 main 分支开发）
BRANCH="$(git rev-parse --abbrev-ref HEAD)"
case "${BRANCH}" in
  main) ok "当前在 main（bootstrap/流程提交允许；代码改动禁止直接提交 main）" ;;
  feat/*|fix/*|test/*|docs/*|chore/*|refactor/*) ok "分支命名: ${BRANCH}" ;;
  *) bad "分支名不符合 <type>/<module>-<slug>: ${BRANCH}" ;;
esac

# 6) Python 语法（不写 .pyc）
if command -v python3 >/dev/null 2>&1; then
  if python3 - "${ROOT}" <<'PY'
import ast, sys
from pathlib import Path
root = Path(sys.argv[1])
bad = []
for pattern in ("python/**/*.py", "tests/**/*.py"):
    for p in root.glob(pattern):
        try:
            ast.parse(p.read_text(encoding="utf-8"))
        except SyntaxError as exc:
            bad.append(f"{p.relative_to(root)}:{exc.lineno}: {exc.msg}")
if bad:
    print("\n".join(bad))
    sys.exit(1)
PY
  then ok "Python 语法检查通过"; else bad "Python 语法错误（见上）"; fi
else
  bad "未找到 python3"
fi

# 7) 进度与台账存在且进度文件足够简洁（要求 5/7）
[ -f doc/progress.md ] && ok "doc/progress.md 存在" || bad "缺少 doc/progress.md"
[ -f doc/issues.md ] && ok "doc/issues.md 存在" || bad "缺少 doc/issues.md"
if [ -f doc/progress.md ]; then
  n="$(grep -cve '^[[:space:]]*$' doc/progress.md || true)"
  [ "${n}" -le 40 ] && ok "doc/progress.md 非空行 ${n}（目标 ≤30）" || bad "doc/progress.md 过长（${n} 行），请精简"
fi

# 8) Rust 格式检查（M0 起双栈；需 rustfmt）
if [ -f Cargo.toml ] && command -v cargo >/dev/null 2>&1; then
  if cargo fmt --all -- --check >/tmp/zgraph-fmt.log 2>&1; then
    ok "cargo fmt --check 通过"
  else
    bad "cargo fmt 未通过（见 /tmp/zgraph-fmt.log；运行 cargo fmt --all 修复）"
  fi
else
  ok "未检测到 Rust 工程或 cargo，跳过 fmt"
fi

# 9) 空白/冲突标记检查
if git diff --check >/dev/null 2>&1; then ok "git diff --check 通过"; else bad "存在空白错误或冲突标记"; fi

echo "---"
[ "${fail}" = 0 ] && { echo "CHECK PASSED"; exit 0; } || { echo "CHECK FAILED" >&2; exit 1; }
