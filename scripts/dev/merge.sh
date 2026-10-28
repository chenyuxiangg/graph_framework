#!/usr/bin/env bash
# 合并门禁：评审记录 -> 工作区干净 -> 同步 main -> 测试无新增失败 -> ff-only 合并 -> 清理 worktree/分支
# 用法：bash scripts/dev/merge.sh <branch> [--push] [--dry-run]
#       PYTHON_BIN=/path/to/python 可指定带 pytest 的解释器（无 uv 环境用）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT}"

die() { echo "MERGE BLOCKED: $*" >&2; exit 1; }
info() { echo "INFO: $*"; }

BRANCH=""
PUSH=0
DRY=0
for arg in "$@"; do
  case "${arg}" in
    --push) PUSH=1 ;;
    --dry-run) DRY=1 ;;
    -*) die "未知参数: ${arg}" ;;
    *) [ -z "${BRANCH}" ] || die "只能指定一个分支"; BRANCH="${arg}" ;;
  esac
done
[ -n "${BRANCH}" ] || die "用法: merge.sh <branch> [--push] [--dry-run]"

# 1) 必须在 main 工作区执行，且 main 干净
CUR="$(git rev-parse --abbrev-ref HEAD)"
[ "${CUR}" = "main" ] || die "请在 main 工作区执行（当前: ${CUR}）"
git diff --quiet && git diff --cached --quiet || die "main 工作区有未提交改动，先提交或 stash"

# 2) 分支存在且不是 main
git rev-parse --verify "refs/heads/${BRANCH}" >/dev/null 2>&1 || die "分支不存在: ${BRANCH}"
[ "${BRANCH}" != "main" ] || die "不能合并 main 到自己"

# 3) 定位分支 worktree；没有则建临时 worktree（评审记录与测试都在分支上下文执行）
branch_wt="$(git worktree list --porcelain | awk -v b="refs/heads/${BRANCH}" '
  $1=="worktree"{wt=$2} $1=="branch"{if($2==b){print wt; exit}}')"
tmp_wt=""
if [ -z "${branch_wt}" ]; then
  tmp_wt=".worktrees/.merge-check-$$"
  info "分支无 worktree，创建临时检查目录: ${tmp_wt}"
  git worktree add --detach "${tmp_wt}" "${BRANCH}" >/dev/null
  branch_wt="${tmp_wt}"
fi
cleanup() { [ -n "${tmp_wt}" ] && git worktree remove --force "${tmp_wt}" >/dev/null 2>&1 || true; }
trap cleanup EXIT

# 4) 评审记录：分支匹配、结论通过、无模板占位（允许在 main 或分支内）
review=""
for base in "${ROOT}" "${branch_wt}"; do
  while IFS= read -r f; do
    [ -n "${f}" ] || continue
    if grep -qE "^[[:space:]]*[-*]?[[:space:]]*\*{0,2}分支\*{0,2}[[:space:]]*[：:][[:space:]]*\`?${BRANCH}\`?[[:space:]]*$" "${f}"; then
      review="${f}"; break
    fi
  done < <(find "${base}/doc/reviews" -maxdepth 1 -name 'REV-*.md' -type f 2>/dev/null | sort)
  [ -n "${review}" ] && break
done
[ -n "${review}" ] || die "未找到包含 “分支: ${BRANCH}” 的评审记录（doc/reviews/REV-*.md，main 或分支内）"
grep -q "REV-<NNN>" "${review}" && die "评审记录仍是模板占位: ${review}"
grep -q "<评审人>" "${review}" && die "评审记录缺少真实评审人: ${review}"
grep -qE "结论[：:][[:space:]]*\**通过" "${review}" || die "评审结论不是“通过”: ${review}"
info "评审记录: ${review} -> 通过"

[ -z "$(git -C "${branch_wt}" status --porcelain)" ] || die "分支工作区不干净: ${branch_wt}"

