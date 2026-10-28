#!/usr/bin/env bash
# merge.sh 自测：在临时 clone 中验证「评审门禁 + dry-run 非破坏 + 真实合并 + 分支/worktree 清理」。
# 用法：bash scripts/dev/selftest-merge.sh
#       PYTHON_BIN=/path/to/python（需 pytest；缺省 python3）
#
# 背景（ISSUE-023）：merge.sh 自身的修复不能在原仓库里直接合并验证（合并时运行的是 main 上的旧脚本），
# 因此本脚本用"临时 clone + 在 main 上先提交新脚本 + 从 origin/main 建分支（确保 upstream 存在）"的方式复现真实条件。
set -euo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PY="${PYTHON_BIN:-python3}"
"${PY}" -c "import pytest" >/dev/null 2>&1 || {
  echo "SELFTEST ERROR: ${PY} 无 pytest；请设置 PYTHON_BIN" >&2
  exit 2
}

TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT
REPO="${TMP}/repo"

MAIN_WT="$(git -C "${SRC}" worktree list --porcelain | awk '$1=="worktree"{print $2; exit}')"
echo "== clone ${MAIN_WT} to ${REPO} =="
git clone -q "${MAIN_WT}" "${REPO}"
cd "${REPO}"
git config user.name "chenyuxiangg"
git config user.email "1007423165@qq.com"
git checkout -q main

# 1) 在 clone 的 main 上安装"待测脚本"（模拟修复已进入 main）
cp "${SRC}/scripts/dev/merge.sh" scripts/dev/merge.sh
git add scripts/dev/merge.sh
git commit -q -m "test(process): install merge.sh under test"

# 2) 从本地 main 建分支（保证 ff 可合并），并把 upstream 指到落后的 origin/main，复现 ISSUE-023 的条件
git checkout -q -b test/process-merge-selftest main
git branch --set-upstream-to=origin/main test/process-merge-selftest >/dev/null 2>&1 || true
echo "<!-- selftest -->" >> doc/index.md
cat > doc/reviews/REV-998-process-selftest.md <<'EOF'
# REV-998 · process · merge.sh 自测

- **分支**：`test/process-merge-selftest`
- **作者**：`selftest`
- **评审人**：`selftest-reviewer`

## 1. 结论

**结论：通过**
EOF
git add -A
git commit -q -m "docs(process): selftest" -m "Review: REV-998" -m "Agent: selftest"

# 3) dry-run：不得清理分支/worktree
git checkout -q main
PYTHON_BIN="${PY}" bash scripts/dev/merge.sh test/process-merge-selftest --dry-run >/tmp/selftest-dry.log 2>&1 || {
  echo "SELFTEST FAIL: dry-run 退出非 0"; tail -20 /tmp/selftest-dry.log; exit 1
}
git branch --list test/process-merge-selftest | grep -q . || { echo "SELFTEST FAIL: dry-run 删除了分支"; exit 1; }
git worktree list | grep -q "test/process-merge-selftest" && { echo "SELFTEST FAIL: dry-run 创建/残留了 worktree"; exit 1; }

# 4) 真实合并（不 push）：必须成功、分支与 worktree 都被清理
PYTHON_BIN="${PY}" bash scripts/dev/merge.sh test/process-merge-selftest >/tmp/selftest-merge.log 2>&1 || {
  echo "SELFTEST FAIL: 真实合并退出非 0"; tail -20 /tmp/selftest-merge.log; exit 1
}
git branch --list test/process-merge-selftest | grep -q . && { echo "SELFTEST FAIL: 分支未删除"; exit 1; }
git worktree list | grep -q "test/process-merge-selftest" && { echo "SELFTEST FAIL: worktree 未清理"; exit 1; }
git log --oneline -1 | grep -q "selftest" || { echo "SELFTEST FAIL: main 未包含合并提交"; exit 1; }

echo "SELFTEST PASSED"
