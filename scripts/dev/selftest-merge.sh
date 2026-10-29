#!/usr/bin/env bash
# merge.sh 自测：在临时 clone 中验证「评审门禁 + dry-run 非破坏 + cargo/pytest 双门禁 + 合并清理」。
# 用法：bash scripts/dev/selftest-merge.sh
#       PYTHON_BIN=/path/to/python（需 pytest；缺省 python3）
#       CARGO_HOME/代理按本机环境设置（cargo 测试会真实执行）
#
# 背景（ISSUE-023/REV-004 #3）：merge.sh 自身的修复不能在原仓库里直接合并验证（合并时运行的是 main 上的旧脚本）。
# 本脚本把**当前分支已提交的树**安装到临时 clone 的 main，再建分支（upstream 指向落后的 origin/main）复现真实条件，
# 并断言 cargo test 确实被执行（而非 legacy 期跳过）。
set -euo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
PY="${PYTHON_BIN:-python3}"
"${PY}" -c "import pytest" >/dev/null 2>&1 || {
  echo "SELFTEST ERROR: ${PY} 无 pytest；请设置 PYTHON_BIN" >&2
  exit 2
}
command -v cargo >/dev/null 2>&1 || {
  echo "SELFTEST ERROR: 未找到 cargo（M0 起门禁含 cargo test）" >&2
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

# 1) 把当前分支已提交的树安装到 clone 的 main（模拟修复已进入 main）
git -C "${SRC}" archive HEAD | tar -x -C "${REPO}"
git add -A
git commit -q -m "test(process): install current tree under test"

# 2) 从本地 main 建分支（ff 可合并），upstream 指到落后的 origin/main（复现 ISSUE-023 条件）
git checkout -q -b test/process-merge-selftest main
git branch --set-upstream-to=origin/main test/process-merge-selftest >/dev/null 2>&1 || true
echo "<!-- selftest -->" >> doc/index.md
mkdir -p doc/reviews
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
if git worktree list | grep -q "test/process-merge-selftest"; then
  echo "SELFTEST FAIL: dry-run 创建/残留了 worktree"; exit 1
fi

# 4) 真实合并（不 push）：双门禁必须执行且通过，分支与 worktree 都被清理
PYTHON_BIN="${PY}" bash scripts/dev/merge.sh test/process-merge-selftest >/tmp/selftest-merge.log 2>&1 || {
  echo "SELFTEST FAIL: 真实合并退出非 0"; tail -20 /tmp/selftest-merge.log; exit 1
}
grep -q "cargo test 通过" /tmp/selftest-merge.log || {
  echo "SELFTEST FAIL: 未执行 cargo test（双栈门禁未生效）"; tail -20 /tmp/selftest-merge.log; exit 1
}
if git branch --list test/process-merge-selftest | grep -q .; then
  echo "SELFTEST FAIL: 分支未删除"; exit 1
fi
if git worktree list | grep -q "test/process-merge-selftest"; then
  echo "SELFTEST FAIL: worktree 未清理"; exit 1
fi
git log --oneline -1 | grep -q "selftest" || { echo "SELFTEST FAIL: main 未包含合并提交"; exit 1; }

echo "SELFTEST PASSED"