# 5) 同步 main（rebase 分支到最新 origin/main）
git fetch origin main >/dev/null 2>&1 || die "git fetch origin main 失败（检查 SSH 连通性）"
if ! git merge-base --is-ancestor origin/main "${BRANCH}"; then
  info "分支落后 origin/main，执行 rebase"
  git -C "${branch_wt}" rebase origin/main || die "rebase 冲突，请人工解决后重试"
fi

# 6) 测试：不得新增失败（对照 main 的 doc/test-baseline.txt）
if [ -n "${PYTHON_BIN:-}" ] && "${PYTHON_BIN}" -c "import pytest" >/dev/null 2>&1; then
  PY=("${PYTHON_BIN}")
elif command -v uv >/dev/null 2>&1 && [ -f pyproject.toml ]; then
  PY=(uv run python)
elif python3 -c "import pytest" >/dev/null 2>&1; then
  PY=(python3)
else
  die "环境无 pytest（uv 未安装且 python3 无 pytest）——先完成 ISSUE-003，或设置 PYTHON_BIN"
fi
info "执行测试: ${PY[*]} -m pytest -q"
set +e
( cd "${branch_wt}" && "${PY[@]}" -m pytest -q --tb=no >/tmp/zgraph-merge-test.log 2>&1 )
set -e
awk '/^(FAILED|ERROR)/{print $2}' /tmp/zgraph-merge-test.log | sort > /tmp/zgraph-current-failures.txt || true
grep -v '^#' doc/test-baseline.txt | sed '/^$/d' | sort > /tmp/zgraph-baseline.txt
new_fail="$(comm -23 /tmp/zgraph-current-failures.txt /tmp/zgraph-baseline.txt)"
fixed="$(comm -13 /tmp/zgraph-current-failures.txt /tmp/zgraph-baseline.txt)"
current_n="$(wc -l < /tmp/zgraph-current-failures.txt | tr -d ' ')"
base_n="$(wc -l < /tmp/zgraph-baseline.txt | tr -d ' ')"
[ -z "${new_fail}" ] || die "出现新增失败（${current_n} vs 基线 ${base_n}）:
${new_fail}"
info "测试通过门槛：失败 ${current_n} 项，基线 ${base_n} 项，无新增"
[ -z "${fixed}" ] || info "注意：有失败项已修复，请缩小 doc/test-baseline.txt: ${fixed}"

if [ "${DRY}" = 1 ]; then
  info "dry-run 结束：检查全部通过，未执行合并（不清理作者 worktree）"
  exit 0
fi

# 7) ff-only 合并
git merge --ff-only "${BRANCH}" || die "非 fast-forward：请确认分支已 rebase 到 origin/main"
info "已合并 ${BRANCH} -> main"

# 8) 清理 worktree
for wt in $(git worktree list --porcelain | awk -v b="refs/heads/${BRANCH}" '
  $1=="worktree"{w=$2} $1=="branch"{if($2==b){print w}}'); do
  git worktree remove --force "${wt}" >/dev/null || die "worktree 删除失败: ${wt}"
  info "已删除 worktree: ${wt}"
done

# 9) 可选推送（先推送：失败时保留分支便于重试）
if [ "${PUSH}" = 1 ]; then
  git push origin main || die "推送失败（main 已合并、分支保留；修复后重跑本脚本或手动 push）"
  info "已推送 origin/main"
else
  info "未推送（加 --push 推送 origin/main）"
fi

# 10) 删除分支
# 注意：ff 合并后分支必为 HEAD 祖先；不能用 `git branch -d`（它要求已合并到 upstream，
# 而轻量/本地分支的 upstream 仍是 origin/main —— push 前/未 push 时会误报“未完全合并”）。
git merge-base --is-ancestor "${BRANCH}" HEAD || die "分支不是 HEAD 祖先，拒绝删除: ${BRANCH}"
git branch -D "${BRANCH}" >/dev/null || die "分支删除失败: ${BRANCH}"
info "已删除分支: ${BRANCH}"

cat <<EOF

合并完成。请确认：
- [ ] doc/progress.md 已更新
- [ ] 新发现的问题已登记 doc/issues.md
- [ ] 基线若缩小，已更新 doc/test-baseline.txt
EOF
