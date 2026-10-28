#!/usr/bin/env bash
# 模块 worktree 管理：new / list / remove
# 用法：
#   bash scripts/dev/worktree.sh new <module> <agent> <slug> [--type feat|fix|test|docs|chore] [--base <ref>] [--dry-run]
#   bash scripts/dev/worktree.sh list
#   bash scripts/dev/worktree.sh remove <module>-<agent> [--force]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "${ROOT}"

MODULES="kernel scheduler state-manager dispatcher monitor orchestrator workers cli config e2e docs process"
TYPES="feat fix test docs chore"

die() { echo "ERROR: $*" >&2; exit 1; }
info() { echo "INFO: $*"; }

valid_module() {
  local m="$1"
  for x in ${MODULES}; do [ "${x}" = "${m}" ] || continue; return 0; done
  die "非法 module: ${m}（可选：${MODULES}）"
}

cmd_new() {
  local module="" agent="" slug="" type="feat" base="origin/main" dry=0
  [ $# -ge 3 ] || die "用法: worktree.sh new <module> <agent> <slug> [--type ...] [--base ...] [--dry-run]"
  module="$1"; agent="$2"; slug="$3"; shift 3
  while [ $# -gt 0 ]; do
    case "$1" in
      --type) [ $# -ge 2 ] || die "--type 需要参数"; type="$2"; shift 2 ;;
      --base) [ $# -ge 2 ] || die "--base 需要参数"; base="$2"; shift 2 ;;
      --dry-run) dry=1; shift ;;
      *) die "未知参数: $1" ;;
    esac
  done
  valid_module "${module}"
  [[ "${slug}" =~ ^[a-z0-9][a-z0-9-]*$ ]] || die "slug 只允许小写字母/数字/短横线: ${slug}"
  local ok=0
  for t in ${TYPES}; do [ "${t}" = "${type}" ] && ok=1; done
  [ "${ok}" = 1 ] || die "非法 type: ${type}（可选：${TYPES}）"

  local branch="${type}/${module}-${slug}"
  local wt=".worktrees/${module}-${agent}"

  git rev-parse --verify "${base}" >/dev/null 2>&1 || die "base 不存在: ${base}（先 git fetch origin）"
  git rev-parse --verify "refs/heads/${branch}" >/dev/null 2>&1 && die "分支已存在: ${branch}"
  [ -e "${wt}" ] && die "worktree 路径已存在: ${wt}"

  info "module=${module} agent=${agent}"
  info "branch=${branch}"
  info "path=${wt}"
  info "base=${base}"
  if [ "${dry}" = 1 ]; then info "dry-run：不创建"; return 0; fi

  mkdir -p "${ROOT}/.worktrees"
  git worktree add -b "${branch}" "${wt}" "${base}"
  cat <<EOF

下一步：
  cd ${wt}
  # 编码 + 单测
  bash scripts/dev/check.sh
  uv run pytest -q
  git add -A && git commit -m "${type}(${module}): <摘要>" -m "Review: REV-<NNN>" -m "Agent: ${agent}"
  # 找独立评审人写 doc/reviews/REV-<NNN>-${module}-<slug>.md
  bash scripts/dev/merge.sh ${branch} --dry-run
EOF
}

cmd_list() {
  git worktree list
  echo "---"
  git for-each-ref --format="%(refname:short)" refs/heads | grep -E '^(feat|fix|test|docs|chore)/' || true
}

cmd_remove() {
  [ $# -ge 1 ] || die "用法: worktree.sh remove <module>-<agent> [--force]"
  local name="$1"; shift
  local force=""
  [ "${1:-}" = "--force" ] && force="--force"
  local wt=".worktrees/${name}"
  [ -d "${wt}" ] || die "worktree 不存在: ${wt}"
  git worktree remove ${force} "${wt}"
  echo "已删除 worktree: ${wt}（分支保留，可由 merge.sh 删除）"
}

[ $# -ge 1 ] || die "用法: worktree.sh {new|list|remove} ..."
sub="$1"; shift
case "${sub}" in
  new) cmd_new "$@" ;;
  list) cmd_list ;;
  remove) cmd_remove "$@" ;;
  *) die "未知子命令: ${sub}" ;;
esac
