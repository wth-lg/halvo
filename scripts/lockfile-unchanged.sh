#!/bin/sh
set -u
name=lockfile-unchanged

usage() {
  cat >&2 <<'EOF'
usage: sh scripts/lockfile-unchanged.sh macos|windows
       sh scripts/lockfile-unchanged.sh --self-test

The body of the step Lockfile unchanged of the CI job named by the first
argument. It reads the checkout at GITHUB_WORKSPACE and fails when HEAD is
not GITHUB_SHA, when a tracked file changed, when a file stands that no
tracked .gitignore ignores, when an index flag is set on a tracked file,
when an ignore line of an untracked file or of git's own settings hides a
file, or when git reports an ignored file or directory whose name is that
of a store file of the capture store's layout, whichever rule ignores it.
On the macos job a change of a file's mode counts; on the windows job it
is not seen. A store file inside a directory that an ignore rule ignores
whole, such as target/, is not seen.

--self-test runs the cases of this script in throwaway repositories under
TMPDIR and prints PASS or FAIL.
EOF
  exit 64
}

is_store_name() {
  case $1 in
    store.db*|store.key|create.pending|writer.lock|daemon.lock|daemon-state.json|delete-all.intent|*.seg) return 0 ;;
  esac
  return 1
}

inspect() {
  tree=$1 want=$2 job=$3
  for variable in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do
    unset "$variable"
  done
  in_checkout() {
    if [ "$job" = macos ]; then
      set -- -c core.fileMode=true "$@"
    fi
    git --no-replace-objects --git-dir="$tree/.git" --work-tree="$tree" -c core.fsmonitor=false -c core.trustctime=true -c core.checkStat=default "$@"
  }
  head=$(in_checkout rev-parse HEAD)
  if [ "$head" != "$want" ]; then
    printf 'HEAD is not the commit of this run: a step or a check moved HEAD of the checkout.\n'
    return 1
  fi
  changed=$(in_checkout status --porcelain --untracked-files=no --ignore-submodules=none)
  untracked=$(in_checkout ls-files --others --exclude-per-directory=.gitignore)
  flagged=$(in_checkout ls-files -v | awk '!/^H /')
  hidden=$(in_checkout status --porcelain --ignored --untracked-files=normal | sed -n 's/^!! //p' | while IFS= read -r entry; do
    source=$(in_checkout check-ignore -v --no-index -- "$entry") || { printf '%s\n' "$entry"; continue; }
    in_checkout ls-files --error-unmatch -- "${source%%:*}" >/dev/null 2>&1 || printf '%s\n' "$entry"
  done)
  stored=$(in_checkout status --porcelain -z --ignored --untracked-files=normal | LC_ALL=C tr '\0' '\n' | sed -n 's/^!! //p' | while IFS= read -r entry; do
    entry_name=${entry%/}
    entry_name=${entry_name##*/}
    if is_store_name "$entry_name"; then
      printf '%s\n' "$entry"
    fi
  done)
  if [ -n "$changed$untracked$flagged$hidden$stored" ]; then
    printf '%s\n' "$changed" "$untracked" "$flagged" "$hidden" "$stored" | sed '/^$/d'
    printf 'A step or a check changed these files of the checkout, left them untracked and not ignored by a tracked .gitignore, hid them by an index flag, or left a file named like a store file of the capture store, which no ignore rule makes right. A check builds with --locked and writes only under target/ and its temporary directory. On the windows job a file that differs by its line endings alone needs an eol attribute in .gitattributes.\n'
    return 1
  fi
  printf 'PASS %s\n' "$name"
}

self_test() {
  for variable in $(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p'); do
    unset "$variable"
  done
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  wrong=
  rejected=0
  passed=0
  skipped=
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then
      wrong="$wrong $1"
    elif [ "$2" = FAIL ]; then
      rejected=$((rejected + 1))
    else
      passed=$((passed + 1))
    fi
  }
  commit_tree() {
    tree_id=$(git -C "$1" write-tree) || return 1
    commit_id=$(printf 'tree %s\nauthor lockfile fixture <fixture@example.invalid> 1 +0000\ncommitter lockfile fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$tree_id" | git -C "$1" hash-object -t commit -w --stdin) || return 1
    printf '%s\n' "$commit_id"
  }
  template=$work/template
  mkdir "$template" || exit 2
  git -C "$template" init -q || exit 2
  git -C "$template" config core.autocrlf false
  printf 'line 1\n' >"$template/tracked.txt"
  printf '%s\n' '/target/' '/store.db*' '/store.key' '/create.pending' '/writer.lock' '/daemon.lock' '/daemon-state.json' '/delete-all.intent' '/*.seg' >"$template/.gitignore"
  git -C "$template" add tracked.txt .gitignore || exit 2
  base=$(commit_tree "$template") || exit 2
  git -C "$template" update-ref HEAD "$base" || exit 2
  case_count=0
  fresh() {
    case_count=$((case_count + 1))
    tree=$work/case-$case_count
    cp -Rp "$template" "$tree" || exit 2
  }
  run_case() {
    case_name=$1 want=$2 job=$3
    got=FAIL
    (inspect "$tree" "$base" "$job") >"$work/case-$case_count.out" 2>&1 && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  plain_sees() {
    seen=$(git -C "$tree" status --porcelain --ignored --untracked-files=all; git -C "$tree" ls-files -v | awk '!/^H /')
    printf '%s' "$seen" | grep -q .
  }
  armed_case() {
    if plain_sees; then
      printf 'case %s/%s not armed: plain git sees the change\n' "$name" "$1"
      wrong="$wrong $1"
      return 0
    fi
    run_case "$@"
  }

  fresh; run_case clean-checkout-on-macos PASS macos
  fresh; run_case clean-checkout-on-windows PASS windows
  fresh; printf 'line 2\n' >>"$tree/tracked.txt"; run_case tracked-file-changed FAIL macos
  fresh; printf 'line 2\n' >>"$tree/tracked.txt"; run_case tracked-file-changed-on-windows FAIL windows
  fresh; : >"$tree/new.txt"; run_case untracked-file FAIL macos
  fresh; : >"$tree/new.txt"; run_case untracked-file-on-windows FAIL windows
  fresh
  moved_tree=$(git -C "$tree" write-tree) || exit 2
  moved=$(printf 'tree %s\nparent %s\nauthor lockfile fixture <fixture@example.invalid> 2 +0000\ncommitter lockfile fixture <fixture@example.invalid> 2 +0000\n\nmoved\n' "$moved_tree" "$base" | git -C "$tree" hash-object -t commit -w --stdin) || exit 2
  git -C "$tree" update-ref HEAD "$moved"
  run_case head-moved FAIL macos
  fresh; printf 'secret.txt\n' >"$work/excludes-$case_count"; git -C "$tree" config core.excludesFile "$work/excludes-$case_count"; : >"$tree/secret.txt"; run_case hidden-by-core-excludesfile FAIL macos
  fresh; git -C "$tree" update-index --assume-unchanged tracked.txt; run_case index-flag-assume-unchanged FAIL macos
  fresh; git -C "$tree" update-index --skip-worktree tracked.txt; run_case index-flag-skip-worktree FAIL windows
  fresh; mkdir "$tree/out"; printf '*\n' >"$tree/out/.gitignore"; : >"$tree/out/file"; run_case hidden-by-an-untracked-gitignore FAIL macos
  fresh; printf 'secret.txt\n' >"$tree/.git/info/exclude"; : >"$tree/secret.txt"; run_case hidden-by-info-exclude FAIL macos
  fresh; mkdir "$tree/target"; : >"$tree/target/build.out"; run_case other-file-ignored-by-a-tracked-rule PASS macos
  fresh; mkdir -p "$tree/target/tmp/case"; : >"$tree/target/tmp/case/store.key"; run_case store-key-inside-an-ignored-directory-is-not-seen PASS macos
  for store_file in store.key store.db store.db-wal store.db-shm create.pending writer.lock daemon.lock daemon-state.json delete-all.intent 0123456789abcdef0123456789abcdef.seg; do
    fresh; : >"$tree/$store_file"; run_case "$store_file-ignored-by-a-tracked-rule" FAIL macos
  done
  fresh; : >"$tree/store.key"; run_case store-key-ignored-by-a-tracked-rule-on-windows FAIL windows
  fresh; mkdir "$tree/store.db.d"; : >"$tree/store.db.d/x"; run_case directory-named-like-a-store-file FAIL macos
  fresh; printf 'store.key\n' >"$tree/.git/info/exclude"; mkdir "$tree/deep"; : >"$tree/deep/store.key"; run_case store-key-ignored-by-info-exclude FAIL macos

  fresh
  printf 'line 2\n' >>"$tree/tracked.txt"
  git -C "$tree" config core.worktree "$work/template"
  armed_case work-tree-in-git-settings-is-another-checkout FAIL macos

  fresh
  printf 'line 2\n' >>"$tree/tracked.txt"
  git -C "$tree" add tracked.txt
  replaced=$(commit_tree "$tree") || exit 2
  git -C "$tree" replace "$base" "$replaced"
  armed_case replace-ref-beside-a-written-index FAIL macos

  fresh
  cp "$tree/.git/index" "$work/clean-index-$case_count"
  git -C "$tree" update-index --skip-worktree tracked.txt
  GIT_INDEX_FILE=$work/clean-index-$case_count
  export GIT_INDEX_FILE
  armed_case index-file-variable-names-another-index FAIL macos
  unset GIT_INDEX_FILE

  fresh
  printf '#!/bin/sh\nprintf "token\\0"\n' >"$work/fsmonitor-hook"
  chmod +x "$work/fsmonitor-hook"
  git -C "$tree" config core.fsmonitor "$work/fsmonitor-hook"
  git -C "$tree" config core.untrackedCache false
  git -C "$tree" update-index -q --refresh
  git -C "$tree" update-index --fsmonitor >/dev/null 2>&1
  git -C "$tree" status --porcelain >/dev/null 2>&1
  sleep 1
  printf 'line 2\n' >>"$tree/tracked.txt"
  armed_case file-system-monitor-hook-hides-a-change FAIL macos

  fresh
  chmod +x "$tree/tracked.txt"
  if git -C "$tree" -c core.fileMode=true diff --quiet; then
    skipped="$skipped mode-change"
    printf 'case %s/mode-change skipped: this system keeps no mode bit that git reads\n' "$name"
  else
    run_case mode-change-counts-on-macos FAIL macos
  fi

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s; outputs in %s)\n' "$name" "$wrong" "$work"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected, %s passing fixtures passed%s)\n' "$name" "$rejected" "$passed" "${skipped:+, skipped:$skipped}"
}

case ${1:-} in
  --self-test) self_test ;;
  macos|windows)
    [ -n "${GITHUB_WORKSPACE:-}" ] && [ -n "${GITHUB_SHA:-}" ] || {
      printf 'sh scripts/lockfile-unchanged.sh %s reads GITHUB_WORKSPACE and GITHUB_SHA, and one of them is empty\n' "$1" >&2
      exit 64
    }
    inspect "$GITHUB_WORKSPACE" "$GITHUB_SHA" "$1"
    ;;
  *) usage ;;
esac
