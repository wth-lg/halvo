#!/bin/sh
set -u
LC_ALL=C
export LC_ALL

usage() {
  cat <<'EOF'
usage: sh scripts/ci/run-ci-d.sh <job> [--self-test] [NAME...]
       sh scripts/ci/run-ci-d.sh --list [<job>]
       sh scripts/ci/run-ci-d.sh --rules
<job> is macos or windows. Start it at the repo root.
scripts/ci/README.md states the interface.
EOF
}

nl='
'
root=${CI_D_SCRIPTS:-scripts}
st_jobs_set=${CI_D_SELF_TEST_JOBS+set}
st_jobs=${CI_D_SELF_TEST_JOBS-}
ck_jobs_set=${CI_D_CHECK_JOBS+set}
ck_jobs=${CI_D_CHECK_JOBS-}
st_limit_set=${CI_D_SCRIPT_LIMIT_S+set}
st_limit=${CI_D_SCRIPT_LIMIT_S-1200}
st_own_limit=$st_limit
[ -n "$st_limit_set" ] || st_own_limit=2800
st_perl="setpgrp(0, 0); my \$f = delete \$ENV{RUN_CI_D_PID_FILE}; if (defined \$f) { open(my \$h, '>', \$f) or exit 127; print \$h \"\$\$\\n\"; close(\$h) or exit 127; } \$SIG{INT} = \$SIG{QUIT} = 'DEFAULT'; exec { \$ARGV[0] } @ARGV; exit 127;"
mode=run
job=
wanted=
seen=' '

case ${1:-} in
  -h | --help)
    usage
    exit 0
    ;;
  --rules)
    [ $# -eq 1 ] || { usage >&2; exit 64; }
    mode=rules
    ;;
  --list)
    [ $# -le 2 ] || { usage >&2; exit 64; }
    mode=list
    job=${2:-}
    ;;
  macos | windows)
    job=$1
    shift
    for arg in "$@"; do
      case $arg in
        --self-test) mode='self-test' ;;
        '' | -* | *[!a-z0-9-]*) usage >&2; exit 64 ;;
        *) wanted="$wanted $arg" ;;
      esac
    done
    ;;
  *)
    usage >&2
    exit 64
    ;;
esac
case $mode:$job in
  rules: | list: | *:macos | *:windows) ;;
  *) usage >&2; exit 64 ;;
esac

if [ -n "$job" ] && [ "$mode" != list ] && [ -n "${RUN_CI_D_NESTED+set}" ]; then
  printf 'FAIL run-ci-d (RUN_CI_D_NESTED is set; the driver refuses it, also when it is empty)\n'
  exit 1
fi

# shellcheck disable=SC2329
st_top() {
  trap '' INT TERM HUP
  if [ -n "$st_worker" ]; then
    kill -s "$3" "$st_worker" 2>/dev/null
    wait "$st_worker"
  fi
  exit "$2"
}

if [ -n "$job" ] && [ "$mode" != list ] && [ "${RUN_CI_D_WORKER:-}" != "$PPID" ]; then
  st_worker=
  trap 'st_top INT 130 USR1' INT
  trap 'st_top TERM 143 TERM' TERM
  trap 'st_top HUP 129 HUP' HUP
  RUN_CI_D_WORKER=$$ sh "$0" "$job" "$@" &
  st_worker=$!
  wait "$st_worker"
  exit "$?"
fi

nested_word=
if [ -n "$job" ] && [ "$mode" != list ] && { true <&9; } 2>/dev/null; then
  read -r nested_word <&9 || nested_word=
  exec 9<&-
  case $nested_word in
    yes | fail | slow | hold | late | gap | wgap) ;;
    *) nested_word= ;;
  esac
fi

cargo_home=${CARGO_HOME:-$HOME/.cargo}
if command -v cygpath >/dev/null 2>&1; then
  cargo_home=$(cygpath -u "$cargo_home")
fi
script_path=$cargo_home/bin:$PATH
cargo_bin=$cargo_home
while :; do
  case $cargo_bin in
    ?*/) cargo_bin=${cargo_bin%/} ;;
    *) break ;;
  esac
done
cargo_bin=$cargo_bin/bin
own_path=
path_rest=$PATH:
while [ -n "$path_rest" ]; do
  path_entry=${path_rest%%:*}
  path_rest=${path_rest#*:}
  path_cut=$path_entry
  while :; do
    case $path_cut in
      ?*/) path_cut=${path_cut%/} ;;
      *) break ;;
    esac
  done
  [ "$path_cut" = "$cargo_bin" ] && continue
  # shellcheck disable=SC3013
  [ -n "$path_entry" ] && [ "$path_entry" -ef "$cargo_bin" ] && continue
  own_path=$own_path${own_path:+:}$path_entry
done
PATH=$own_path
export PATH
perl_cmd=$(command -v perl) || perl_cmd=perl
own_git_names=$(env | sed -n 's/^\(GIT_[A-Za-z0-9_]*\)=.*/\1/p')

own_git() {
  (
    for own_git_name in $own_git_names; do
      unset "$own_git_name"
    done
    GIT_CONFIG_GLOBAL=/dev/null exec git -c core.attributesFile=/dev/null "$@"
  )
}

own_git_index() {
  (
    for own_git_name in $own_git_names; do
      unset "$own_git_name"
    done
    GIT_INDEX_FILE=$tree_index GIT_CONFIG_GLOBAL=/dev/null exec git -c core.attributesFile=/dev/null "$@"
  )
}

cdup=$(own_git rev-parse --show-cdup 2>/dev/null)
code=$?
if [ "$code" -ne 0 ] || [ -n "$cdup" ]; then
  printf 'FAIL run-ci-d (start it at the root of the git work tree)\n'
  exit 1
fi
repo=$(pwd)
list_form() {
  while :; do
    case $list_root in
      ./?*) list_root=${list_root#./} ;;
      *//*) list_root=${list_root%%//*}/${list_root#*//} ;;
      ?*/) list_root=${list_root%/} ;;
      *) break ;;
    esac
  done
}
list_root=$root
list_form
case $list_root in
  "$repo"/*) list_root=${list_root#"$repo"/} ;;
  "$repo") list_root=. ;;
esac
list_form
case $root in
  /* | [A-Za-z]:[/\\]*) list_cd=$root ;;
  *) list_cd=./$root ;;
esac
list_top=$(pwd -P 2>/dev/null) || list_top=
list_phys=$(cd -P "$list_cd" 2>/dev/null && pwd -P) || list_phys=
if [ -n "$list_top" ] && [ -n "$list_phys" ]; then
  case $list_phys in
    "$list_top"/*) list_root=${list_phys#"$list_top"/} ;;
    "$list_top") list_root=. ;;
  esac
fi
tracked=$(own_git -c core.quotePath=false ls-files -- "$list_root" 2>/dev/null) || tracked=

inspect() {
  script_file=$1
  script_name=${script_file##*/}
  script_name=${script_name%.sh}
  script_jobs=
  script_rule=
  script_lane=
  if [ -L "$script_file" ] || [ ! -f "$script_file" ]; then
    refusal="it is not a regular file"
    return 1
  fi
  case ${script_file##*/} in
    *.sh) ;;
    *)
      refusal="the file name is not NN-name.sh, two digits and a name of a-z, 0-9 and -"
      return 1
      ;;
  esac
  case ${script_name#[0-9][0-9]-} in
    "$script_name" | '' | *[!a-z0-9-]*)
      refusal="the file name is not NN-name.sh, two digits and a name of a-z, 0-9 and -"
      return 1
      ;;
  esac
  script_jobs=$(sed -n '1,10s/^# jobs: //p' "$script_file")
  case $script_jobs in
    '' | *"$nl"*)
      refusal="its first 10 lines hold no single '# jobs:' line"
      return 1
      ;;
    *[!a-z\ ]*)
      refusal="its '# jobs:' line names something other than macos and windows"
      return 1
      ;;
  esac
  for word in $script_jobs; do
    case $word in
      macos | windows) ;;
      *)
        refusal="its '# jobs:' line names $word; a job is macos or windows"
        return 1
        ;;
    esac
  done
  script_rule=$(sed -n '1,10s/^# rule: //p' "$script_file")
  case $script_rule in
    '' | *"$nl"*)
      refusal="its first 10 lines hold no single '# rule:' line"
      return 1
      ;;
  esac
  script_lane=$(sed -n '1,10{/^# lane:/p;}' "$script_file")
  script_lane=${script_lane%"$(printf '\r')"}
  case $script_lane in
    '') script_lane=cargo ;;
    '# lane: free') script_lane=free ;;
    '# lane: alone') script_lane=alone ;;
    *"$nl"*)
      refusal="its first 10 lines hold more than one '# lane:' line"
      return 1
      ;;
    *)
      refusal="its '# lane:' line is neither '# lane: free' nor '# lane: alone'; a check that names no lane runs in the cargo lane"
      return 1
      ;;
  esac
  script_limit=$(sed -n '1,10{/^# limit:/p;}' "$script_file")
  script_limit=${script_limit%"$(printf '\r')"}
  case $script_limit in
    '') ;;
    *"$nl"*)
      refusal="its first 10 lines hold more than one '# limit:' line"
      return 1
      ;;
    '# limit: '[1-9]*)
      script_limit=${script_limit#'# limit: '}
      case $script_limit in
        *[!0-9]* | ??????????*)
          refusal="its '# limit:' line is not a whole number of seconds from 1 to 999999999"
          return 1
          ;;
      esac
      ;;
    *)
      refusal="its '# limit:' line is not a whole number of seconds from 1 to 999999999"
      return 1
      ;;
  esac
  list_file=$list_root/${script_file#"$root"/}
  [ "$list_root" != . ] || list_file=${script_file#"$root"/}
  case $nl$tracked$nl in
    *"$nl$list_file$nl"*) ;;
    *)
      refusal='git does not track it'
      return 1
      ;;
  esac
  case $seen in
    *" $script_name "*)
      refusal="a second script has this name: $script_file"
      return 1
      ;;
  esac
  seen="$seen$script_name "
  refusal=
  return 0
}

is_entry() {
  case ${1##*/} in
    . | ..) return 1 ;;
  esac
  [ -e "$1" ] || [ -L "$1" ]
}

names_job() {
  case " $script_jobs " in
    *" $1 "*) return 0 ;;
  esac
  return 1
}

if [ "$mode" = list ] || [ "$mode" = rules ]; then
  status=0
  count=0
  for file in "$root"/*/ci.d/* "$root"/*/ci.d/.*; do
    is_entry "$file" || continue
    if ! inspect "$file"; then
      printf 'FAIL %s (%s)\n' "$script_name" "$refusal"
      status=1
      continue
    fi
    if [ "$mode" = rules ]; then
      printf '%s (%s): %s\n' "$script_name" "$script_jobs" "$script_rule"
    else
      [ -z "$job" ] || names_job "$job" || continue
      printf '%s\n' "$file"
    fi
    count=$((count + 1))
  done
  if [ "$count" -eq 0 ] && [ "$status" -eq 0 ]; then
    printf 'FAIL run-ci-d (no scripts%s)\n' "${job:+ for $job}"
    status=1
  fi
  exit "$status"
fi

tmp=$(mktemp -d "${TMPDIR:-/tmp}/run-ci-d.XXXXXX") || {
  printf 'FAIL run-ci-d (mktemp failed)\n'
  exit 1
}
trap 'st_clear "$tmp"' EXIT
tdirs=$(mktemp -d "${TMPDIR:-/tmp}/run-ci-d-tmp.XXXXXX") || {
  printf 'FAIL run-ci-d (mktemp failed)\n'
  exit 1
}
trap 'st_clear "$tdirs"; st_clear "$tmp"' EXIT
recap=$tmp/recap
: >"$recap"
passed=0
failed=0
skipped=0
selected=0
status=0

tree_index=$tmp/index
if command -v cygpath >/dev/null 2>&1; then
  tree_index=$(cygpath -m "$tree_index")
fi

case $0 in
  /*) self=$0 ;;
  *) self=$repo/$0 ;;
esac

record() {
  printf '%s\n' "$2" >>"$recap"
  case $1 in
    PASS) passed=$((passed + 1)) ;;
    FAIL) failed=$((failed + 1)) ;;
    SKIP) skipped=$((skipped + 1)) ;;
  esac
}

judge() {
  if [ "$mode" = self-test ]; then
    tag=self-test:$script_name
    case $code:$last in
      "0:PASS $tag ("*" failing fixtures rejected)")
        k=${last#"PASS $tag ("}
        k=${k%" failing fixtures rejected)"}
        case $k in
          '' | *[!0-9]*) record FAIL "FAIL $tag (exit 0)" ;;
          *)
            if [ "$k" -gt 0 ]; then
              record PASS "$last"
            else
              record FAIL "FAIL $tag (0 failing fixtures rejected)"
            fi
            ;;
        esac
        ;;
      "1:FAIL $tag ("*")") record FAIL "$last" ;;
      *) record FAIL "FAIL $tag (exit $code)" ;;
    esac
    return
  fi
  case $code:$last in
    "0:PASS $script_name") record PASS "$last" ;;
    "1:FAIL $script_name ("*")") record FAIL "$last" ;;
    "77:SKIP $script_name ("*")")
      if [ "${CI:-}" = true ]; then
        reason=${last#"SKIP $script_name ("}
        record FAIL "FAIL $script_name (skipped in CI: $reason"
      else
        record SKIP "$last"
      fi
      ;;
    *) record FAIL "FAIL $script_name (exit $code)" ;;
  esac
}


tree_state() {
  tree_head=$(own_git rev-parse -q --verify HEAD 2>/dev/null) || tree_head=none
  (
    printf 'HEAD %s\n' "$tree_head"
    own_git_index status --porcelain --untracked-files=no 2>/dev/null || printf 'status failed\n'
    own_git diff-index --cached --name-only HEAD >"$tmp/staged" 2>/dev/null || printf 'status failed\n'
    sed 's/^/I  /' "$tmp/staged"
    own_git ls-files -v >"$tmp/ls-files" 2>/dev/null || printf 'status failed\n'
    sed -n 's/^\([^H]\) /!\1 /p' "$tmp/ls-files"
    tree_h=$(own_git config --list --show-origin 2>/dev/null | own_git hash-object --stdin 2>/dev/null) || tree_h=unreadable
    printf 'GIT config %s\n' "$tree_h"
    tree_h=none
    tree_p=$(own_git rev-parse --git-path info/attributes 2>/dev/null) || tree_p=
    [ ! -f "$tree_p" ] || tree_h=$(own_git hash-object --no-filters -- "$tree_p" 2>/dev/null) || tree_h=unreadable
    printf 'GIT info/attributes %s\n' "$tree_h"
    own_git for-each-ref --format='GIT %(refname) %(objectname)' refs/replace/ 2>/dev/null || printf 'status failed\n'
  ) >"$1"
}

tree_report() {
  case $1:$2 in
    ':HEAD '* | ':GIT '*) return ;;
    *':HEAD '*) tree_msg="$1 moved HEAD" ;;
    *':GIT '*)
      tree_msg=${2#GIT }
      tree_msg="$1 changed the git settings of the checkout: ${tree_msg%% *}"
      ;;
    ':status failed') tree_msg='git cannot read the tracked files before the first script' ;;
    *':status failed') tree_msg="git cannot read the tracked files after $1" ;;
    *)
      if [ -z "$1" ]; then
        case $2 in
          '!'*) tree_msg="a tracked file is marked assume-unchanged or skip-worktree before the first script: ${2#???}" ;;
          *) tree_msg="a tracked file differs from HEAD before the first script: ${2#???}" ;;
        esac
      else
        tree_msg="$1 changed a tracked file: ${2#???}"
      fi
      ;;
  esac
  case $nl$tree_said$nl in
    *"$nl$tree_msg$nl"*) return ;;
  esac
  tree_said=$tree_said$nl$tree_msg
  if [ -n "$tree_to" ]; then
    printf '%s\n' "FAIL run-ci-d ($tree_msg)" >>"$tree_to"
  else
    record FAIL "FAIL run-ci-d ($tree_msg)"
  fi
}

tree_check() {
  tree_said=
  tree_state "$tmp/tree.after"
  while IFS= read -r tree_line; do
    grep -qxF -e "$tree_line" "$tmp/tree.before" && continue
    tree_report "$1" "$tree_line"
    printf '%s\n' "$tree_line" >>"$tmp/tree.reported"
  done <"$tmp/tree.after"
  while IFS= read -r tree_line; do
    case $tree_line in
      'HEAD '* | 'GIT config '* | 'GIT info/attributes '*) continue ;;
    esac
    grep -qxF -e "$tree_line" "$tmp/tree.after" && continue
    grep -qxF -e "$tree_line" "$tmp/tree.reported" && continue
    tree_report "$1" "$tree_line"
  done <"$tmp/tree.before"
  mv "$tmp/tree.after" "$tmp/tree.before"
}

tree_start() {
  : >"$tmp/tree.reported"
  tree_said=
  own_git_index read-tree HEAD >/dev/null 2>&1
  own_git_index update-index -q --refresh >/dev/null 2>&1
  tree_state "$tmp/tree.before"
  own_git --no-optional-locks status --porcelain --untracked-files=no >"$tmp/tree.checkout" 2>/dev/null || printf 'status failed\n' >"$tmp/tree.checkout"
  while IFS= read -r tree_line; do
    tree_report '' "$tree_line"
  done <"$tmp/tree.before"
  while IFS= read -r tree_line; do
    tree_report '' "$tree_line"
  done <"$tmp/tree.checkout"
}

note() {
  eval "st_kind_$st_n=note st_state_$st_n=done st_line_$st_n=\$1"
  st_n=$((st_n + 1))
}

st_drop() {
  eval "st_kind_$1=note st_state_$1=done st_line_$1=\$2"
}

st_add() {
  st_a_limit=$st_limit
  [ -n "$st_limit_set" ] || [ -z "$4" ] || [ "$4" -le "$st_a_limit" ] || st_a_limit=$4
  eval "st_kind_$st_n=script st_file_$st_n=\$1 st_name_$st_n=\$2 st_lane_$st_n=\$3 st_lim_$st_n=\$st_a_limit st_state_$st_n=waiting"
  st_n=$((st_n + 1))
}

st_track() {
  st_now=$(date +%s)
  eval "st_pid_$1=\$2 st_dir_$1=\$3 st_since_$1=\$st_now st_state_$1=running"
  st_running="$st_running$1 "
  st_beside="$st_beside$1 "
  st_naps=0
}

st_own() {
  st_d=$tmp/self-test.run-ci-d
  mkdir -p "$st_d" || {
    note 'FAIL self-test:run-ci-d (its temporary directory cannot be made)'
    return
  }
  (
    record() {
      printf '%s %s\n' "$1" "$2" >"$st_d/verdict"
    }
    own_self_test
    : >"$st_d/done"
  ) </dev/null >"$st_d/out" 2>"$st_d/err" &
  eval "st_kind_$st_n=own st_name_$st_n=run-ci-d st_lim_$st_n=$st_own_limit"
  st_n=$((st_n + 1))
  st_track "$((st_n - 1))" "$!" "$st_d"
}

st_start() {
  eval "st_q_file=\$st_file_$1 st_q_name=\$st_name_$1"
  st_q_tag=$st_q_name
  st_q_arg=
  if [ "$mode" = self-test ]; then
    st_q_tag=self-test:$st_q_name
    st_q_arg=--self-test
  fi
  st_q_artifacts=$repo/target/ci-artifacts/$st_q_name
  mkdir -p "$st_q_artifacts" || {
    st_drop "$1" "FAIL $st_q_name (its artifacts directory cannot be made)"
    return 1
  }
  st_d=$tmp/$mode.$st_q_name
  st_td=$tdirs/$mode.$st_q_name/t
  mkdir -p "$st_d" "$st_td" || {
    st_drop "$1" "FAIL $st_q_tag (its temporary directory cannot be made)"
    return 1
  }
  eval "st_tdir_$1=\$st_td"
  (
    CI_D_REPO=$repo
    CI_D_JOB=$job
    CI_D_ARTIFACTS=$st_q_artifacts
    TMPDIR=$st_td
    export CI_D_REPO CI_D_JOB CI_D_ARTIFACTS TMPDIR
    st_t0=$(date +%s)
    {
      RUN_CI_D_PID_FILE=$st_d/pid PATH=$script_path "$perl_cmd" -e "$st_perl" sh "$st_q_file" ${st_q_arg:+"$st_q_arg"} </dev/null 2>"$st_d/err" &
      wait "$!"
      printf '%s\n' "$?" >"$st_d/status"
    } | cat >"$st_d/out"
    st_t1=$(date +%s)
    st_c=none
    [ ! -s "$st_d/status" ] || read -r st_c <"$st_d/status"
    printf '%s %s\n' "$st_c" "$((st_t1 - st_t0))" >"$st_d/code"
    : >"$st_d/done"
  ) </dev/null >/dev/null 2>&1 &
  st_track "$1" "$!" "$st_d"
}

st_queue() {
  st_q_file=$script_file
  st_q_name=$script_name
  st_q_solo=$(sed -n '1,10s/^# self-test: alone$/alone/p' "$st_q_file")
  case $st_alone in
    *" $st_q_name "*) st_q_solo=alone ;;
  esac
  if [ -n "$st_q_solo" ]; then
    st_wait_below 1
  else
    st_wait_below "$st_jobs"
  fi
  st_add "$st_q_file" "$st_q_name" "$script_lane" "$script_limit"
  st_start "$((st_n - 1))" || return
  [ -z "$st_q_solo" ] || st_wait_below 1
}

st_pending() {
  eval "st_pending_i=\$st_cursor_$1"
  while [ "$st_pending_i" -lt "$st_n" ]; do
    eval "st_pk=\$st_kind_$st_pending_i:\$st_state_$st_pending_i:\${st_lane_$st_pending_i-}"
    [ "$st_pk" != "script:waiting:$1" ] || break
    st_pending_i=$((st_pending_i + 1))
  done
  eval "st_cursor_$1=\$((st_pending_i + 1))"
  [ "$st_pending_i" -lt "$st_n" ]
}

st_lanes() {
  st_busy=
  st_free=0
  for st_l in cargo free alone; do
    eval "st_cursor_$st_l=0"
  done
  while :; do
    st_poll
    while [ -z "$st_busy" ] && st_pending cargo; do
      ! st_start "$st_pending_i" || st_busy=yes
    done
    while [ "$st_free" -lt "$ck_jobs" ] && st_pending free; do
      ! st_start "$st_pending_i" || st_free=$((st_free + 1))
    done
    if [ "$st_running" = ' ' ]; then
      while st_pending alone; do
        ! st_start "$st_pending_i" || break
      done
    fi
    st_flush
    [ "$st_running" != ' ' ] || return 0
    st_nap
  done
}

st_finish() {
  eval "st_f_d=\$st_dir_$1 st_f_td=\$st_tdir_$1 st_f_name=\$st_name_$1 st_f_lane=\$st_lane_$1"
  if [ "$st_f_lane" = free ]; then
    st_free=$((st_free - 1))
  else
    st_busy=
  fi
  : >"$tmp/after.$1"
  st_f_pid=
  [ ! -s "$st_f_d/pid" ] || read -r st_f_pid <"$st_f_d/pid"
  [ -z "$st_f_pid" ] || st_end_group 0 "$st_f_pid"
  st_clear "$st_f_td" || printf 'FAIL %s (its TMPDIR cannot be removed)\n' "$st_f_name" >>"$tmp/after.$1"
  st_f_who=
  st_f_count=0
  st_f_j=0
  while [ "$st_f_j" -lt "$st_n" ]; do
    case $st_beside in
      *" $st_f_j "*)
        eval "st_f_other=\$st_name_$st_f_j"
        st_f_who="$st_f_who${st_f_who:+, }$st_f_other"
        st_f_count=$((st_f_count + 1))
        ;;
    esac
    st_f_j=$((st_f_j + 1))
  done
  [ "$st_f_count" -le 1 ] || st_f_who="one of $st_f_who"
  tree_to=$tmp/after.$1
  tree_check "$st_f_who"
  tree_to=
  st_beside="${st_beside%%" $1 "*} ${st_beside#*" $1 "}"
}

# shellcheck disable=SC2329
st_group() {
  perl -e "exit(kill(\$ARGV[0], -\$ARGV[1]) ? 0 : 1)" "$1" "$2" 2>/dev/null
}

# shellcheck disable=SC2329
st_interrupt() {
  trap '' INT TERM HUP USR1
  [ -z "$st_sleep" ] || kill -s TERM "$st_sleep" 2>/dev/null
  st_groups=
  for st_i in $st_running $st_ending; do
    eval "st_d=\$st_dir_$st_i st_p=\$st_pid_$st_i"
    st_sp=
    [ ! -s "$st_d/pid" ] || read -r st_sp <"$st_d/pid"
    [ -z "$st_sp" ] || st_groups="$st_groups $st_sp"
    kill -s TERM "$st_p" 2>/dev/null
  done
  if [ "$mode" = self-test ]; then
    st_i=0
    while [ "$st_i" -lt "$st_n" ]; do
      eval "st_k=\$st_kind_$st_i st_d=\${st_dir_$st_i-}"
      st_sp=
      [ "$st_k" != script ] || [ ! -s "$st_d/pid" ] || read -r st_sp <"$st_d/pid"
      case " $st_groups " in
        *" $st_sp "*) ;;
        *) [ -z "$st_sp" ] || st_groups="$st_groups $st_sp" ;;
      esac
      st_i=$((st_i + 1))
    done
  fi
  printf '' >"$tmp/own-stopping"
  for st_f in "$tmp/nested.pid" "$tmp/witness.pid"; do
    st_sp=
    [ ! -s "$st_f" ] || read -r st_sp <"$st_f"
    [ -z "$st_sp" ] || st_groups="$st_groups $st_sp"
  done
  for st_g in $st_groups; do
    st_group TERM "$st_g"
  done
  st_grace=0
  while [ "$st_grace" -lt 50 ]; do
    st_alive=
    for st_g in $st_groups; do
      ! st_group 0 "$st_g" || st_alive=yes
    done
    [ -n "$st_alive" ] || break
    sleep 0.1
    st_grace=$((st_grace + 1))
  done
  for st_g in $st_groups; do
    st_group KILL "$st_g"
  done
  if [ "$mode" != self-test ]; then
    st_i=$st_next
    while [ "$st_i" -lt "$st_n" ]; do
      eval "st_k=\$st_kind_$st_i st_x=\$st_state_$st_i st_d=\${st_dir_$st_i-}"
      if [ "$st_k" = script ] && [ "$st_x" != waiting ]; then
        [ ! -e "$st_d/out" ] || cat "$st_d/out"
        [ ! -e "$st_d/err" ] || cat "$st_d/err" >&2
      fi
      st_i=$((st_i + 1))
    done
  fi
  printf 'FAIL run-ci-d (the step was interrupted by %s; every %s that it started is ended)\n' "$1" "$st_what"
  exit "$2"
}

st_end_group() {
  perl -e "my \$p = shift; my @g = map { split ' ' } @ARGV; for (my \$t = 0; \$t < \$p + 60; \$t++) { my @a = grep { kill(0, -\$_) } @g; last unless @a; kill('TERM', map { -\$_ } @a) if \$t == \$p; kill('KILL', map { -\$_ } @a) if \$t == \$p + 50; select(undef, undef, undef, 0.1); }" "$1" "$2" 2>/dev/null
}

st_leave() {
  eval "st_td=\$st_tdir_$1 st_t=\$st_name_$1"
  [ "$mode" = self-test ] && st_t=self-test:$st_t
  st_clear "$st_td" || record FAIL "FAIL $st_t (its TMPDIR cannot be removed)"
}

st_stop() {
  eval "st_d=\$st_dir_$1 st_p=\$st_pid_$1 st_state_$1=timeout"
  st_own_stop=
  st_sp=
  [ ! -s "$st_d/pid" ] || read -r st_sp <"$st_d/pid"
  if [ -n "$st_sp" ]; then
    st_end_group 0 "$st_sp"
  else
    eval "st_k=\$st_kind_$1"
    if [ "$st_k" = own ]; then
      st_own_stop=yes
      printf '' >"$tmp/own-stopping"
      st_own_groups=
      for st_f in "$tmp/nested.pid" "$tmp/witness.pid"; do
        st_sp=
        [ ! -s "$st_f" ] || read -r st_sp <"$st_f"
        [ -z "$st_sp" ] || st_own_groups="$st_own_groups $st_sp"
      done
      [ -z "$st_own_groups" ] || st_end_group 0 "$st_own_groups"
    fi
    kill -TERM "$st_p" 2>/dev/null
    st_grace=0
    while [ ! -e "$st_d/done" ] && kill -0 "$st_p" 2>/dev/null && [ "$st_grace" -lt 50 ]; do
      sleep 0.1
      st_grace=$((st_grace + 1))
    done
  fi
  st_grace=0
  while [ ! -e "$st_d/done" ] && kill -0 "$st_p" 2>/dev/null && [ "$st_grace" -lt 20 ]; do
    sleep 0.1
    st_grace=$((st_grace + 1))
  done
  [ -e "$st_d/done" ] || kill -KILL "$st_p" 2>/dev/null
  wait "$st_p" 2>/dev/null
  if [ -n "$st_own_stop" ]; then
    true >"$tmp/nested.pid"
    true >"$tmp/witness.pid"
  fi
}

st_poll() {
  st_now=$(date +%s)
  st_left=' '
  st_count=0
  st_ended=
  for st_i in $st_running; do
    eval "st_d=\$st_dir_$st_i st_p=\$st_pid_$st_i st_t=\$st_since_$st_i st_lm=\$st_lim_$st_i"
    if [ -e "$st_d/done" ]; then
      eval "st_state_$st_i=done"
      st_naps=0
    elif ! kill -0 "$st_p" 2>/dev/null; then
      if [ -e "$st_d/done" ]; then
        eval "st_state_$st_i=done"
      else
        eval "st_state_$st_i=lost"
      fi
      st_naps=0
    elif [ "$((st_now - st_t))" -ge "$st_lm" ]; then
      st_stop "$st_i"
      st_naps=0
    else
      st_left="$st_left$st_i "
      st_count=$((st_count + 1))
      continue
    fi
    st_ended="$st_ended $st_i"
  done
  st_running=$st_left
  [ "$mode" != self-test ] || return 0
  st_ending=$st_ended
  for st_i in $st_ended; do
    st_finish "$st_i"
    st_ending=${st_ending#" $st_i"}
  done
}

st_print() {
  eval "st_d=\$st_dir_$1 st_k=\$st_kind_$1 st_x=\$st_state_$1 st_t=\$st_since_$1 st_lm=\$st_lim_$1 script_name=\$st_name_$1"
  st_tag=$script_name
  [ "$mode" != self-test ] || st_tag=self-test:$script_name
  [ ! -e "$st_d/out" ] || cat "$st_d/out"
  st_e=0
  [ ! -e "$st_d/err" ] || st_e=$(wc -c <"$st_d/err")
  st_e=${st_e##* }
  [ "$st_e" -eq 0 ] || head -c "$st_e" "$st_d/err" >&2
  eval "st_errsize_$1=\$st_e"
  if [ "$st_k" = own ]; then
    st_vs=
    st_vl=
    [ ! -s "$st_d/verdict" ] || read -r st_vs st_vl <"$st_d/verdict"
    case $st_x:$st_vs in
      timeout:*) record FAIL "FAIL self-test:run-ci-d (time limit $st_lm s)" ;;
      *:PASS | *:FAIL) record "$st_vs" "$st_vl" ;;
      *) record FAIL 'FAIL self-test:run-ci-d (it ended without a verdict)' ;;
    esac
    return
  fi
  code=
  st_sec=
  [ ! -s "$st_d/code" ] || read -r code st_sec <"$st_d/code"
  if [ -z "$st_sec" ]; then
    st_now=$(date +%s)
    st_sec=$((st_now - st_t))
  fi
  printf 'seconds %s %s\n' "$script_name" "$st_sec"
  last=
  [ ! -e "$st_d/out" ] || last=$(tail -n 1 "$st_d/out")
  case $st_x:$code in
    timeout:*) record FAIL "FAIL $st_tag (time limit $st_lm s)" ;;
    lost:* | *:none) record FAIL "FAIL $st_tag (it ended without an exit status)" ;;
    *) judge ;;
  esac
  if [ "$mode" != self-test ] && [ -e "$tmp/after.$1" ]; then
    while IFS= read -r st_line; do
      record FAIL "$st_line"
    done <"$tmp/after.$1"
  fi
}

st_clear() {
  st_tries=0
  while [ -e "$1" ] || [ -L "$1" ]; do
    [ "$st_tries" -lt 3 ] || return 1
    [ "$st_tries" -eq 0 ] || sleep 1
    chmod -R u+rwx "$1" 2>/dev/null
    rm -rf "$1" 2>/dev/null
    st_tries=$((st_tries + 1))
  done
}

st_flush() {
  while [ "$st_next" -lt "$st_n" ]; do
    eval "st_k=\$st_kind_$st_next st_x=\$st_state_$st_next"
    if [ "$st_k" = note ]; then
      eval "st_l=\$st_line_$st_next"
      record FAIL "$st_l"
    elif [ "$st_x" = running ] || [ "$st_x" = waiting ]; then
      return 0
    else
      st_print "$st_next"
    fi
    st_next=$((st_next + 1))
  done
}

st_wait_below() {
  while :; do
    st_poll
    st_flush
    [ "$st_count" -ge "$1" ] || return 0
    st_nap
  done
}

st_nap() {
  if [ "$st_naps" -lt 10 ]; then
    sleep 0.1 &
  else
    sleep 0.5 &
  fi
  st_sleep=$!
  wait "$st_sleep"
  st_sleep=
  st_naps=$((st_naps + 1))
}

whole_number() {
  case $1 in
    '' | 0* | *[!0-9]* | ??????????*) return 1 ;;
  esac
}

st_entry() {
  st_word=${1%"$cr"}
  st_word=${st_word#"${st_word%%[![:blank:]]*}"}
  st_reason=${st_word#"${st_word%%[[:blank:]]*}"}
  st_reason=${st_reason#"${st_reason%%[![:blank:]]*}"}
  st_word=${st_word%%[[:blank:]]*}
}

new_repo() {
  fixture_added=
  if [ ! -e "$tmp/fixture-made-once.done" ]; then
    fixture=$tmp/fixture-made-once
    mkdir -p "$fixture" || return 1
    git -C "$fixture" init -q >/dev/null 2>&1 || return 1
    printf 'version = 4\n' >"$fixture/Cargo.lock"
    git -C "$fixture" add Cargo.lock >/dev/null 2>&1 && fixture_commit || return 1
    printf '' >"$tmp/fixture-made-once.done"
  fi
  fixture=$(mktemp -d "$tmp/repo.XXXXXX") || return 1
  cp -Rp "$tmp/fixture-made-once/." "$fixture/"
}

new_repo_by_git() {
  fixture_added=
  fixture=$(mktemp -d "$tmp/repo.XXXXXX") || return 1
  git -C "$fixture" init -q >/dev/null 2>&1 || return 1
  printf 'version = 4\n' >"$fixture/Cargo.lock"
  git -C "$fixture" add Cargo.lock >/dev/null 2>&1 && fixture_commit
}

fixture_commit() {
  if [ -n "$fixture_added" ]; then
    printf '%s' "$fixture_added" | git --literal-pathspecs -C "$fixture" add --pathspec-from-file=- >/dev/null 2>&1 || return 1
    fixture_added=
  fi
  fixture_tree=$(git -C "$fixture" write-tree) || return 1
  fixture_head=$(printf 'tree %s\nauthor ci.d fixture <fixture@example.invalid> 1 +0000\ncommitter ci.d fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$fixture_tree" | git -C "$fixture" hash-object -t commit -w --stdin) || return 1
  git -C "$fixture" update-ref HEAD "$fixture_head"
}

track_file() {
  printf 'line 1\n' >"$fixture/tracked.txt" &&
    git -C "$fixture" add tracked.txt >/dev/null 2>&1 && fixture_commit
}

add_script() {
  mkdir -p "$fixture/scripts/$1/ci.d" || return 1
  printf '#!/bin/sh\n%s\n%s\n' "$3" "$4" >"$fixture/scripts/$1/ci.d/$2" || return 1
  fixture_added="${fixture_added}scripts/$1/ci.d/$2$nl"
}

own_case() {
  case_name=$1
  own_seen="$own_seen$nl$1"
  want_exit=$2
  want_line=$3
  shift 3
  own_run "$@"
  own_judge
}

own_run() {
  [ -z "$fixture_added" ] || fixture_commit
  [ -z "$case_max_s" ] || case_t0=$(date +%s)
  [ -z "$case_index_same" ] || cp "$fixture/.git/index" "$tmp/index.before"
  witness=
  if [ -n "$case_witness" ]; then
    witness_wait=0
    [ "$nested_word" != wgap ] || witness_wait=4
    # shellcheck disable=SC2016
    "$perl_bin" -e 'setpgrp(0, 0); sleep $ARGV[2] if $ARGV[2]; open(my $h, ">", $ARGV[0]) or exit 1; print $h "$$\n"; close($h) or exit 1; if (-e $ARGV[1]) { truncate($ARGV[0], 0); exit 0 } sleep 120' "$tmp/witness.pid" "$tmp/own-stopping" "$witness_wait" </dev/null >/dev/null 2>&1 &
    witness=$!
    if [ "$nested_word" = wgap ]; then
      printf '%s\n' "$witness" >>"$repo/pids"
      printf '%s witness\n' "$witness" >>"$repo/pid-roles"
    fi
  fi
  got_out=$(
    cd "$fixture/$case_cd" || exit 99
    unset CI CI_D_SCRIPTS CI_D_SELF_TEST_JOBS CI_D_SCRIPT_LIMIT_S CI_D_CHECK_JOBS
    [ -z "$case_ci" ] || { CI=$case_ci; export CI; }
    [ -z "$case_scripts" ] || { CI_D_SCRIPTS=$case_scripts; export CI_D_SCRIPTS; }
    [ -z "$case_cdpath" ] || { CDPATH=$case_cdpath; export CDPATH; }
    [ -z "$case_locale" ] || { LC_ALL=$case_locale; export LC_ALL; }
    [ -z "$case_jobs" ] || { CI_D_SELF_TEST_JOBS=$case_jobs; export CI_D_SELF_TEST_JOBS; }
    [ -z "$case_check_jobs" ] || { CI_D_CHECK_JOBS=$case_check_jobs; export CI_D_CHECK_JOBS; }
    [ -z "$case_limit" ] || { CI_D_SCRIPT_LIMIT_S=$case_limit; export CI_D_SCRIPT_LIMIT_S; }
    [ -z "$case_path" ] || { PATH=$case_path:$PATH; export PATH; }
    procs_env=NUMBER_OF_PROCESSORS=$case_procs
    [ "$case_procs" != none ] || procs_env=-uNUMBER_OF_PROCESSORS
    CARGO_HOME=${case_cargo_home:-$tmp/cargo-home}
    CALLER_VALUE=kept
    export CALLER_VALUE
    [ -z "$case_env" ] || mkdir -p "$tmp/case-home" || exit 99
    if [ -n "$case_git_home" ]; then
      HOME=$(mktemp -d "$tmp/case-git-home.XXXXXX") || exit 99
      XDG_CONFIG_HOME=$HOME/config
      export HOME XDG_CONFIG_HOME
      if [ "$case_git_home" = plain ]; then
        unset GIT_CONFIG_GLOBAL
      else
        GIT_CONFIG_GLOBAL=$HOME/gitconfig
        export GIT_CONFIG_GLOBAL
      fi
    fi
    if [ -n "$case_git_count" ]; then
      GIT_CONFIG_COUNT=1
      GIT_CONFIG_KEY_0=core.checkStat
      GIT_CONFIG_VALUE_0=minimal
      export GIT_CONFIG_COUNT GIT_CONFIG_KEY_0 GIT_CONFIG_VALUE_0
    fi
    if [ -n "$case_git_names" ]; then
      GIT_AUTHOR_NAME='A Caller'
      GIT_CASE_VALUE="two  words, and a tab${tab}here"
      GIT_INDEX_FILE=$tmp/no-index-of-the-caller/index
      export GIT_AUTHOR_NAME GIT_CASE_VALUE GIT_INDEX_FILE
    fi
    TMPDIR=${case_tmpdir:-$tmp}
    export CARGO_HOME TMPDIR
    [ "$nested_word" != late ] || sleep 4
    nested_wait=0
    [ "$nested_word" != gap ] || nested_wait=4
    printf 'a line on standard input\n' | RUN_CI_D_PID_FILE=$tmp/nested.pid RUN_CI_D_MARK=$tmp/own-stopping RUN_CI_D_WAIT=$nested_wait ${case_switch_set:+env "RUN_CI_D_NESTED=$case_switch"} ${case_procs:+env "$procs_env"} ${case_env:+env "HOME=$tmp/case-home" SECOND_CALLER_VALUE=kept job=from-the-caller} "$perl_bin" -e "$case_perl" sh ${case_worker:+-c "$worker_wrap"} "$self" "$@" 2>&1 9<<EOF &
${case_nested:-yes}
EOF
    case_pid=$!
    if [ "$nested_word" = hold ] || [ "$nested_word" = late ] || [ "$nested_word" = gap ]; then
      printf '%s\n%s\n' "$case_pid" "$witness" >>"$repo/pids"
      printf '%s nested-driver\n%s witness\n' "$case_pid" "$witness" >>"$repo/pid-roles"
    fi
    if [ -n "$case_signal" ]; then
      n=0
      for case_marker in $case_wait; do
        while [ ! -e "$fixture/$case_marker" ] && [ "$n" -lt 100 ]; do
          sleep 0.1
          n=$((n + 1))
        done
      done
      case_before=0
      if [ -f "$fixture/pids" ]; then
        while IFS= read -r case_left; do
          [ -z "$case_left" ] || ! kill -0 "$case_left" 2>/dev/null || case_before=$((case_before + 1))
        done <"$fixture/pids"
      fi
      printf '%s\n' "$case_before" >"$fixture/before"
      kill -s "$case_signal" "$case_pid"
    fi
    wait "$case_pid"
  )
  got_exit=$?
  true >"$tmp/nested.pid"
  [ -z "$case_max_s" ] || case_t1=$(date +%s)
  left=0
  if [ -f "$fixture/pids" ]; then
    n=0
    while :; do
      left=0
      while IFS= read -r case_left; do
        [ -z "$case_left" ] || ! kill -0 "$case_left" 2>/dev/null || left=$((left + 1))
      done <"$fixture/pids"
      [ "$left" -gt 0 ] && [ "$n" -lt 100 ] || break
      sleep 0.1
      n=$((n + 1))
    done
    while IFS= read -r case_left; do
      [ -n "$case_left" ] || continue
      kill -0 "$case_left" 2>/dev/null || continue
      case_role=
      [ ! -f "$fixture/pid-roles" ] || case_role=$(sed -n "s/^$case_left //p" "$fixture/pid-roles")
      case_what=$(ps -o lstart=,command= -p "$case_left" 2>/dev/null) || case_what=$(ps -p "$case_left" 2>/dev/null | sed -n 2p)
      printf 'case run-ci-d/%s process left: %s (%s) %s\n' "$case_name" "$case_left" "${case_role:-recorded by a fixture script}" "$case_what" | cut -c1-240
    done <"$fixture/pids"
  fi
  witness_left=no
  if [ -n "$witness" ]; then
    ! kill -0 "$witness" 2>/dev/null || witness_left=yes
    kill "$witness" 2>/dev/null
    true >"$tmp/witness.pid"
    wait "$witness" 2>/dev/null
  fi
}

own_unflag() {
  [ -f "$fixture/flagged" ] || return 0
  while IFS= read -r own_flagged; do
    [ ! -e "$own_flagged" ] || chflags nouchg "$own_flagged"
  done <"$fixture/flagged"
}

tmpdirs_gone() {
  tmpdirs_got=no-tmpdirs-recorded
  [ -f "$fixture/tmpdirs" ] || return 1
  tmpdirs_n=$(sort -u "$fixture/tmpdirs" | wc -l)
  tmpdirs_n=${tmpdirs_n##* }
  tmpdirs_got=$tmpdirs_n-tmpdirs-recorded
  [ "$tmpdirs_n" -eq "$case_gone" ] || return 1
  tmpdirs_got=tmpdir-left
  while IFS= read -r tmpdirs_p; do
    [ ! -e "$tmpdirs_p" ] || return 1
  done <"$fixture/tmpdirs"
}

own_judge() {
  before=
  empty_left=
  [ -z "$case_empty" ] || empty_left=$(ls -A "$case_tmpdir")
  once_count=0
  [ -z "$case_once" ] || once_count=$(printf '%s\n' "$got_out" | grep -cxF -e "$case_once")
  if [ -n "$case_signal" ]; then
    [ ! -s "$fixture/before" ] || read -r before <"$fixture/before"
    printf 'case run-ci-d/%s processes by recorded id: %s before the signal, %s after\n' "$case_name" "${before:-none}" "$left"
  fi
  if [ "$want_exit" -eq 0 ]; then want=PASS; else want=FAIL; fi
  if [ "$got_exit" -ne "$want_exit" ]; then
    got=exit-$got_exit
  elif ! printf '%s\n' "$got_out" | grep -qxF -e "$want_line"; then
    got=line-missing
  elif [ -n "$case_absent" ] && printf '%s\n' "$got_out" | grep -qxF -e "$case_absent"; then
    got=line-present
  elif [ -n "$case_witness" ] && [ "$witness_left" != yes ]; then
    got='witness-ended'
  elif [ "$left" -gt 0 ]; then
    got=$left-processes-left
  elif [ -n "$case_index_same" ] && ! cmp -s "$fixture/.git/index" "$tmp/index.before"; then
    got=index-of-the-checkout-changed
  elif [ -n "$case_signal" ] && [ "${before:-0}" -lt "$case_signal_min" ]; then
    got=${before:-no}-processes-before-the-signal
  elif [ -n "$case_gone" ] && ! tmpdirs_gone; then
    got=$tmpdirs_got
  elif [ -n "$empty_left" ]; then
    got=temporary-directory-left
  elif [ -n "$case_made" ] && [ ! -e "$fixture/$case_made" ]; then
    got=no-$case_made
  elif [ -n "$case_once" ] && [ "$once_count" -ne 1 ]; then
    got=$once_count-times-$case_once
  else
    got=$want
    if [ -n "$case_first" ]; then
      case $nl$got_out in
        *"$nl$case_first"*"$nl$case_then"*) ;;
        *) got=misordered ;;
      esac
    fi
    if [ -n "$case_max_s" ] && [ "$((case_t1 - case_t0))" -gt "$case_max_s" ]; then
      got=slower-than-$case_max_s-s
    fi
  fi
  printf 'case run-ci-d/%s want=%s got=%s\n' "$case_name" "$want" "$got"
  [ -z "$case_max_s" ] || printf 'case run-ci-d/%s seconds=%s max=%s\n' "$case_name" "$((case_t1 - case_t0))" "$case_max_s"
  own_clock
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  case_reset
}

own_tree_case() {
  case_name=$1
  own_seen="$own_seen$nl$1"
  want_line=$2
  tree_list=$(own_git -c core.quotePath=false ls-tree -r --name-only HEAD 2>/dev/null | grep -E '(^|/)ci\.d/[^/]+$')
  got_out=
  if [ -n "$tree_list" ]; then
    got_out=$(
      set -f
      IFS=$nl
      # shellcheck disable=SC2086
      set -- $tree_list
      for tree_f in "$@"; do
        [ -f "$tree_f" ] || printf '%s: not in the work tree\n' "$tree_f"
      done
      awk 'FNR <= 10 && /^# limit:/ { sub(/\r$/, ""); print FILENAME ": " $0 }' "$@" 2>/dev/null
    )
  fi
  if [ -z "$tree_list" ]; then
    got=no-ci.d-file-listed
  elif [ "$got_out" = "$want_line" ]; then
    got=PASS
  else
    got=limit-lines-differ
    printf '%s\n' "$got_out" | sed "s|^|case run-ci-d/$case_name limit line: |"
  fi
  printf 'case run-ci-d/%s want=PASS got=%s\n' "$case_name" "$got"
  own_clock
  [ "$got" = PASS ] || wrong="$wrong $case_name"
  case_reset
}

own_clock() {
  [ -z "$nested_word" ] || return 0
  own_now=$(date +%s)
  printf 'case run-ci-d/%s case-seconds=%s\n' "$case_name" "$((own_now - own_mark))"
  own_mark=$own_now
  own_n=$((own_n + 1))
}

own_gate() {
  case "$own_macos_alone$nl" in
    *"$nl$1:"*)
      [ "${2:-$own_host}" = windows ] || return 0
      own_gate_reason=${own_macos_alone#*"$nl$1:"}
      own_gate_reason=${own_gate_reason%%"$nl"*}
      return 1
      ;;
  esac
  return 0
}

own_not_run() {
  own_seen="$own_seen$nl$1"
  own_skipped="$own_skipped$nl$1"
  [ -n "$nested_word" ] || printf 'case run-ci-d/%s not-run (%s)\n' "$1" "$2"
}

own_reached() {
  case "$own_seen$nl" in
    *"$nl$1$nl"*) return 0 ;;
  esac
  return 1
}

own_list_case() {
  case_name=$1
  own_seen="$own_seen$nl$1"
  got=PASS
  own_list_n=0
  own_list_r=0
  while IFS= read -r own_line; do
    [ -n "$own_line" ] || continue
    own_list_name=${own_line%%:*}
    own_list_n=$((own_list_n + 1))
    own_reached "$own_list_name" || { [ "$got" != PASS ] || got=no-case-$own_list_name; }
    ! own_gate "$own_list_name" windows || { [ "$got" != PASS ] || got=the-gate-runs-$own_list_name-on-windows; }
  done <<EOF
$own_macos_alone
EOF
  while IFS= read -r own_line; do
    [ -n "$own_line" ] || continue
    own_list_rule=${own_line%%:*}
    own_list_r=$((own_list_r + 1))
    own_list_held=
    # shellcheck disable=SC2086
    for own_list_name in ${own_line#*:}; do
      own_reached "$own_list_name" || { [ "$got" != PASS ] || got=no-case-$own_list_name; }
      if ! own_gate "$own_list_name" windows; then
        [ "$got" != PASS ] || got=on-both-lists-$own_list_name
      elif [ "$own_host" != windows ]; then
        own_list_held=yes
      else
        case "$own_skipped$nl" in
          *"$nl$own_list_name$nl"*) ;;
          *) ! own_reached "$own_list_name" || own_list_held=yes ;;
        esac
      fi
    done
    [ -n "$own_list_held" ] || { [ "$got" != PASS ] || got=no-windows-case-for-$own_list_rule; }
  done <<EOF
$own_windows_rules
EOF
  printf 'case run-ci-d/%s windows form: %s cases not-run (macos alone), %s rules each with a case that runs\n' "$case_name" "$own_list_n" "$own_list_r"
  printf 'case run-ci-d/%s want=PASS got=%s\n' "$case_name" "$got"
  own_clock
  [ "$got" = PASS ] || wrong="$wrong $case_name"
  case_reset
}

case_reset() {
  case_ci=
  case_scripts=
  case_cd=
  case_locale=
  case_tmpdir=
  case_jobs=
  case_check_jobs=
  case_limit=
  case_first=
  case_then=
  case_max_s=
  case_nested=
  case_switch=
  case_switch_set=
  case_worker=
  case_path=
  case_signal=
  case_wait=
  case_absent=
  case_signal_min=
  case_gone=
  case_empty=
  case_procs=
  case_made=
  case_once=
  case_witness=
  case_env=
  case_git_home=
  case_cdpath=
  case_index_same=
  case_git_count=
  case_git_names=
  case_cargo_home=
}

# shellcheck disable=SC2016
own_self_test() {
  own_n=0
  own_start=0
  [ -n "$nested_word" ] || own_start=$(date +%s)
  own_mark=$own_start
  own_host=macos
  if command -v cygpath >/dev/null 2>&1; then
    own_host=windows
  fi
  own_seen=
  own_skipped=
  own_macos_alone="
two-alone-checks-run-one-after-the-other-in-order:lanes
alone-check-starts-after-both-lanes-have-ended:lanes
lane-line-at-line-11-is-no-lane:lanes
step-ci.d-one-script-at-a-time-whatever-ci-d-self-test-jobs-says:lanes
checks-without-a-lane-one-at-a-time-beside-the-free-lane:lanes
alone-list-line-with-leading-blanks:lanes
alone-list-with-comment-and-blank-lines:lanes
free-lane-runs-ci-d-check-jobs-at-a-time:lanes
self-test-of-the-driver-alone:lanes
alone-list-names-a-script-of-the-other-job-alone:lanes
checks-of-two-lanes-run-at-the-same-time:lanes
output-of-an-alone-check-in-the-order-of-the-run:lanes
two-at-a-time:lanes
exit-0-without-pass-line:verdict
pass-line-with-a-tail:verdict
pass-line-with-exit-1:verdict
pass-line-with-exit-77:verdict
self-test-count-is-no-number:verdict
self-test-exits-2:verdict
self-test-fails:verdict
self-test-pass-line-with-exit-1:verdict
self-test-prints-nothing:verdict
self-test-rejects-two:verdict
self-test-rejects-nothing:verdict
failing-self-test-beside-another:verdict
self-tests-run-at-the-same-time:verdict
self-test-without-scripts:verdict
refusal-keeps-its-place-in-the-recap:recap
refusal-keeps-its-place-in-the-recap-of-the-step-ci.d:recap
skip-in-ci:skip
skip-outside-ci:skip
fail-does-not-stop-the-run:skip
"
  own_windows_rules="
path-forms-and-spellings-of-the-root:root-that-starts-with-two-slashes tracked-script-runs-and-untracked-script-is-refused-windows-form-root tracked-script-runs-and-untracked-script-is-refused-doubled-slashes tracked-script-runs-and-untracked-script-is-refused-dot-slash cdpath-of-the-caller-leaves-the-root
carriage-return-at-the-end-of-a-header-line-or-of-a-line-of-the-alone-list:lane-line-with-a-carriage-return-names-the-free-lane limit-line-with-a-carriage-return-raises-the-limit alone-list-line-with-a-carriage-return alone-line-with-a-carriage-return-names-the-alone-part
order-of-the-glob-and-of-sort:byte-order list-in-byte-order hidden-entry-after-the-others list-puts-hidden-entries-last outputs-kept-apart-in-name-order output-in-name-order-when-a-later-check-ends-first outputs-of-free-checks-kept-apart-in-name-order
process-groups-and-signals:step-interrupted-by-TERM step-ci.d-interrupted-by-INT script-that-ignores-term-past-its-limit self-test-past-its-limit default-limit-reached-at-1200-s child-that-a-script-leaves-is-ended script-that-sends-TERM-to-itself-in-the-step-ci.d-ends-as-in-the-foreground script-that-sends-QUIT-to-itself-in-the-step-ci.d-runs-on-as-in-the-foreground interrupt-of-the-own-cases-ends-the-running-case-s-driver-and-witness
perl-and-its-groups-under-msys:perl-does-not-run perl-does-not-run-in-the-step-ci.d child-in-a-group-of-its-own-is-named-at-the-next-read
retry-of-a-removal-and-the-mode-555-probe:tmpdir-removed-on-the-third-try-after-two-waits tmpdir-whose-first-removal-fails-is-removed-on-a-later-try tmpdir-whose-parent-the-script-made-read-only-is-removed
conversions-of-git-core.autocrlf-stat-data-and-the-read-of-the-index:file-of-an-autocrlf-checkout-rewritten-with-lf-before-the-first-script tracked-script-rewritten-with-crlf-before-the-first-script tracked-script-rewritten-with-crlf-by-a-script stat-data-written-into-the-index-leaves-the-read-of-the-driver index-of-the-checkout-with-stale-stat-data-is-the-same-after-a-run same-size-re-encoding-under-a-global-attributes-file-is-named
cargo-bin-directory-on-path-and-the-driver-s-own-git:git-in-the-cargo-bin-directory-does-not-answer-for-the-driver git-in-the-cargo-bin-directory-spelled-with-a-dot-does-not-answer-for-the-driver git-names-of-the-caller-reach-a-script-unchanged global-git-setting-of-a-script-leaves-the-read-of-the-driver
descriptor-9:descriptor-9-is-closed-for-a-script
count-of-processors:default-check-jobs-from-number-of-processors default-number-from-number-of-processors default-number-of-one-when-number-of-processors-is-none default-number-of-one-when-number-of-processors-is-x default-check-jobs-of-one-without-number-of-processors
file-system-that-folds-case:run-directory-renamed-by-case-alone-stays-refused
"
  good="# jobs: macos windows$nl# rule: A fixture rule."
  seven="#$nl#$nl#$nl#$nl#$nl#$nl#"
  pass_a='[ $# -eq 0 ] || exit 3; printf "PASS 10-a\n"'
  pass_b='printf "PASS 20-b\n"'
  fail_a='printf "FAIL 10-a (planned)\n"; exit 1'
  skip_a='printf "SKIP 10-a (no tool)\n"; exit 77'
  rejected=0
  wrong=
  fixture_added=
  case_reset
  perl_bin=$(command -v perl) || perl_bin=perl
  case_perl="setpgrp(0, 0); my \$f = delete \$ENV{RUN_CI_D_PID_FILE}; my \$m = delete \$ENV{RUN_CI_D_MARK}; my \$w = delete \$ENV{RUN_CI_D_WAIT}; if (defined \$f) { sleep \$w if \$w; open(my \$h, '>', \$f) or exit 127; print \$h \"\$\$\\n\"; close(\$h) or exit 127; if (defined \$m && -e \$m) { truncate(\$f, 0); exit 143; } } \$SIG{QUIT} = 'DEFAULT'; \$SIG{INT} = 'DEFAULT'; exec { \$ARGV[0] } @ARGV; exit 127;"
  worker_wrap='RUN_CI_D_WORKER=$$; export RUN_CI_D_WORKER; sh "$0" "$@"; exit "$?"'
  for tool in perl getconf; do
    mkdir -p "$tmp/no-$tool" && printf '#!/bin/sh\nexit 1\n' >"$tmp/no-$tool/$tool" && chmod +x "$tmp/no-$tool/$tool"
  done
  real_date=$(command -v date) || real_date='date'
  real_sleep=$(command -v sleep) || real_sleep='sleep'
  mkdir -p "$tmp/slow-sleep" &&
    printf '#!/bin/sh\nif [ "$1" = 0.1 ]; then exec %s 0.4; fi\nexec %s "$@"\n' "$real_sleep" "$real_sleep" >"$tmp/slow-sleep/sleep" &&
    chmod +x "$tmp/slow-sleep/sleep"
  mkdir -p "$tmp/late-date" &&
    printf '#!/bin/sh\nfor w in want-read.*; do [ ! -e "$w" ] || [ -e "read-done.${w#want-read.}" ] || : >"read-done.${w#want-read.}"; done\nif [ "$1" = +%%s ] && [ -r late ]; then\n  read -r late <late\n  now=$(%s +%%s) || exit 1\n  echo "$((now + late))"\nelse\n  exec %s "$@"\nfi\n' "$real_date" "$real_date" >"$tmp/late-date/date" &&
    chmod +x "$tmp/late-date/date"
  usage_line='usage: sh scripts/ci/run-ci-d.sh <job> [--self-test] [NAME...]'
  name_refusal='the file name is not NN-name.sh, two digits and a name of a-z, 0-9 and -'
  lane_refusal="its '# lane:' line is neither '# lane: free' nor '# lane: alone'; a check that names no lane runs in the cargo lane"
  untracked_refusal='git does not track it'
  guard_line='FAIL run-ci-d (Cargo.lock or package-lock.json differs from HEAD; a script builds with --locked and changes no tracked file)'
  perl_line='FAIL run-ci-d (perl does not run; the driver starts each script through perl, for INT and QUIT at their defaults and a process group of its own)'

  first_b=$pass_b
  [ "$nested_word" != hold ] || first_b='sleep 8; printf "PASS 20-b\n"'
  [ "$nested_word" != late ] || first_b='sleep 30; printf "PASS 20-b\n"'
  [ "$nested_word" != gap ] || first_b='sleep 30; printf "PASS 20-b\n"'
  [ "$nested_word" != wgap ] || first_b='sleep 30; printf "PASS 20-b\n"'
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 20-b.sh "$good" "$first_b"
  case $nested_word in
    hold | late | gap | wgap) case_witness=yes ;;
  esac
  own_case all-pass 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  if [ -n "$nested_word" ]; then
    [ "$nested_word" != fail ] || wrong="$wrong nested-run-asked-to-fail"
    [ "$nested_word" != slow ] || sleep 6
    if [ -n "$wrong" ]; then
      record FAIL "FAIL self-test:run-ci-d (wrong verdict in:$wrong)"
    else
      record PASS 'PASS self-test:run-ci-d (a nested run stops after its first case)'
    fi
    return
  fi
  own_case rules 0 '10-a (macos windows): A fixture rule.' --rules
  own_case list 0 'scripts/r1/ci.d/20-b.sh' --list macos
  own_case name-matches-no-script 1 'FAIL run-ci-d (no script 99-none for macos)' macos 99-none
  own_case one-name-matches-no-script 1 'FAIL run-ci-d (no script 99-none for macos)' macos 20-b 99-none
  own_case unknown-job 64 "$usage_line" linux
  own_case rules-with-an-argument 64 "$usage_line" --rules macos
  own_case list-with-two-arguments 64 "$usage_line" --list macos windows
  own_case list-for-an-unknown-job 64 "$usage_line" --list linux
  own_case name-with-a-space 64 "$usage_line" macos '10-a 20-b'
  own_case driver-name-in-a-run 1 'FAIL run-ci-d (no script run-ci-d for macos)' macos 20-b run-ci-d
  case_cd=..
  own_case outside-a-work-tree 1 'FAIL run-ci-d (start it at the root of the git work tree)' macos
  case_tmpdir=$tmp/missing
  own_case mktemp-fails 1 'FAIL run-ci-d (mktemp failed)' macos
  case_cd=scripts
  own_case below-the-root 1 'FAIL run-ci-d (start it at the root of the git work tree)' macos
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  own_case no-scripts 1 'FAIL run-ci-d (no scripts for macos)' macos

  new_repo && add_script r1 10-a.sh '# rule: A fixture rule.' "$pass_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case no-jobs-line 1 "FAIL 10-a (its first 10 lines hold no single '# jobs:' line)" macos
  own_case rules-refuses 1 "FAIL 10-a (its first 10 lines hold no single '# jobs:' line)" --rules
  new_repo && add_script r1 10-a.sh '# jobs: macos' "$pass_a"
  own_case no-rule-line 1 "FAIL 10-a (its first 10 lines hold no single '# rule:' line)" macos
  new_repo && add_script r1 10-a.sh "$seven$nl$good" "$pass_a"
  own_case header-ends-at-line-10 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$seven$nl#$nl$good" "$pass_a"
  own_case rule-at-line-11 1 "FAIL 10-a (its first 10 lines hold no single '# rule:' line)" macos
  new_repo && add_script r1 10-a.sh "$seven$nl#$nl# rule: A fixture rule.$nl# jobs: macos" "$pass_a"
  own_case jobs-at-line-11 1 "FAIL 10-a (its first 10 lines hold no single '# jobs:' line)" macos
  new_repo && add_script r1 10-a.sh "# jobs: macos linux$nl# rule: A fixture rule." "$pass_a"
  own_case unknown-job-word 1 "FAIL 10-a (its '# jobs:' line names linux; a job is macos or windows)" macos
  new_repo && add_script r1 format.sh "$good" 'printf "PASS format\n"'
  own_case file-name 1 "FAIL format ($name_refusal)" macos
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 20-nosuffix "$good" 'printf "PASS 20-nosuffix\n"'
  own_case file-name-without-a-suffix 1 "FAIL 20-nosuffix ($name_refusal)" macos
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 30-upper.SH "$good" 'printf "PASS 30-upper\n"'
  own_case file-name-with-an-upper-case-suffix 1 "FAIL 30-upper.SH ($name_refusal)" macos
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && mkdir "$fixture/scripts/r1/ci.d/40-dir.sh"
  own_case directory-with-a-script-name 1 'FAIL 40-dir (it is not a regular file)' macos
  own_case directory-fails-the-other-job 1 'FAIL 40-dir (it is not a regular file)' windows
  new_repo && add_script r1 10-a.sh "$good" "$pass_a"
  if ln -s no-such-file "$fixture/scripts/r1/ci.d/50-dangling.sh" 2>/dev/null && [ -L "$fixture/scripts/r1/ci.d/50-dangling.sh" ]; then
    own_case dangling-link-with-a-script-name 1 'FAIL 50-dangling (it is not a regular file)' macos
  else
    own_not_run dangling-link-with-a-script-name 'a link is a copy on this host'
  fi
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 .20-hidden.sh "$good" 'printf "PASS .20-hidden\n"'
  case_first="PASS 10-a${nl}FAIL .20-hidden ($name_refusal)"
  case_then='ci.d macos: 1 passed, 1 failed, 0 skipped'
  own_case hidden-entry-after-the-others 1 "FAIL .20-hidden ($name_refusal)" macos
  case_first='scripts/r1/ci.d/10-a.sh'
  case_then="FAIL .20-hidden ($name_refusal)"
  own_case list-puts-hidden-entries-last 1 "FAIL .20-hidden ($name_refusal)" --list macos
  new_repo && add_script r1 10-.sh "$good" 'printf "PASS 10-\n"'
  own_case file-name-without-a-name 1 "FAIL 10- ($name_refusal)" macos
  new_repo && add_script r1 10-A.sh "$good" 'printf "PASS 10-A\n"'
  own_case file-name-with-a-capital 1 "FAIL 10-A ($name_refusal)" macos
  new_repo && add_script r1 1-a.sh "$good" 'printf "PASS 1-a\n"'
  own_case file-name-with-one-digit 1 "FAIL 1-a ($name_refusal)" macos
  new_repo && add_script r1 10-a.sh "# jobs: macos${tab}windows$nl# rule: A fixture rule." "$pass_a"
  own_case jobs-line-with-a-tab 1 "FAIL 10-a (its '# jobs:' line names something other than macos and windows)" macos
  new_repo && add_script r1 10-a.sh "$good$nl# rule: A second rule." "$pass_a"
  own_case two-rule-lines 1 "FAIL 10-a (its first 10 lines hold no single '# rule:' line)" macos
  new_repo && add_script r1 10-a.sh "# jobs: macos$nl$good" "$pass_a"
  own_case two-jobs-lines 1 "FAIL 10-a (its first 10 lines hold no single '# jobs:' line)" macos
  new_repo && add_script r1 10-a.sh "$good$nl# lane: cargo" "$pass_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case lane-other-than-free 1 "FAIL 10-a ($lane_refusal)" macos
  new_repo && add_script r1 10-a.sh "$good$nl# lane: free$nl# lane: free" "$pass_a"
  own_case two-lane-lines 1 "FAIL 10-a (its first 10 lines hold more than one '# lane:' line)" macos
  new_repo && add_script r1 10-a.sh "$good$nl# lane:" "$pass_a"
  own_case rules-refuses-a-lane-line 1 "FAIL 10-a ($lane_refusal)" --rules
  new_repo && add_script r1 10-a.sh "$good$nl# lane: free" "$pass_a"
  own_case lane-free-is-no-refusal 0 '10-a (macos windows): A fixture rule.' --rules
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r2 10-a.sh "$good" "$pass_a"
  own_case name-used-twice 1 'FAIL 10-a (a second script has this name: scripts/r2/ci.d/10-a.sh)' macos
  own_case rules-name-used-twice 1 'FAIL 10-a (a second script has this name: scripts/r2/ci.d/10-a.sh)' --rules
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" &&
    printf '#!/bin/sh\n%s\n%s\n' "$good" 'printf "PASS 20-planted\n"' >"$fixture/scripts/r1/ci.d/20-planted.sh"
  case_absent='PASS 20-planted'
  case_witness=yes
  own_case untracked-script-under-a-ci.d-directory 1 "FAIL 20-planted ($untracked_refusal)" macos
  own_case rules-refuses-an-untracked-script 1 "FAIL 20-planted ($untracked_refusal)" --rules
  plant='printf "PASS 20-planted\n"'
  for spelling in dot-slash trailing-slash absolute doubled-slashes outside-ascii; do
    run_dir=r1
    [ "$spelling" != outside-ascii ] || run_dir=$(printf 'r\303\251')
    new_repo && add_script "$run_dir" 10-a.sh "$good" "$pass_a" &&
      printf '#!/bin/sh\n%s\n%s\n' "$good" "$plant" >"$fixture/scripts/$run_dir/ci.d/20-planted.sh"
    case $spelling in
      dot-slash) case_scripts=./scripts ;;
      trailing-slash) case_scripts=scripts/ ;;
      absolute) case_scripts=$fixture/scripts ;;
      doubled-slashes) case_scripts=$(printf '%s\n' "$fixture/scripts/" | sed 's|\([^/]\)/|\1//|g') ;;
    esac
    case_witness=yes
    case_absent="FAIL 10-a ($untracked_refusal)"
    case_first='PASS 10-a'
    case_then="FAIL 20-planted ($untracked_refusal)"
    own_case "tracked-script-runs-and-untracked-script-is-refused-$spelling" 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  done
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" &&
    printf '#!/bin/sh\n%s\n%s\n' "$good" "$plant" >"$fixture/scripts/r1/ci.d/20-planted.sh"
  case_scripts=/$fixture/scripts
  case_witness=yes
  case_absent='PASS 20-planted'
  if command -v cygpath >/dev/null 2>&1; then
    own_case root-that-starts-with-two-slashes 1 'FAIL run-ci-d (no scripts for macos)' macos
  else
    case_first='PASS 10-a'
    case_then="FAIL 20-planted ($untracked_refusal)"
    own_case root-that-starts-with-two-slashes 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  fi
  e1_ok='ci.d macos: 1 passed, 1 failed, 0 skipped'
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" &&
    printf '#!/bin/sh\n%s\n%s\n' "$good" "$plant" >"$fixture/scripts/r1/ci.d/20-planted.sh"
  e1_link=$(mktemp -d "$tmp/root-link.XXXXXX") && ln -s "$fixture" "$e1_link/to" 2>/dev/null
  if [ -L "$e1_link/to" ]; then
    case_scripts=$e1_link/to/scripts
    case_witness=yes
    case_absent="FAIL 10-a ($untracked_refusal)"
    case_first='PASS 10-a'
    case_then="FAIL 20-planted ($untracked_refusal)"
    own_case tracked-script-runs-and-untracked-script-is-refused-root-through-a-link 1 "$e1_ok" macos
  else
    own_not_run tracked-script-runs-and-untracked-script-is-refused-root-through-a-link 'a link is a copy on this host'
  fi
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" &&
    printf '#!/bin/sh\n%s\n%s\n' "$good" "$plant" >"$fixture/scripts/r1/ci.d/20-planted.sh"
  e1_real=$fixture
  e1_link=$(mktemp -d "$tmp/cwd-link.XXXXXX") && ln -s "$e1_real" "$e1_link/to" 2>/dev/null
  if [ -L "$e1_link/to" ]; then
    fixture=$e1_link/to
    case_scripts=$e1_real/scripts
    case_witness=yes
    case_absent="FAIL 10-a ($untracked_refusal)"
    case_first='PASS 10-a'
    case_then="FAIL 20-planted ($untracked_refusal)"
    own_case tracked-script-runs-and-untracked-script-is-refused-working-directory-through-a-link 1 "$e1_ok" macos
    fixture=$e1_real
  else
    own_not_run tracked-script-runs-and-untracked-script-is-refused-working-directory-through-a-link 'a link is a copy on this host'
  fi
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" &&
    printf '#!/bin/sh\n%s\n%s\n' "$good" "$plant" >"$fixture/scripts/r1/ci.d/20-planted.sh"
  case_scripts=$fixture/scripts/.
  case_witness=yes
  case_absent="FAIL 10-a ($untracked_refusal)"
  case_first='PASS 10-a'
  case_then="FAIL 20-planted ($untracked_refusal)"
  own_case tracked-script-runs-and-untracked-script-is-refused-root-that-ends-in-a-dot 1 "$e1_ok" macos
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && fixture_commit &&
    mkdir -p "$fixture/deep/sub" "$fixture/deep/scripts/r1/ci.d" &&
    printf '#!/bin/sh\n%s\n%s\n' "$good" "$plant" >"$fixture/deep/scripts/r1/ci.d/10-a.sh" &&
    ln -s deep/sub "$fixture/x" 2>/dev/null
  if [ -L "$fixture/x" ]; then
    case_scripts=x/../scripts
    case_witness=yes
    case_absent='PASS 20-planted'
    own_case untracked-script-under-a-tracked-name-through-a-link-and-dot-dot-is-refused 1 "FAIL 10-a ($untracked_refusal)" macos
  else
    own_not_run untracked-script-under-a-tracked-name-through-a-link-and-dot-dot-is-refused 'a link is a copy on this host'
  fi
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && fixture_commit && mkdir -p "$fixture/alt/scripts"
  case_cdpath=$fixture/alt
  case_scripts=scripts
  case_witness=yes
  own_case cdpath-of-the-caller-leaves-the-root 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  if command -v cygpath >/dev/null 2>&1; then
    new_repo && add_script r1 10-a.sh "$good" "$pass_a" &&
      printf '#!/bin/sh\n%s\n%s\n' "$good" "$plant" >"$fixture/scripts/r1/ci.d/20-planted.sh"
    case_scripts=$(cygpath -m "$fixture/scripts")
    case_witness=yes
    case_absent="FAIL 10-a ($untracked_refusal)"
    case_first='PASS 10-a'
    case_then="FAIL 20-planted ($untracked_refusal)"
    own_case tracked-script-runs-and-untracked-script-is-refused-windows-form-root 1 "$e1_ok" macos
  else
    own_not_run tracked-script-runs-and-untracked-script-is-refused-windows-form-root 'no cygpath on this host'
  fi
  : >"$tmp/case-probe"
  if [ -e "$tmp/CASE-PROBE" ]; then
    new_repo && add_script r1 10-a.sh "$good" "$pass_a" && fixture_commit &&
      mv "$fixture/scripts/r1" "$fixture/scripts/R1x" && mv "$fixture/scripts/R1x" "$fixture/scripts/R1"
    case_witness=yes
    own_case run-directory-renamed-by-case-alone-stays-refused 1 "FAIL 10-a ($untracked_refusal)" macos
  else
    own_not_run run-directory-renamed-by-case-alone-stays-refused 'the file system keeps letter case apart'
  fi

  if own_gate pass-line-with-exit-1; then
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS 10-a\n"; exit 1'
  own_case pass-line-with-exit-1 1 'FAIL 10-a (exit 1)' macos
  else
    own_not_run pass-line-with-exit-1 "macos alone: $own_gate_reason"
  fi
  if own_gate exit-0-without-pass-line; then
  new_repo && add_script r1 10-a.sh "$good" 'printf "done\n"'
  own_case exit-0-without-pass-line 1 'FAIL 10-a (exit 0)' macos
  else
    own_not_run exit-0-without-pass-line "macos alone: $own_gate_reason"
  fi
  if own_gate pass-line-with-a-tail; then
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS 10-a (3 checks)\n"'
  own_case pass-line-with-a-tail 1 'FAIL 10-a (exit 0)' macos
  else
    own_not_run pass-line-with-a-tail "macos alone: $own_gate_reason"
  fi
  if own_gate pass-line-with-exit-77; then
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS 10-a\n"; exit 77'
  own_case pass-line-with-exit-77 1 'FAIL 10-a (exit 77)' macos
  else
    own_not_run pass-line-with-exit-77 "macos alone: $own_gate_reason"
  fi
  new_repo && add_script r1 10-a.sh "$good" 'if read -r line; then printf "FAIL 10-a (it read a line)\n"; exit 1; fi; printf "PASS 10-a\n"'
  own_case no-standard-input 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" '[ "$CI_D_JOB" = windows ] || { printf "FAIL 10-a (job)\n"; exit 1; }; printf "PASS 10-a\n"'
  own_case job-handed-to-the-script 0 'ci.d windows: 1 passed, 0 failed, 0 skipped' windows
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && printf 'a file\n' >"$fixture/target"
  own_case artifacts-directory-cannot-be-made 1 'FAIL 10-a (its artifacts directory cannot be made)' macos
  if own_gate fail-does-not-stop-the-run; then
  new_repo && add_script r1 10-a.sh "$good" "$fail_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case fail-does-not-stop-the-run 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  else
    own_not_run fail-does-not-stop-the-run "macos alone: $own_gate_reason"
  fi
  own_case name-selects-one 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos 20-b
  if own_gate skip-outside-ci; then
  new_repo && add_script r1 10-a.sh "$good" "$skip_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case skip-outside-ci 0 'ci.d macos: 1 passed, 0 failed, 1 skipped' macos
  else
    own_not_run skip-outside-ci "macos alone: $own_gate_reason"
  fi
  if own_gate skip-in-ci; then
  case_ci=true
  own_case skip-in-ci 1 'FAIL 10-a (skipped in CI: no tool)' macos
  else
    own_not_run skip-in-ci "macos alone: $own_gate_reason"
  fi
  new_repo && add_script r1 10-a.sh "$good" "$skip_a"
  own_case every-script-skipped 1 'ci.d macos: 0 passed, 0 failed, 1 skipped' macos
  new_repo && add_script r1 10-a.sh "# jobs: windows$nl# rule: A fixture rule." "$pass_a"
  own_case other-job-only 1 'FAIL run-ci-d (no scripts for macos)' macos
  own_case job-named 0 'ci.d windows: 1 passed, 0 failed, 0 skipped' windows
  own_case list-other-job-only 1 'FAIL run-ci-d (no scripts for macos)' --list macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/Cargo.lock"; printf "PASS 10-a\n"'
  own_case lockfile-changed 1 "$guard_line" macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/package-lock.json"; printf "PASS 10-a\n"' &&
    printf '{}\n' >"$fixture/package-lock.json" && git -C "$fixture" add package-lock.json >/dev/null 2>&1 && fixture_commit
  own_case package-lock-changed 1 "$guard_line" macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/Cargo.lock"; git -C "$CI_D_REPO" add Cargo.lock; printf "PASS 10-a\n"'
  own_case lockfile-changed-and-staged 1 "$guard_line" macos
  new_repo && add_script r1 10-a.sh "$good" 'git -C "$CI_D_REPO" update-index --assume-unchanged Cargo.lock; printf "x\n" >>"$CI_D_REPO/Cargo.lock"; printf "PASS 10-a\n"'
  own_case lockfile-changed-under-assume-unchanged 1 "$guard_line" macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/tracked.txt"; printf "PASS 10-a\n"' &&
    add_script r1 20-b.sh "$good" "$pass_b"
  case_absent='FAIL run-ci-d (20-b changed a tracked file: tracked.txt)'
  own_case script-changes-a-tracked-file 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 05-x.sh "$good" 'printf "PASS 05-x\n"' &&
    add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/tracked.txt"; sleep 1; for f in "$TMPDIR"/../* "$TMPDIR"/../../*; do [ ! -f "$f" ] || : >"$f"; done; printf "PASS 10-a\n"'
  case_tmpdir=$(mktemp -d "$tmp/own-tmp.XXXXXX") || case_tmpdir=$tmp/no-own-tmp
  case_witness=yes
  case_absent='FAIL run-ci-d (10-a moved HEAD)'
  case_first="PASS 05-x${nl}PASS 10-a${nl}FAIL run-ci-d (10-a changed a tracked file: tracked.txt)"
  case_then='ci.d macos: 2 passed, 1 failed, 0 skipped'
  own_case script-that-empties-the-files-beside-its-tmpdir-leaves-the-baseline 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'd=$(mktemp -d "$TMPDIR/x.XXXXXX") && printf "x\n" >"$d/f" && printf "x\n" >"$CI_D_ARTIFACTS/f" && printf "PASS 10-a\n"'
  own_case script-writes-only-under-its-tmpdir-and-artifacts 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'cp "$CI_D_REPO/tracked.txt" "$CI_D_REPO/kept"; printf "x\n" >>"$CI_D_REPO/tracked.txt"; git -C "$CI_D_REPO" add tracked.txt; cp "$CI_D_REPO/kept" "$CI_D_REPO/tracked.txt"; printf "PASS 10-a\n"'
  own_case script-stages-a-change-and-restores-the-file 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  for bit in assume-unchanged skip-worktree; do
    new_repo && track_file && add_script r1 10-a.sh "$good" "git -C \"\$CI_D_REPO\" update-index --$bit tracked.txt; printf 'x\\n' >>\"\$CI_D_REPO/tracked.txt\"; printf 'PASS 10-a\\n'"
    case_once='FAIL run-ci-d (10-a changed a tracked file: tracked.txt)'
    case_witness=yes
    own_case "script-changes-a-tracked-file-under-$bit" 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  done
  new_repo && track_file && add_script r1 10-a.sh "$good" 'r=$CI_D_REPO; printf "x\n" >>"$r/tracked.txt"; git -C "$r" add tracked.txt; t=$(git -C "$r" write-tree); p=$(git -C "$r" rev-parse HEAD); c=$(printf "tree %s\nparent %s\nauthor f <f@example.invalid> 1 +0000\ncommitter f <f@example.invalid> 1 +0000\n\nf\n" "$t" "$p" | git -C "$r" hash-object -t commit -w --stdin); git -C "$r" update-ref HEAD "$c"; printf "PASS 10-a\n"'
  own_case script-commits-a-change 1 'FAIL run-ci-d (10-a moved HEAD)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" "$pass_a" && printf 'x\n' >>"$fixture/tracked.txt"
  case_absent='FAIL run-ci-d (10-a changed a tracked file: tracked.txt)'
  own_case tracked-file-differs-before-the-first-script 1 'FAIL run-ci-d (a tracked file differs from HEAD before the first script: tracked.txt)' macos
  tool_line='FAIL run-ci-d (a tracked file differs from HEAD before the first script: tool.sh)'
  new_repo && printf '*.sh text eol=lf\n' >"$fixture/.gitattributes" && printf 'echo hi\n' >"$fixture/tool.sh" &&
    git -C "$fixture" add .gitattributes tool.sh >/dev/null 2>&1 && add_script r1 10-a.sh "$good" "$pass_a" && fixture_commit &&
    printf 'echo hi\r\n' >"$fixture/tool.sh"
  case_witness=yes
  case_absent='FAIL run-ci-d (10-a changed a tracked file: tool.sh)'
  own_case tracked-script-rewritten-with-crlf-before-the-first-script 1 "$tool_line" macos
  new_repo && printf '*.sh text eol=lf\n' >"$fixture/.gitattributes" && printf 'echo hi\n' >"$fixture/tool.sh" &&
    git -C "$fixture" add .gitattributes tool.sh >/dev/null 2>&1 &&
    add_script r1 10-a.sh "$good" 'printf "echo hi\r\n" >"$CI_D_REPO/tool.sh"; printf "PASS 10-a\n"' && fixture_commit
  case_witness=yes
  own_case tracked-script-rewritten-with-crlf-by-a-script 1 'FAIL run-ci-d (10-a changed a tracked file: tool.sh)' macos
  late_crlf='d=$(cd "$TMPDIR/../.." && pwd); perl -e "setpgrp(0, 0); exec @ARGV" sh -c '"'"'n=0; while [ -d "$1" ] && [ "$n" -lt 100 ]; do sleep 0.1; n=$((n + 1)); done; printf "echo hi\r\n" >"$0/tool.sh"; : >"$0/rewritten"'"'"' "$CI_D_REPO" "$d" >/dev/null 2>&1 & printf "%s\n" "$!" >>"$CI_D_REPO/pids"'
  new_repo && printf '*.sh text eol=lf\n' >"$fixture/.gitattributes" && printf 'echo hi\n' >"$fixture/tool.sh" &&
    git -C "$fixture" add .gitattributes tool.sh >/dev/null 2>&1 &&
    add_script r1 10-a.sh "$good" "if [ \"\${1:-}\" = --self-test ]; then $late_crlf; printf 'PASS self-test:10-a (1 failing fixtures rejected)\\n'; else printf 'PASS 10-a\\n'; fi" && fixture_commit
  case_witness=yes
  own_case self-test-that-leaves-a-writer-of-the-checkout 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test 10-a
  n=0
  while [ ! -e "$fixture/rewritten" ] && [ "$n" -lt 100 ]; do
    sleep 0.1
    n=$((n + 1))
  done
  [ ! -f "$fixture/pids" ] || true >"$fixture/pids"
  case_witness=yes
  case_absent='FAIL run-ci-d (10-a changed a tracked file: tool.sh)'
  own_case writer-left-by-the-self-test-step-is-named-before-the-step-ci.d 1 "$tool_line" macos
  new_repo && git -C "$fixture" config core.autocrlf true && printf 'line 1\r\n' >"$fixture/tool.txt" &&
    git -C "$fixture" add tool.txt >/dev/null 2>&1 && add_script r1 10-a.sh "$good" "$pass_a" && fixture_commit &&
    printf 'line 1\n' >"$fixture/tool.txt"
  case_witness=yes
  own_case file-of-an-autocrlf-checkout-rewritten-with-lf-before-the-first-script 1 'FAIL run-ci-d (a tracked file differs from HEAD before the first script: tool.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'git -C "$CI_D_REPO" checkout -q -- tracked.txt; printf "PASS 10-a\n"' && printf 'x\n' >>"$fixture/tracked.txt"
  own_case script-restores-a-file-that-differed-before 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'git -C "$CI_D_REPO" config core.checkStat minimal; printf "PASS 10-a\n"'
  own_case script-changes-the-git-settings 1 'FAIL run-ci-d (10-a changed the git settings of the checkout: config)' macos
  settings_line='FAIL run-ci-d (10-a changed the git settings of the checkout: config)'
  stat_edit='sleep 1 && cp -p "$CI_D_REPO/tracked.txt" "$CI_D_ARTIFACTS/kept" && printf "line 2\n" >"$CI_D_REPO/tracked.txt" && touch -r "$CI_D_ARTIFACTS/kept" "$CI_D_REPO/tracked.txt"'
  if ! command -v cygpath >/dev/null 2>&1; then
    included=$(mktemp "$tmp/included.XXXXXX") && printf '[http "https://example.invalid/"]\n\textraheader = none\n' >"$included"
    new_repo && printf 'line 1\n' >"$fixture/tracked.txt" && touch -t 200001010000 "$fixture/tracked.txt" &&
      git -C "$fixture" add tracked.txt >/dev/null 2>&1 && git -C "$fixture" config include.path "$included" &&
      add_script r1 10-a.sh "$good" "i=\$(git -C \"\$CI_D_REPO\" config --local --get include.path) && printf '[core]\\n\\tcheckStat = minimal\\n' >>\"\$i\" && $stat_edit; printf 'PASS 10-a\\n'" && fixture_commit
    case_witness=yes
    own_case setting-written-through-an-included-file-is-named 1 "$settings_line" macos
    new_repo && printf 'line 1\n' >"$fixture/tracked.txt" && touch -t 200001010000 "$fixture/tracked.txt" &&
      git -C "$fixture" add tracked.txt >/dev/null 2>&1 && git -C "$fixture" config extensions.worktreeConfig true &&
      add_script r1 10-a.sh "$good" "w=\$(git -C \"\$CI_D_REPO\" rev-parse --git-path config.worktree) && printf '[core]\\n\\tcheckStat = minimal\\n' >>\"\$w\" && $stat_edit; printf 'PASS 10-a\\n'" && fixture_commit
    case_witness=yes
    own_case setting-written-through-config-worktree-is-named 1 "$settings_line" macos
  else
    own_not_run setting-written-through-an-included-file-is-named 'git under cygpath reads no inode'
    own_not_run setting-written-through-config-worktree-is-named 'git under cygpath reads no inode'
  fi
  new_repo && printf '\303\251x\n' >"$fixture/tracked.txt" && git -C "$fixture" add tracked.txt >/dev/null 2>&1 &&
    add_script r1 10-a.sh "$good" "[ \"\${XDG_CONFIG_HOME:-}\" = \"\$HOME/config\" ] || { printf 'FAIL 10-a (no home of the case)\\n'; exit 1; }; mkdir -p \"\$XDG_CONFIG_HOME/git\" && printf 'tracked.txt text working-tree-encoding=ISO-8859-1\\n' >\"\$XDG_CONFIG_HOME/git/attributes\" && sleep 1 && printf '\\351x\\r\\n' >\"\$CI_D_REPO/tracked.txt\"; printf 'PASS 10-a\\n'" && fixture_commit
  case_git_home=yes
  case_witness=yes
  own_case same-size-re-encoding-under-a-global-attributes-file-is-named 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'a=$(git -C "$CI_D_REPO" rev-parse --git-path info/attributes) && mkdir -p "${a%/*}" && printf "* -text\n" >>"$a"; printf "PASS 10-a\n"' &&
    add_script r1 20-b.sh "$good" "$pass_b"
  case_absent='FAIL run-ci-d (20-b changed the git settings of the checkout: info/attributes)'
  own_case script-changes-the-git-attributes 1 'FAIL run-ci-d (10-a changed the git settings of the checkout: info/attributes)' macos
  real_git=$(command -v git) || real_git=git
  for git_read in 'ls-files -v' 'status --porcelain'; do
    mkdir -p "$tmp/git-fails-after-10-a" &&
      printf '#!/bin/sh\ncase "$*" in *"%s"*) [ ! -e failing-git ] || exit 128 ;; esac\nexec %s "$@"\n' "$git_read" "$real_git" >"$tmp/git-fails-after-10-a/git" &&
      chmod +x "$tmp/git-fails-after-10-a/git"
    new_repo && add_script r1 10-a.sh "$good" ': >"$CI_D_REPO/failing-git"; printf "PASS 10-a\n"' && add_script r1 20-b.sh "$good" "$pass_b"
    case_path=$tmp/git-fails-after-10-a
    own_case "git-${git_read%% *}-fails-at-the-read-after-a-script" 1 'FAIL run-ci-d (git cannot read the tracked files after 10-a)' macos
  done
  same_filter='cp "$CI_D_REPO/tracked.txt" "$CI_D_ARTIFACTS/kept" && printf "tracked.txt filter=same\n" >"$CI_D_ARTIFACTS/attributes"'
  new_repo && track_file && add_script r1 10-a.sh "$good" "[ \"\${GIT_CONFIG_GLOBAL:-}\" = \"\$HOME/gitconfig\" ] || { printf 'FAIL 10-a (no home of the case)\\n'; exit 1; }; $same_filter && git config --global core.attributesFile \"\$CI_D_ARTIFACTS/attributes\" && git config --global filter.same.clean \"cat '\$CI_D_ARTIFACTS/kept'\" && printf 'line 2\\n' >\"\$CI_D_REPO/tracked.txt\"; printf 'PASS 10-a\\n'"
  case_git_home=yes
  case_witness=yes
  own_case global-git-setting-of-a-script-leaves-the-read-of-the-driver 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  if ! command -v cygpath >/dev/null 2>&1; then
    new_repo && printf 'line 1\n' >"$fixture/tracked.txt" && touch -t 200001010000 "$fixture/tracked.txt" &&
      git -C "$fixture" add tracked.txt >/dev/null 2>&1 && fixture_commit &&
      add_script r1 10-a.sh "$good" "[ \"\${GIT_CONFIG_GLOBAL:-}\" = \"\$HOME/gitconfig\" ] || { printf 'FAIL 10-a (no home of the case)\\n'; exit 1; }; git config --global core.checkStat minimal && sleep 1 && cp -p \"\$CI_D_REPO/tracked.txt\" \"\$CI_D_ARTIFACTS/kept\" && printf 'line 2\\n' >\"\$CI_D_REPO/tracked.txt\" && touch -r \"\$CI_D_ARTIFACTS/kept\" \"\$CI_D_REPO/tracked.txt\"; printf 'PASS 10-a\\n'"
    case_git_home=yes
    case_witness=yes
    own_case global-check-stat-of-a-script-leaves-the-read-of-the-driver 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  else
    own_not_run global-check-stat-of-a-script-leaves-the-read-of-the-driver 'git under cygpath reads no inode'
  fi
  new_repo && track_file && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/tracked.txt"; printf "PASS 10-a\n"'
  case_cargo_home=$(mktemp -d "$tmp/cargo-home.XXXXXX") && mkdir "$case_cargo_home/bin" &&
    printf '#!/bin/sh\ncase "$*" in *"status --porcelain"* | *diff-index*) exit 0 ;; esac\nexec %s "$@"\n' "$real_git" >"$case_cargo_home/bin/git" &&
    chmod +x "$case_cargo_home/bin/git" || wrong="$wrong stand-in-git-not-made"
  case_path=$case_cargo_home/bin
  case_witness=yes
  stand_in_cargo=$case_cargo_home
  own_case git-in-the-cargo-bin-directory-does-not-answer-for-the-driver 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/tracked.txt"; printf "PASS 10-a\n"'
  case_cargo_home=$stand_in_cargo/
  case_path=$stand_in_cargo/bin
  case_witness=yes
  own_case git-in-the-cargo-bin-directory-named-with-a-slash-does-not-answer-for-the-driver 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/tracked.txt"; printf "PASS 10-a\n"'
  case_cargo_home=$stand_in_cargo
  case_path=$stand_in_cargo/./bin
  case_witness=yes
  own_case git-in-the-cargo-bin-directory-spelled-with-a-dot-does-not-answer-for-the-driver 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" "[ -z \"\${GIT_CONFIG_GLOBAL+set}\" ] && [ \"\${XDG_CONFIG_HOME:-}\" = \"\$HOME/config\" ] || { printf 'FAIL 10-a (no home of the case)\\n'; exit 1; }; git config --global ci-d.case seen && printf 'PASS 10-a\\n'"
  case_git_home=plain
  case_witness=yes
  own_case global-setting-of-a-script-leaves-the-settings-that-the-driver-reads 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo_by_git && track_file && add_script r1 10-a.sh "$good" "$pass_a" && fixture_commit && touch -t 200001010000 "$fixture/tracked.txt"
  case_index_same=yes
  case_witness=yes
  own_case index-of-the-checkout-with-stale-stat-data-is-the-same-after-a-run 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  if ! command -v cygpath >/dev/null 2>&1; then
    new_repo && printf 'line 1\n' >"$fixture/tracked.txt" && touch -t 200001010000 "$fixture/tracked.txt" &&
      git -C "$fixture" add tracked.txt >/dev/null 2>&1 && fixture_commit &&
      add_script r1 10-a.sh "$good" "[ \"\${GIT_CONFIG_COUNT:-}\" = 1 ] || { printf 'FAIL 10-a (no setting of the caller)\\n'; exit 1; }; sleep 1 && cp -p \"\$CI_D_REPO/tracked.txt\" \"\$CI_D_ARTIFACTS/kept\" && printf 'line 2\\n' >\"\$CI_D_REPO/tracked.txt\" && touch -r \"\$CI_D_ARTIFACTS/kept\" \"\$CI_D_REPO/tracked.txt\"; printf 'PASS 10-a\\n'"
    case_git_count=yes
    case_witness=yes
    own_case git-setting-of-the-caller-leaves-the-read-of-the-driver 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  else
    own_not_run git-setting-of-the-caller-leaves-the-read-of-the-driver 'git under cygpath reads no inode'
  fi
  new_repo && add_script r1 10-a.sh "$good" '[ "${GIT_AUTHOR_NAME:-}" = "A Caller" ] && [ "${GIT_CASE_VALUE:-}" = "two  words, and a tab	here" ] && [ "${GIT_INDEX_FILE##*/no-index-of-the-caller/}" = index ] || { printf "FAIL 10-a (the GIT_ names of the caller changed)\n"; exit 1; }; printf "PASS 10-a\n"'
  case_git_names=yes
  case_witness=yes
  own_case git-names-of-the-caller-reach-a-script-unchanged 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" "$same_filter && printf 'line 2\\n' >\"\$CI_D_REPO/tracked.txt\" && touch -t 200001010000 \"\$CI_D_REPO/tracked.txt\" && git -C \"\$CI_D_REPO\" -c core.attributesFile=\"\$CI_D_ARTIFACTS/attributes\" -c filter.same.clean=\"cat '\$CI_D_ARTIFACTS/kept'\" update-index -q --refresh; printf 'PASS 10-a\\n'"
  case_witness=yes
  own_case stat-data-written-into-the-index-leaves-the-read-of-the-driver 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && add_script r1 10-a.sh "$good" 'r=$CI_D_REPO; p=$(git -C "$r" rev-parse HEAD); t=$(git -C "$r" write-tree); c=$(printf "tree %s\nauthor f <f@example.invalid> 2 +0000\ncommitter f <f@example.invalid> 2 +0000\n\nf\n" "$t" | git -C "$r" hash-object -t commit -w --stdin); git -C "$r" update-ref "refs/replace/$p" "$c"; printf "PASS 10-a\n"' && track_file
  own_case script-adds-a-replace-ref 1 "FAIL run-ci-d (10-a changed the git settings of the checkout: refs/replace/$fixture_head)" macos
  new_repo && track_file && add_script r1 10-a.sh "$good" "$pass_a" && git -C "$fixture" update-index --skip-worktree tracked.txt
  own_case tracked-file-marked-skip-worktree-before-the-first-script 1 'FAIL run-ci-d (a tracked file is marked assume-unchanged or skip-worktree before the first script: tracked.txt)' macos
  new_repo && track_file &&
    add_script r1 10-a.sh "$good" '( n=0; while [ ! -e "$CI_D_REPO/started.20-b" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; mkdir "$TMPDIR/late"; printf "x\n" >>"$CI_D_REPO/tracked.txt" ) >/dev/null 2>&1 & printf "%s\n" "$!" >>"$CI_D_REPO/pids"; printf "%s\n" "$TMPDIR" >>"$CI_D_REPO/tmpdirs"; printf "PASS 10-a\n"' &&
    add_script r1 20-b.sh "$good" ': >"$CI_D_REPO/started.20-b"; sleep 3; printf "PASS 20-b\n"'
  case_gone=1
  own_case child-that-a-script-leaves-is-ended 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" '( trap "sleep 1; : >\"\$CI_D_REPO/term-done\"; exit 0" TERM; sleep 60 & wait ) & printf "%s\n" "$!" >>"$CI_D_REPO/pids"; trap "" TERM; sleep 60 & printf "%s\n%s\n" "$$" "$!" >>"$CI_D_REPO/pids"; wait; printf "PASS 10-a\n"'
  case_limit=2
  case_max_s=30
  case_made=term-done
  case_witness=yes
  own_case child-whose-term-trap-takes-a-second-finishes-it-before-kill 1 'FAIL 10-a (time limit 2 s)' macos
  new_repo && track_file &&
    add_script r1 10-a.sh "$good" '( trap "printf \"x\\n\" >>\"\$CI_D_REPO/tracked.txt\"; exit 0" TERM; sleep 60 & wait ) >/dev/null 2>&1 & printf "%s\n" "$!" >>"$CI_D_REPO/pids"; printf "PASS 10-a\n"' &&
    add_script r1 20-b.sh "$good" "$pass_b"
  case_absent='FAIL run-ci-d (20-b changed a tracked file: tracked.txt)'
  case_witness=yes
  own_case group-ended-before-the-tracked-files-are-read 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" 'printf "x\n" >>"$CI_D_REPO/tracked.txt"; printf "PASS 10-a\n"' &&
    add_script r1 20-b.sh "$good" 'printf "line of 20-b\n"; printf "PASS 20-b\n"'
  case_first="line of 20-b${nl}PASS 20-b"
  case_then='ci.d macos: 2 passed, 1 failed, 0 skipped'
  case_absent='FAIL run-ci-d (20-b changed a tracked file: tracked.txt)'
  own_case later-scripts-run-after-a-change-of-a-tracked-file 1 'FAIL run-ci-d (10-a changed a tracked file: tracked.txt)' macos
  new_repo && track_file &&
    add_script r1 10-a.sh "$good" "perl -e 'setpgrp(0, 0); exec @ARGV' sh -c 'n=0; while [ ! -e \"\$0/started.20-b\" ] && [ \"\$n\" -lt 100 ]; do sleep 0.1; n=\$((n + 1)); done; printf \"x\\n\" >>\"\$0/tracked.txt\"; : >\"\$0/changed\"' \"\$CI_D_REPO\" >/dev/null 2>&1 & printf '%s\\n' \"\$!\" >>\"\$CI_D_REPO/pids\"; printf 'PASS 10-a\\n'" &&
    add_script r1 20-b.sh "$good" ': >"$CI_D_REPO/started.20-b"; n=0; while [ ! -e "$CI_D_REPO/changed" ] && [ "$n" -lt 100 ]; do sleep 0.1; n=$((n + 1)); done; printf "PASS 20-b\n"'
  case_witness=yes
  own_case child-in-a-group-of-its-own-is-named-at-the-next-read 1 'FAIL run-ci-d (20-b changed a tracked file: tracked.txt)' macos

  if own_gate self-test-rejects-two; then
  new_repo && add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; printf "PASS self-test:10-a (2 failing fixtures rejected)\n"'
  own_case self-test-rejects-two 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test 10-a
  else
    own_not_run self-test-rejects-two "macos alone: $own_gate_reason"
  fi
  if own_gate self-test-rejects-nothing; then
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS self-test:10-a (0 failing fixtures rejected)\n"'
  own_case self-test-rejects-nothing 1 'FAIL self-test:10-a (0 failing fixtures rejected)' macos --self-test 10-a
  else
    own_not_run self-test-rejects-nothing "macos alone: $own_gate_reason"
  fi
  if own_gate self-test-count-is-no-number; then
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS self-test:10-a (no failing fixtures rejected)\n"'
  own_case self-test-count-is-no-number 1 'FAIL self-test:10-a (exit 0)' macos --self-test 10-a
  else
    own_not_run self-test-count-is-no-number "macos alone: $own_gate_reason"
  fi
  if own_gate self-test-fails; then
  new_repo && add_script r1 10-a.sh "$good" 'printf "FAIL self-test:10-a (planned)\n"; exit 1'
  own_case self-test-fails 1 'FAIL self-test:10-a (planned)' macos --self-test 10-a
  else
    own_not_run self-test-fails "macos alone: $own_gate_reason"
  fi
  if own_gate self-test-exits-2; then
  new_repo && add_script r1 10-a.sh "$good" 'exit 2'
  own_case self-test-exits-2 1 'FAIL self-test:10-a (exit 2)' macos --self-test 10-a
  else
    own_not_run self-test-exits-2 "macos alone: $own_gate_reason"
  fi
  if own_gate self-test-pass-line-with-exit-1; then
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS self-test:10-a (2 failing fixtures rejected)\n"; exit 1'
  own_case self-test-pass-line-with-exit-1 1 'FAIL self-test:10-a (exit 1)' macos --self-test 10-a
  else
    own_not_run self-test-pass-line-with-exit-1 "macos alone: $own_gate_reason"
  fi
  new_repo && track_file &&
    add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; printf "x\n" >>"$CI_D_REPO/tracked.txt"; printf "PASS self-test:10-a (1 failing fixtures rejected)\n"' &&
    add_script r1 20-b.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; printf "PASS self-test:20-b (1 failing fixtures rejected)\n"'
  case_jobs=2
  own_case self-test-changes-a-tracked-file 1 'FAIL run-ci-d (the step ci.d self-test changed a tracked file: tracked.txt)' macos --self-test 10-a 20-b
  new_repo && track_file && add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; printf "PASS self-test:10-a (1 failing fixtures rejected)\n"' &&
    printf 'x\n' >>"$fixture/tracked.txt"
  case_absent='FAIL run-ci-d (the step ci.d self-test changed a tracked file: tracked.txt)'
  own_case tracked-file-differs-before-the-first-self-test 1 'FAIL run-ci-d (a tracked file differs from HEAD before the first script: tracked.txt)' macos --self-test 10-a

  new_repo && add_script B 10-x.sh "$good" ': >"$CI_D_REPO/first-ran"; printf "PASS 10-x\n"' &&
    add_script a 20-y.sh "$good" '[ -e "$CI_D_REPO/first-ran" ] || { printf "FAIL 20-y (it ran first)\n"; exit 1; }; printf "PASS 20-y\n"'
  case_locale=en_US.UTF-8
  own_case byte-order 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  case_locale=en_US.UTF-8
  case_first='scripts/B/ci.d/10-x.sh'
  case_then='scripts/a/ci.d/20-y.sh'
  own_case list-in-byte-order 0 'scripts/a/ci.d/20-y.sh' --list macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "line of 10-a\n"; printf "PASS 10-a\n"'
  case_first="line of 10-a${nl}PASS 10-a${nl}seconds 10-a "
  case_then='PASS 10-a'
  case_once='line of 10-a'
  own_case seconds-line-after-the-output-of-a-script 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" 'here=$(pwd); [ "$CI_D_REPO" = "$here" ] && [ "$CI_D_JOB" = macos ] && [ "$CI_D_ARTIFACTS" = "$here/target/ci-artifacts/10-a" ] && [ -d "$CI_D_ARTIFACTS" ] && [ "$LC_ALL" = C ] && [ "${PATH%%:*}" = "$CARGO_HOME/bin" ] && [ "${CALLER_VALUE:-}" = kept ] && [ "${SECOND_CALLER_VALUE:-}" = kept ] && [ "${HOME##*/}" = case-home ] && [ "${job:-}" != from-the-caller ] || { printf "FAIL 10-a (environment)\n"; exit 1; }; printf "PASS 10-a\n"'
  case_locale=en_US.UTF-8
  case_env=yes
  own_case environment 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  leaves='[ -d "$TMPDIR" ] && [ -z "$(ls -A "$TMPDIR")" ] || { printf "FAIL %s (its TMPDIR is no empty directory)\n" "$me"; exit 1; }; printf "%s\n" "$TMPDIR" >>"$CI_D_REPO/tmpdirs"; mkdir "$TMPDIR/left" && : >"$TMPDIR/left/file"'
  new_repo && add_script r1 10-a.sh "$good" "me=10-a; $leaves; printf 'PASS 10-a\\n'" &&
    add_script r1 20-b.sh "$good" "me=20-b; $leaves; chmod 0 \"\$TMPDIR/left\"; printf 'FAIL 20-b (planned)\\n'; exit 1"
  case_gone=2
  own_case tmpdir-of-each-script-removed-whatever-its-status 1 'FAIL 20-b (planned)' macos
  real_rm=$(command -v rm) || real_rm='rm'
  mkdir -p "$tmp/rm-fails-once" &&
    printf '#!/bin/sh\nfor a in "$@"; do last=$a; done\ncase $last in *.10-a/t) if [ ! -e rm-failed-once ]; then : >rm-failed-once; exit 1; fi ;; esac\nexec %s "$@"\n' "$real_rm" >"$tmp/rm-fails-once/rm" &&
    chmod +x "$tmp/rm-fails-once/rm" || wrong="$wrong stand-in-rm-not-made"
  new_repo && add_script r1 10-a.sh "$good" "me=10-a; $leaves; printf 'PASS 10-a\\n'"
  case_path=$tmp/rm-fails-once
  case_gone=1
  case_made=rm-failed-once
  case_witness=yes
  own_case tmpdir-whose-first-removal-fails-is-removed-on-a-later-try 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  mkdir -p "$tmp/rm-fails-twice" &&
    printf '#!/bin/sh\nfor a in "$@"; do last=$a; done\ncase $last in *.10-a/t)\n  now=$(%s -MTime::HiRes=time -e '"'"'printf "%%d\\n", time * 1000'"'"') || exit 1\n  n=0; [ ! -r rm-count ] || read -r n <rm-count\n  prev=0; [ ! -r rm-last ] || read -r prev <rm-last\n  if [ "$n" -lt 2 ] || [ "$((now - prev))" -lt 900 ]; then echo "$((n + 1))" >rm-count; echo "$now" >rm-last; exit 1; fi ;;\nesac\nexec %s "$@"\n' "$perl_bin" "$real_rm" >"$tmp/rm-fails-twice/rm" &&
    chmod +x "$tmp/rm-fails-twice/rm" || wrong="$wrong stand-in-rm-not-made"
  new_repo && add_script r1 10-a.sh "$good" "me=10-a; $leaves; printf 'PASS 10-a\\n'"
  case_path=$tmp/rm-fails-twice
  case_gone=1
  case_made=rm-last
  case_witness=yes
  own_case tmpdir-removed-on-the-third-try-after-two-waits 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 20-b.sh "$good" "$pass_b"
  case_tmpdir=$(mktemp -d "$tmp/own-tmp.XXXXXX")
  case_empty=yes
  own_case driver-removes-its-temporary-directory 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" 'mkdir "$TMPDIR/left"; chmod 555 "$TMPDIR/.."; printf "PASS 10-a\n"'
  mkdir "$tmp/read-only" && chmod 555 "$tmp/read-only"
  if { true >"$tmp/read-only/probe"; } 2>/dev/null; then
    own_case tmpdir-whose-parent-the-script-made-read-only-is-removed 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
    own_not_run tmpdir-that-cannot-be-removed 'a directory of mode 555 takes a new file on this host'
  else
    own_not_run tmpdir-whose-parent-the-script-made-read-only-is-removed 'a directory of mode 555 takes no new file on this host'
    case_tmpdir=$(mktemp -d "$tmp/own-tmp.XXXXXX")
    case_empty=yes
    own_case tmpdir-that-cannot-be-removed 1 'FAIL 10-a (its TMPDIR cannot be removed)' macos
  fi
  new_repo && add_script r1 10-a.sh "$good" 'chmod 555 "$TMPDIR/.."; printf "PASS 10-a\n"' && add_script r1 20-b.sh "$good" "$pass_b"
  case_witness=yes
  if { true >"$tmp/read-only/probe"; } 2>/dev/null; then
    own_case parent-of-a-tmpdir-closed-by-its-script-leaves-the-next-script 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  else
    case_first="FAIL 10-a (its TMPDIR cannot be removed)${nl}PASS 20-b"
    case_then='ci.d macos: 2 passed, 1 failed, 0 skipped'
    own_case parent-of-a-tmpdir-closed-by-its-script-leaves-the-next-script 1 'FAIL 10-a (its TMPDIR cannot be removed)' macos
  fi
  chmod 755 "$tmp/read-only"
  if command -v chflags >/dev/null 2>&1 && mkdir "$tmp/flag-probe" && : >"$tmp/flag-probe/f" && chflags uchg "$tmp/flag-probe/f" 2>/dev/null; then
    chflags nouchg "$tmp/flag-probe/f"
    trap 'own_unflag' EXIT
    trap 'own_unflag; exit 143' TERM
    trap 'own_unflag; exit 130' INT
    trap 'own_unflag; exit 129' HUP
    locks='mkdir "$TMPDIR/locked" && : >"$TMPDIR/locked/file" && chflags uchg "$TMPDIR/locked/file" && printf "%s\n" "$TMPDIR/locked/file" >>"$CI_D_REPO/flagged"'
    new_repo && add_script r1 10-a.sh "$good" "[ \"\${1:-}\" = --self-test ] || exit 3; $locks; printf 'PASS self-test:10-a (1 failing fixtures rejected)\\n'"
    case_absent='FAIL self-test:10-a (it ended without an exit status)'
    own_case self-test-tmpdir-that-cannot-be-removed 1 'FAIL self-test:10-a (its TMPDIR cannot be removed)' macos --self-test 10-a
    own_unflag
    new_repo && add_script r1 10-a.sh "$good" "$locks; printf 'PASS 10-a\\n'"
    case_absent='FAIL 10-a (it ended without an exit status)'
    case_then='ci.d macos: 1 passed, 1 failed, 0 skipped'
    case_first='FAIL 10-a (its TMPDIR cannot be removed)'
    own_case tmpdir-flagged-immutable-cannot-be-removed 1 'FAIL 10-a (its TMPDIR cannot be removed)' macos
    own_unflag
    trap - EXIT TERM INT HUP
  else
    own_not_run self-test-tmpdir-that-cannot-be-removed 'no chflags on this host'
    own_not_run tmpdir-flagged-immutable-cannot-be-removed 'no chflags on this host'
  fi
  term_child='( trap ": >\"\$CI_D_REPO/child-got-term\"; exit 0" TERM; sleep 60 & wait ) & printf "%s\n" "$!" >>"$CI_D_REPO/pids"'
  new_repo && add_script r1 10-a.sh "$good" "me=10-a; $leaves; $term_child; trap '' TERM; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; wait; printf 'PASS 10-a\\n'" &&
    add_script r1 20-b.sh "$good" "$pass_b"
  case_gone=1
  case_made=child-got-term
  case_limit=5
  case_max_s=40
  case_path=$tmp/slow-sleep
  case_first='FAIL 10-a (time limit 5 s)'
  case_then='PASS 20-b'
  own_case script-that-ignores-term-past-its-limit 1 'FAIL 10-a (time limit 5 s)' macos
  new_repo && add_script r1 10-a.sh "$good" "$pass_a"
  case_limit=2s
  own_case limit-not-a-number-in-the-step-ci.d 1 'FAIL run-ci-d (CI_D_SCRIPT_LIMIT_S=2s is not a whole number from 1 to 999999999)' macos
  new_repo && add_script r1 10-a.sh "$good" '[ $# -eq 0 ] || exit 3; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 30 & printf "%s\n%s\n" "$$" "$!" >>"$CI_D_REPO/pids"; wait; printf "PASS 10-a\n"' &&
    printf '1200\n' >"$fixture/offset"
  case_path=$tmp/late-date
  case_max_s=15
  own_case default-limit-reached-at-1200-s 1 'FAIL 10-a (time limit 1200 s)' macos
  new_repo && add_script r1 10-a.sh "$good" '[ $# -eq 0 ] || exit 3; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 1; printf "PASS 10-a\n"' &&
    printf '1180\n' >"$fixture/offset"
  case_path=$tmp/late-date
  own_case default-limit-not-reached-at-1181-s 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 30 & printf "%s\n%s\n" "$$" "$!" >>"$CI_D_REPO/pids"; wait; printf "PASS self-test:10-a (1 failing fixtures rejected)\n"' &&
    printf '1200\n' >"$fixture/offset"
  case_path=$tmp/late-date
  case_max_s=15
  own_case default-limit-of-a-self-test-reached-at-1200-s 1 'FAIL self-test:10-a (time limit 1200 s)' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1500" '[ $# -eq 0 ] || exit 3; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 1; printf "PASS 10-a\n"' &&
    printf '1200\n' >"$fixture/offset"
  case_path=$tmp/late-date
  case_witness=yes
  own_case limit-line-raises-the-limit-of-its-script 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1500" '[ $# -eq 0 ] || exit 3; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 30 & printf "%s\n%s\n" "$$" "$!" >>"$CI_D_REPO/pids"; wait; printf "PASS 10-a\n"' &&
    printf '1500\n' >"$fixture/offset"
  case_path=$tmp/late-date
  case_max_s=15
  case_witness=yes
  own_case limit-line-reached 1 'FAIL 10-a (time limit 1500 s)' macos
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1500" 'trap "" TERM; sleep 60 & printf "%s\n%s\n" "$$" "$!" >>"$CI_D_REPO/pids"; wait; printf "PASS 10-a\n"'
  case_limit=2
  case_max_s=20
  case_witness=yes
  own_case limit-of-the-caller-before-the-limit-line 1 'FAIL 10-a (time limit 2 s)' macos
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 20m" "$pass_a"
  own_case limit-line-that-is-no-whole-number 1 "FAIL 10-a (its '# limit:' line is not a whole number of seconds from 1 to 999999999)" --rules
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1500$nl# limit: 1800" "$pass_a"
  own_case two-limit-lines 1 "FAIL 10-a (its first 10 lines hold more than one '# limit:' line)" --rules
  mkdir -p "$tmp/jump-date" &&
    printf '#!/bin/sh\nif [ "$1" = +%%s ] && [ -r jump ]; then\n  now=$(%s +%%s) || exit 1\n  if [ -e jump.used ]; then read -r jump <jump; echo "$((now + jump))"; else : >jump.used; echo "$now"; fi\nelse\n  exec %s "$@"\nfi\n' "$real_date" "$real_date" >"$tmp/jump-date/date" &&
    chmod +x "$tmp/jump-date/date" || wrong="$wrong stand-in-date-not-made"
  new_repo && printf '2700\n' >"$fixture/jump"
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=slow
  case_path=$tmp/jump-date
  case_max_s=40
  case_witness=yes
  own_case own-cases-past-2700-s-pass-below-their-limit 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test run-ci-d
  new_repo && printf '2800\n' >"$fixture/jump"
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=slow
  case_path=$tmp/jump-date
  case_max_s=40
  case_witness=yes
  own_case own-cases-reach-their-limit-at-2800-s 1 'FAIL self-test:run-ci-d (time limit 2800 s)' macos --self-test run-ci-d
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1500" '[ "${1:-}" = --self-test ] || exit 3; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 1; printf "PASS self-test:10-a (1 failing fixtures rejected)\n"' &&
    printf '1200\n' >"$fixture/offset"
  case_path=$tmp/late-date
  case_witness=yes
  own_case limit-line-raises-the-limit-of-its-self-test 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 2" 'sleep 4; printf "PASS 10-a\n"'
  case_witness=yes
  own_case limit-line-below-the-default-leaves-the-default 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  for bound in 0 0100 1000000000; do
    new_repo && add_script r1 10-a.sh "$good$nl# limit: $bound" "$pass_a"
    own_case "limit-line-of-$bound-is-refused" 1 "FAIL 10-a (its '# limit:' line is not a whole number of seconds from 1 to 999999999)" --rules
  done
  new_repo && add_script r1 10-a.sh "$good$nl$seven$nl# limit: 20m" "$pass_a"
  own_case limit-line-at-line-11-is-no-limit-line 0 '10-a (macos windows): A fixture rule.' --rules
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1500$cr" '[ $# -eq 0 ] || exit 3; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 1; printf "PASS 10-a\n"' &&
    printf '1200\n' >"$fixture/offset"
  case_path=$tmp/late-date
  case_witness=yes
  own_case limit-line-with-a-carriage-return-raises-the-limit 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  pair_a='if [ "${1:-}" = --self-test ]; then v="PASS self-test:10-a (1 failing fixtures rejected)"; else [ $# -eq 0 ] || exit 3; v="PASS 10-a"; fi; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 1; printf "%s\n" "$v"'
  pair_b=': >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset2" "$CI_D_REPO/late"; sleep 30 & printf "%s\n%s\n" "$$" "$!" >>"$CI_D_REPO/pids"; wait; printf "PASS 20-b\n"'
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1800" "$pair_a" && add_script r1 20-b.sh "$good" "$pair_b" && fixture_commit &&
    printf '1500\n' >"$fixture/offset" && printf '3000\n' >"$fixture/offset2"
  case_path=$tmp/late-date
  case_max_s=15
  case_witness=yes
  case_first='PASS 10-a'
  case_then='FAIL 20-b (time limit 1200 s)'
  own_case limit-line-of-1800-passes-past-1200-s-where-no-line-fails 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1800" "$pair_a" && add_script r1 20-b.sh "$good" "$pair_b" && fixture_commit &&
    printf '1500\n' >"$fixture/offset" && printf '3000\n' >"$fixture/offset2"
  case_path=$tmp/late-date
  case_jobs=1
  case_max_s=15
  case_witness=yes
  case_first='PASS self-test:10-a (1 failing fixtures rejected)'
  case_then='FAIL self-test:20-b (time limit 1200 s)'
  own_case limit-line-of-1800-passes-past-1200-s-where-no-line-fails-in-the-self-test-step 1 'ci.d macos self-test: 1 passed, 1 failed' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good$nl# limit: 1800" '[ $# -eq 0 ] || exit 3; : >"$CI_D_REPO/want-read.${0##*/}"; n=0; while [ ! -e "$CI_D_REPO/read-done.${0##*/}" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done; cp "$CI_D_REPO/offset" "$CI_D_REPO/late"; sleep 30 & printf "%s\n%s\n" "$$" "$!" >>"$CI_D_REPO/pids"; wait; printf "PASS 10-a\n"' &&
    add_script r1 20-b.sh "$good" "$pass_b" && fixture_commit && printf '1800\n' >"$fixture/offset"
  case_path=$tmp/late-date
  case_max_s=15
  case_witness=yes
  case_first='FAIL 10-a (time limit 1800 s)'
  case_then='PASS 20-b'
  own_case limit-line-of-1800-reached-past-1800-s 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  own_tree_case limit-lines-of-the-repository "scripts/r1/ci.d/12-test.sh: # limit: 1800${nl}scripts/r1/ci.d/15-gate-self-test.sh: # limit: 1800"

  new_repo && add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; printf "PASS self-test:10-a (2 failing fixtures rejected)\n"'
  own_case bare-self-test-runs-the-own-cases 0 'PASS self-test:run-ci-d (a nested run stops after its first case)' macos --self-test
  if own_gate self-test-without-scripts; then
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  own_case self-test-without-scripts 1 'FAIL run-ci-d (no scripts for macos)' macos --self-test
  else
    own_not_run self-test-without-scripts "macos alone: $own_gate_reason"
  fi
  if own_gate self-test-of-the-driver-alone; then
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  own_case self-test-of-the-driver-alone 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test run-ci-d

  else
    own_not_run self-test-of-the-driver-alone "macos alone: $own_gate_reason"
  fi
  prelude='[ "${1:-}" = --self-test ] || exit 3; me=${0##*/}; me=${me%.sh}; w() { n=0; while [ ! -e "$CI_D_REPO/$1" ]; do [ "$n" -lt 100 ] || return 1; sleep 0.1; n=$((n + 1)); done; }; meet() { : >"$CI_D_REPO/start.$me"; n=0; while [ "$n" -lt 100 ]; do for f in "$CI_D_REPO"/start.*; do [ "$f" = "$CI_D_REPO/start.$me" ] || [ ! -e "$f" ] || return 0; done; sleep 0.1; n=$((n + 1)); done; return 1; }'
  passes='printf "PASS self-test:%s (1 failing fixtures rejected)\n" "$me"'
  ran_alone='{ printf "FAIL self-test:%s (it ran alone)\n" "$me"; exit 1; }'
  starts=': >"$CI_D_REPO/start.$me"'
  ends=': >"$CI_D_REPO/end.$me"'
  running='others=0; for f in "$CI_D_REPO"/start.*; do o=${f##*/start.}; [ "$o" = "$me" ] || [ ! -e "$f" ] || [ -e "$CI_D_REPO/end.$o" ] || others=$((others + 1)); done'
  alone='before=0; for f in "$CI_D_REPO"/start.*; do [ -e "$f" ] || continue; o=${f##*/start.}; [ -e "$CI_D_REPO/end.$o" ] || { printf "FAIL self-test:%s (it started beside %s)\n" "$me" "$o"; exit 1; }; before=$((before + 1)); done; : >"$CI_D_REPO/start.$me"; sleep 2; after=0; for f in "$CI_D_REPO"/start.*; do after=$((after + 1)); done; [ "$after" -eq "$((before + 1))" ] || { printf "FAIL self-test:%s (another self-test started beside it)\n" "$me"; exit 1; }'
  meets="$prelude; meet || $ran_alone; $passes"
  prelude_any=${prelude#*exit 3; }
  alone_list=scripts/ci/self-test-alone.txt

  new_repo && add_script r1 10-a.sh "$good" "$prelude; $leaves; $passes" && add_script r1 20-b.sh "$good" "$prelude; $leaves; $passes"
  case_jobs=2
  case_gone=2
  own_case tmpdir-of-each-self-test-removed 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; ( sleep 30 ) >/dev/null 2>&1 & printf '%s\\n' \"\$!\" >>\"\$CI_D_REPO/pids\"; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $passes"
  case_jobs=2
  case_max_s=25
  own_case child-that-a-self-test-leaves-is-ended-at-the-end-of-the-step 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  ends_together='case $me in 10-a) other=20-b ;; *) other=10-a ;; esac; ( trap "t=\$CI_D_REPO/term; : >\"\$t.\$me\"; n=0; while [ ! -e \"\$t.\$other\" ] && [ \$n -lt 30 ]; do sleep 0.1; n=\$((n + 1)); done; [ ! -e \"\$t.\$other\" ] || : >\"\$CI_D_REPO/met.\$me\"; exit 0" TERM; sleep 60 & wait ) >/dev/null 2>&1 & printf "%s\n" "$!" >>"$CI_D_REPO/pids"'
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $ends_together; $passes" && add_script r1 20-b.sh "$good" "$prelude; $ends_together; $passes"
  case_jobs=2
  case_made=met.10-a
  case_max_s=40
  case_witness=yes
  own_case groups-that-two-self-tests-left-are-ended-by-one-process 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes" && add_script r1 20-b.sh "$good" "$prelude; $passes"
  case_tmpdir=$(mktemp -d "$tmp/own-tmp.XXXXXX")
  case_empty=yes
  own_case driver-removes-its-temporary-directory-in-the-self-test-step 0 'ci.d macos self-test: 3 passed, 0 failed' macos --self-test

  if own_gate self-tests-run-at-the-same-time; then
  new_repo && add_script r1 10-a.sh "$good" "$meets" && add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case self-tests-run-at-the-same-time 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  else
    own_not_run self-tests-run-at-the-same-time "macos alone: $own_gate_reason"
  fi
  new_repo &&
    add_script r1 10-a.sh "$good" "$prelude; printf \"line 1 of %s\\n\" \"\$me\"; printf \"note of %s\\n\" \"\$me\" >&2; : >\"\$CI_D_REPO/a1\"; w b1 || exit 1; printf \"line 2 of %s\\n\" \"\$me\"; : >\"\$CI_D_REPO/a2\"; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; w a1 || exit 1; printf \"line 1 of %s\\n\" \"\$me\"; printf \"note of %s\\n\" \"\$me\" >&2; : >\"\$CI_D_REPO/b1\"; w a2 || exit 1; printf \"line 2 of %s\\n\" \"\$me\"; $passes"
  case_jobs=2
  case_first="line 1 of 10-a${nl}line 2 of 10-a${nl}PASS self-test:10-a (1 failing fixtures rejected)${nl}note of 10-a${nl}seconds 10-a "
  case_then="line 1 of 20-b${nl}line 2 of 20-b${nl}PASS self-test:20-b (1 failing fixtures rejected)${nl}note of 20-b${nl}seconds 20-b "
  own_case outputs-kept-apart-in-name-order 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || $ran_alone; sleep 1; $ends; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; meet || $ran_alone; sleep 1; $ends; $passes" &&
    add_script r1 30-c.sh "$good$nl# self-test: alone" "$prelude; $alone; $ends; $passes" &&
    add_script r1 40-d.sh "$good" "$prelude; $alone; $ends; $passes" &&
    add_script r1 50-e.sh "$good" "$prelude; $starts; sleep 1; $ends; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '40-d its self-test fails beside another one\n' >"$fixture/$alone_list"
  case_jobs=4
  own_case alone-by-header-and-by-list 0 'ci.d macos self-test: 5 passed, 0 failed' macos --self-test 10-a 20-b 30-c 40-d 50-e
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $alone; $ends; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $starts; sleep 1; $ends; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '10-a its self-test fails beside another one\r\n' >"$fixture/$alone_list"
  case_jobs=2
  own_case alone-list-line-with-a-carriage-return 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  if own_gate two-at-a-time; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $starts; sleep 1; $running; [ \"\$others\" -le 1 ] || { printf \"FAIL self-test:%s (two others ran beside it)\\n\" \"\$me\"; exit 1; }; $ends; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $starts; sleep 1; $running; [ \"\$others\" -le 1 ] || { printf \"FAIL self-test:%s (two others ran beside it)\\n\" \"\$me\"; exit 1; }; $ends; $passes" &&
    add_script r1 30-c.sh "$good" "$prelude; $starts; sleep 1; $running; [ \"\$others\" -le 1 ] || { printf \"FAIL self-test:%s (two others ran beside it)\\n\" \"\$me\"; exit 1; }; $ends; $passes"
  case_jobs=2
  own_case two-at-a-time 0 'ci.d macos self-test: 3 passed, 0 failed' macos --self-test 10-a 20-b 30-c
  else
    own_not_run two-at-a-time "macos alone: $own_gate_reason"
  fi
  if own_gate failing-self-test-beside-another; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || $ran_alone; printf \"FAIL self-test:%s (planned)\\n\" \"\$me\"; exit 1" &&
    add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case failing-self-test-beside-another 1 'FAIL self-test:10-a (planned)' macos --self-test 10-a 20-b
  else
    own_not_run failing-self-test-beside-another "macos alone: $own_gate_reason"
  fi
  new_repo && add_script r1 10-a.sh "$good" "$prelude; printf \"%s\\n\" \"\$me\" >\"\$TMPDIR/shared\"; meet || $ran_alone; read -r got <\"\$TMPDIR/shared\"; [ \"\$got\" = \"\$me\" ] || { printf \"FAIL self-test:%s (another self-test wrote its file)\\n\" \"\$me\"; exit 1; }; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; printf \"%s\\n\" \"\$me\" >\"\$TMPDIR/shared\"; meet || $ran_alone; read -r got <\"\$TMPDIR/shared\"; [ \"\$got\" = \"\$me\" ] || { printf \"FAIL self-test:%s (another self-test wrote its file)\\n\" \"\$me\"; exit 1; }; $passes"
  case_jobs=2
  own_case two-self-tests-write-one-path 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  if own_gate self-test-prints-nothing; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || exit 1; exit 0" && add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case self-test-prints-nothing 1 'FAIL self-test:10-a (exit 0)' macos --self-test 10-a 20-b
  else
    own_not_run self-test-prints-nothing "macos alone: $own_gate_reason"
  fi
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || $ran_alone; kill -s TERM \"\$\$\"; $passes" && add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case self-test-ended-by-a-signal 1 'FAIL self-test:10-a (exit 143)' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || $ran_alone; kill -s KILL \"\$PPID\"; sleep 1; $passes" && add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case driver-job-of-a-self-test-killed 1 'FAIL self-test:10-a (it ended without an exit status)' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $leaves; $term_child; trap 'printf \"ended by TERM\\n\"; exit 143' TERM; sleep 30 & wait \"\$!\"; $passes" && add_script r1 20-b.sh "$good" "$prelude; $passes"
  case_jobs=2
  case_gone=1
  case_made=child-got-term
  case_limit=2
  case_first="ended by TERM${nl}seconds 10-a "
  case_then='FAIL self-test:10-a (time limit 2 s)'
  own_case self-test-past-its-limit 1 'FAIL self-test:10-a (time limit 2 s)' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; printf '%s\\n' \"\$\$\" >\"\$CI_D_REPO/pid.10-a\"; trap '' TERM; sleep 60 & printf '%s\\n' \"\$!\" >>\"\$CI_D_REPO/pids\"; wait; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $passes" &&
    add_script r1 30-c.sh "$good$nl# self-test: alone" "$prelude; read -r p <\"\$CI_D_REPO/pid.10-a\"; n=0; while kill -0 \"\$p\" 2>/dev/null; do [ \"\$n\" -lt 50 ] || { printf \"FAIL self-test:%s (the self-test past its limit still runs)\\n\" \"\$me\"; exit 1; }; sleep 0.1; n=\$((n + 1)); done; $passes"
  case_jobs=2
  case_limit=2
  case_max_s=30
  case_first='FAIL self-test:10-a (time limit 2 s)'
  case_then='PASS self-test:30-c (1 failing fixtures rejected)'
  own_case self-test-that-ignores-term-past-its-limit 1 'FAIL self-test:10-a (time limit 2 s)' macos --self-test 10-a 20-b 30-c
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes" && printf 'a file\n' >"$fixture/target"
  own_case artifacts-directory-of-a-self-test-cannot-be-made 1 'FAIL 10-a (its artifacts directory cannot be made)' macos --self-test 10-a
  procs=$(getconf _NPROCESSORS_ONLN 2>/dev/null) || procs=
  case $procs in '' | 0* | *[!0-9]*) procs=${NUMBER_OF_PROCESSORS:-1} ;; esac
  if [ "$procs" -ge 2 ] 2>/dev/null; then
    new_repo && add_script r1 10-a.sh "$good" "$meets" && add_script r1 20-b.sh "$good" "$meets"
    own_case default-of-several-at-a-time 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  else
    own_not_run default-of-several-at-a-time 'one processor on this host'
  fi
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '99-gone it was renamed\n' >"$fixture/$alone_list"
  own_case alone-list-names-no-script 1 "FAIL run-ci-d ($alone_list names 99-gone, which is no script)" macos --self-test 10-a
  if own_gate alone-list-names-a-script-of-the-other-job-alone; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes" &&
    add_script r1 20-w.sh "# jobs: windows$nl# rule: A fixture rule." "$prelude; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '20-w its self-test fails beside another one\n' >"$fixture/$alone_list"
  own_case alone-list-names-a-script-of-the-other-job-alone 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test 10-a
  else
    own_not_run alone-list-names-a-script-of-the-other-job-alone "macos alone: $own_gate_reason"
  fi
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes"
  case_jobs=x
  own_case jobs-not-a-number 1 'FAIL run-ci-d (CI_D_SELF_TEST_JOBS=x is not a whole number from 1 to 999999999)' macos --self-test 10-a
  case_jobs=0
  own_case jobs-zero 1 'FAIL run-ci-d (CI_D_SELF_TEST_JOBS=0 is not a whole number from 1 to 999999999)' macos --self-test 10-a
  case_limit=2s
  own_case limit-not-a-number 1 'FAIL run-ci-d (CI_D_SCRIPT_LIMIT_S=2s is not a whole number from 1 to 999999999)' macos --self-test 10-a
  case_limit=99999999999999999999
  own_case limit-of-twenty-digits 1 'FAIL run-ci-d (CI_D_SCRIPT_LIMIT_S=99999999999999999999 is not a whole number from 1 to 999999999)' macos --self-test 10-a
  case_limit=999999999
  own_case limit-of-nine-digits 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" "$prelude; ( sleep 1; printf \"FAIL self-test:%s (a check that ended late)\\n\" \"\$me\" ) & $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $passes"
  case_jobs=2
  own_case output-of-a-child-that-outlives-its-self-test 1 'FAIL self-test:10-a (exit 0)' macos --self-test 10-a 20-b
  if own_gate refusal-keeps-its-place-in-the-recap; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude; sleep 1; $passes" &&
    add_script r1 20-x.sh '# rule: A fixture rule.' "$prelude; $passes" &&
    add_script r1 30-c.sh "$good" "$prelude; $passes"
  case_jobs=2
  case_first="PASS self-test:10-a (1 failing fixtures rejected)${nl}FAIL 20-x (its first 10 lines hold no single '# jobs:' line)${nl}PASS self-test:30-c (1 failing fixtures rejected)"
  case_then='ci.d macos self-test: 2 passed, 1 failed'
  own_case refusal-keeps-its-place-in-the-recap 1 "FAIL 20-x (its first 10 lines hold no single '# jobs:' line)" macos --self-test 10-a 30-c
  else
    own_not_run refusal-keeps-its-place-in-the-recap "macos alone: $own_gate_reason"
  fi
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=fail
  own_case own-cases-that-fail-in-a-nested-run 1 'ci.d macos self-test: 0 passed, 1 failed' macos --self-test run-ci-d
  nested_line='FAIL run-ci-d (RUN_CI_D_NESTED is set; the driver refuses it, also when it is empty)'
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_switch_set=yes
  case_switch=yes
  case_witness=yes
  case_absent='PASS self-test:run-ci-d (a nested run stops after its first case)'
  own_case nested-switch-set-by-the-caller-is-refused 1 "$nested_line" macos --self-test run-ci-d
  mkdir -p "$tmp/caller-switch" && printf 'yes\n' >"$tmp/caller-switch/nested-switch"
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_switch_set=yes
  case_switch=$tmp/caller-switch/nested-switch
  case_witness=yes
  case_absent='PASS self-test:run-ci-d (a nested run stops after its first case)'
  own_case nested-switch-file-of-the-caller-is-refused 1 "$nested_line" macos --self-test run-ci-d
  new_repo && add_script r1 10-a.sh "$good" "$pass_a"
  case_switch_set=yes
  case_switch=fail
  case_witness=yes
  case_absent='PASS 10-a'
  own_case nested-switch-set-by-the-caller-is-refused-in-the-step-ci.d 1 "$nested_line" macos
  case_switch_set=yes
  case_witness=yes
  case_absent='PASS 10-a'
  own_case empty-nested-switch-of-the-caller-is-refused 1 "$nested_line" macos
  new_repo && add_script r1 10-a.sh "$good" 'if { true <&9; } 2>/dev/null; then printf "FAIL 10-a (descriptor 9 is open)\n"; exit 1; fi; printf "PASS 10-a\n"'
  case_witness=yes
  own_case descriptor-9-is-closed-for-a-script 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  case_switch_set=yes
  case_switch=yes
  case_worker=yes
  case_witness=yes
  case_absent='PASS 10-a'
  own_case nested-switch-with-a-worker-variable-of-the-caller-is-refused 1 "$nested_line" macos
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=slow
  case_limit=2
  case_max_s=30
  case_witness=yes
  own_case own-cases-past-the-limit 1 'FAIL self-test:run-ci-d (time limit 2 s)' macos --self-test run-ci-d
  new_repo
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=hold
  case_limit=3
  case_max_s=40
  case_witness=yes
  own_case own-cases-past-the-limit-end-the-running-case 1 'FAIL self-test:run-ci-d (time limit 3 s)' macos --self-test run-ci-d
  new_repo
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=late
  case_limit=2
  case_max_s=60
  case_witness=yes
  own_case own-cases-stopped-while-the-running-case-starts-its-driver 1 'FAIL self-test:run-ci-d (time limit 2 s)' macos --self-test run-ci-d
  new_repo
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=gap
  case_limit=2
  case_max_s=60
  case_witness=yes
  own_case own-cases-stopped-between-the-start-and-the-record-of-the-running-case-s-driver 1 'FAIL self-test:run-ci-d (time limit 2 s)' macos --self-test run-ci-d
  new_repo
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=wgap
  case_limit=2
  case_max_s=60
  case_witness=yes
  own_case own-cases-stopped-between-the-start-and-the-record-of-the-running-case-s-witness 1 'FAIL self-test:run-ci-d (time limit 2 s)' macos --self-test run-ci-d
  new_repo
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=gap
  case_tmpdir=$(mktemp -d "$tmp/interrupt-tmp.XXXXXX")
  case_empty=yes
  case_signal=TERM
  case_wait=pids
  case_signal_min=2
  case_max_s=60
  case_witness=yes
  own_case interrupt-of-the-own-cases-ends-the-running-case-s-driver-and-witness 143 'FAIL run-ci-d (the step was interrupted by TERM; every self-test that it started is ended)' macos --self-test run-ci-d
  new_repo && add_script r1 10-a.sh "$good" "$prelude; printf '%s\\n' \"\$\$\" >\"\$CI_D_REPO/pid.10-a\"; sleep 60 >/dev/null 2>&1 & printf '%s\\n' \"\$!\" >>\"\$CI_D_REPO/pids\"; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; w pid.10-a || exit 1; read -r p <\"\$CI_D_REPO/pid.10-a\"; n=0; while kill -0 \"\$p\" 2>/dev/null && [ \"\$n\" -lt 100 ]; do sleep 0.1; n=\$((n + 1)); done; sleep 1; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.20-b\"; wait; $passes"
  case_jobs=2
  case_signal=TERM
  case_wait=started.20-b
  case_max_s=30
  case_signal_min=3
  case_witness=yes
  own_case interrupt-of-the-self-test-step-ends-the-groups-of-ended-self-tests 143 'FAIL run-ci-d (the step was interrupted by TERM; every self-test that it started is ended)' macos --self-test 10-a 20-b
  if own_gate alone-list-line-with-leading-blanks; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $alone; $ends; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $starts; sleep 1; $ends; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '  10-a\tits self-test fails beside another one\n' >"$fixture/$alone_list"
  case_jobs=2
  own_case alone-list-line-with-leading-blanks 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  else
    own_not_run alone-list-line-with-leading-blanks "macos alone: $own_gate_reason"
  fi
  if own_gate alone-list-with-comment-and-blank-lines; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $alone; $ends; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $starts; sleep 1; $ends; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '# a comment\n\n  # 20-b an indented comment\n10-a its self-test fails beside another one\n' >"$fixture/$alone_list"
  case_jobs=2
  own_case alone-list-with-comment-and-blank-lines 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  else
    own_not_run alone-list-with-comment-and-blank-lines "macos alone: $own_gate_reason"
  fi
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '10-a\n' >"$fixture/$alone_list"
  own_case alone-list-entry-without-a-reason 1 "FAIL run-ci-d ($alone_list names 10-a without a reason)" macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '  10-a \t\r\n' >"$fixture/$alone_list"
  own_case alone-list-entry-with-blanks-alone-after-the-name 1 "FAIL run-ci-d ($alone_list names 10-a without a reason)" macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes" &&
    mkdir -p "$fixture/$alone_list"
  own_case alone-list-that-cannot-be-read 1 "FAIL run-ci-d ($alone_list cannot be read)" macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" "$prelude; for f in \"\$TMPDIR\"/../../../run-ci-d.*/self-test.run-ci-d/done; do [ -e \"\$f\" ] || { printf 'FAIL self-test:%s (it started beside the own cases)\\n' \"\$me\"; exit 1; }; done; $passes"
  case_tmpdir=$(mktemp -d "$tmp/own-tmp.XXXXXX")
  case_jobs=1
  case_witness=yes
  own_case own-cases-take-one-of-the-slots 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test
  new_repo && add_script r1 10-a.sh "$good" "$meets" && add_script r1 20-b.sh "$good" "$meets"
  case_path=$tmp/no-getconf
  case_procs=2
  own_case default-number-from-number-of-processors 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  for procs in none x; do
    new_repo && add_script r1 10-a.sh "$good" "$prelude; $alone; $ends; $passes" && add_script r1 20-b.sh "$good" "$prelude; $alone; $ends; $passes"
    case_path=$tmp/no-getconf
    case_procs=$procs
    own_case "default-number-of-one-when-number-of-processors-is-$procs" 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  done
  if own_gate step-ci.d-one-script-at-a-time-whatever-ci-d-self-test-jobs-says; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude_any; $alone; $ends; printf 'PASS %s\\n' \"\$me\"" &&
    add_script r1 20-b.sh "$good" "$prelude_any; $alone; $ends; printf 'PASS %s\\n' \"\$me\""
  case_jobs=2
  own_case step-ci.d-one-script-at-a-time-whatever-ci-d-self-test-jobs-says 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  else
    own_not_run step-ci.d-one-script-at-a-time-whatever-ci-d-self-test-jobs-says "macos alone: $own_gate_reason"
  fi
  free="$good$nl# lane: free"
  check_passes='printf "PASS %s\n" "$me"'
  check_alone='{ printf "FAIL %s (it ran alone)\n" "$me"; exit 1; }'
  cargo_before='before=0; for f in "$CI_D_REPO"/cargo-start.*; do [ -e "$f" ] || continue; o=${f##*/cargo-start.}; [ -e "$CI_D_REPO/cargo-end.$o" ] || { printf "FAIL %s (it started beside %s)\n" "$me" "$o"; exit 1; }; before=$((before + 1)); done; : >"$CI_D_REPO/cargo-start.$me"'
  cargo_after='sleep 2; after=0; for f in "$CI_D_REPO"/cargo-start.*; do after=$((after + 1)); done; [ "$after" -eq "$((before + 1))" ] || { printf "FAIL %s (another check of the cargo lane started beside it)\n" "$me"; exit 1; }; : >"$CI_D_REPO/cargo-end.$me"'
  if own_gate checks-of-two-lanes-run-at-the-same-time; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude_any; meet || $check_alone; $check_passes" &&
    add_script r1 20-b.sh "$free" "$prelude_any; meet || $check_alone; $check_passes"
  own_case checks-of-two-lanes-run-at-the-same-time 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  else
    own_not_run checks-of-two-lanes-run-at-the-same-time "macos alone: $own_gate_reason"
  fi
  lone="$good$nl# lane: alone"
  busy=': >"$CI_D_REPO/start.$me"; sleep 2; : >"$CI_D_REPO/end.$me"'
  solo='for f in "$CI_D_REPO"/start.*; do [ -e "$f" ] || continue; o=${f##*/start.}; [ -e "$CI_D_REPO/end.$o" ] || { printf "FAIL %s (it started beside %s)\n" "$me" "$o"; exit 1; }; done; : >"$CI_D_REPO/start.$me"; sleep 2; for f in "$CI_D_REPO"/start.*; do o=${f##*/start.}; [ "$o" = "$me" ] || [ -e "$CI_D_REPO/end.$o" ] || { printf "FAIL %s (%s started beside it)\n" "$me" "$o"; exit 1; }; done; : >"$CI_D_REPO/end.$me"'
  after_lanes='[ -e "$CI_D_REPO/end.20-b" ] && [ -e "$CI_D_REPO/end.30-c" ] || { printf "FAIL %s (it started before the lanes ended)\n" "$me"; exit 1; }'
  if own_gate alone-check-starts-after-both-lanes-have-ended; then
  new_repo && add_script r1 10-a.sh "$lone" "$prelude_any; $after_lanes; $solo; $check_passes" &&
    add_script r1 20-b.sh "$good" "$prelude_any; $busy; $check_passes" &&
    add_script r1 30-c.sh "$free" "$prelude_any; $busy; $check_passes"
  own_case alone-check-starts-after-both-lanes-have-ended 0 'ci.d macos: 3 passed, 0 failed, 0 skipped' macos
  else
    own_not_run alone-check-starts-after-both-lanes-have-ended "macos alone: $own_gate_reason"
  fi
  if own_gate two-alone-checks-run-one-after-the-other-in-order; then
  new_repo && add_script r1 10-a.sh "$lone" "$prelude_any; $solo; $check_passes" &&
    add_script r1 20-b.sh "$lone" "$prelude_any; [ -e \"\$CI_D_REPO/end.10-a\" ] || { printf 'FAIL %s (it ran before 10-a)\\n' \"\$me\"; exit 1; }; $solo; $check_passes" &&
    add_script r1 30-c.sh "$good" "$prelude_any; $busy; $check_passes"
  own_case two-alone-checks-run-one-after-the-other-in-order 0 'ci.d macos: 3 passed, 0 failed, 0 skipped' macos
  else
    own_not_run two-alone-checks-run-one-after-the-other-in-order "macos alone: $own_gate_reason"
  fi
  if own_gate output-of-an-alone-check-in-the-order-of-the-run; then
  new_repo && add_script r1 10-a.sh "$lone" "$prelude_any; printf 'line of %s\\n' \"\$me\"; $check_passes" &&
    add_script r1 20-b.sh "$good" "$prelude_any; printf 'line of %s\\n' \"\$me\"; $check_passes" &&
    add_script r1 30-c.sh "$free" "$prelude_any; printf 'line of %s\\n' \"\$me\"; $check_passes"
  case_first="line of 10-a${nl}PASS 10-a${nl}seconds 10-a "
  case_then="line of 20-b${nl}PASS 20-b${nl}seconds 20-b "
  own_case output-of-an-alone-check-in-the-order-of-the-run 0 'ci.d macos: 3 passed, 0 failed, 0 skipped' macos
  else
    own_not_run output-of-an-alone-check-in-the-order-of-the-run "macos alone: $own_gate_reason"
  fi
  new_repo && track_file && add_script r1 10-a.sh "$good" "$prelude_any; $busy; $check_passes" &&
    add_script r1 20-b.sh "$lone" "$prelude_any; printf 'x\\n' >>\"\$CI_D_REPO/tracked.txt\"; $check_passes" &&
    add_script r1 30-c.sh "$free" "$prelude_any; $busy; $check_passes"
  case_absent='FAIL run-ci-d (one of 20-b, 30-c changed a tracked file: tracked.txt)'
  own_case tracked-file-changed-by-an-alone-check-names-it-alone 1 'FAIL run-ci-d (20-b changed a tracked file: tracked.txt)' macos
  new_repo && add_script r1 10-a.sh "$lone" "trap '' TERM; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; wait; printf 'PASS 10-a\\n'" &&
    add_script r1 20-b.sh "$lone" "printf 'PASS 20-b\\n'"
  case_limit=3
  case_max_s=40
  case_witness=yes
  case_first='FAIL 10-a (time limit 3 s)'
  case_then='PASS 20-b'
  own_case alone-check-past-its-limit-and-the-next-runs 1 'FAIL 10-a (time limit 3 s)' macos
  new_repo && add_script r1 10-a.sh "$good" "printf 'line of 10-a\\n'; printf 'PASS 10-a\\n'" &&
    add_script r1 20-b.sh "$lone" "printf 'line of 20-b\\n'; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.20-b\"; wait; printf 'PASS 20-b\\n'"
  case_signal=INT
  case_wait=started.20-b
  case_max_s=30
  case_signal_min=2
  case_witness=yes
  case_first='line of 10-a'
  case_then='line of 20-b'
  own_case interrupt-during-the-alone-part 130 'FAIL run-ci-d (the step was interrupted by INT; every script that it started is ended)' macos
  new_repo && add_script r1 10-a.sh "$lone$cr" "$prelude_any; [ -e \"\$CI_D_REPO/end.20-b\" ] || { printf 'FAIL %s (it started before the lanes ended)\\n' \"\$me\"; exit 1; }; $check_passes" &&
    add_script r1 20-b.sh "$good" "$prelude_any; $busy; $check_passes"
  own_case alone-line-with-a-carriage-return-names-the-alone-part 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$lone$nl# lane: free" "$pass_a"
  own_case lane-lines-alone-and-free 1 "FAIL 10-a (its first 10 lines hold more than one '# lane:' line)" macos
  new_repo && add_script r1 10-a.sh "$lone" "$prelude_any; [ -e \"\$CI_D_REPO/end.30-c\" ] || { printf 'FAIL %s (it started beside the free script 30-c)\\n' \"\$me\"; exit 1; }; $check_passes" &&
    add_script r1 20-b.sh "$good" "$prelude_any; : >\"\$CI_D_REPO/start.\$me\"; w start.30-c || exit 1; : >\"\$CI_D_REPO/end.\$me\"; $check_passes" &&
    add_script r1 30-c.sh "$free" "$prelude_any; : >\"\$CI_D_REPO/start.\$me\"; w end.20-b || exit 1; sleep 2; : >\"\$CI_D_REPO/end.\$me\"; $check_passes"
  own_case alone-part-waits-for-a-free-script-that-outlasts-the-cargo-lane 0 'ci.d macos: 3 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$free" "$pass_a" && add_script r1 20-b.sh "$free" "$pass_b" &&
    mkdir -p "$fixture/target/ci-artifacts" && printf 'a file\n' >"$fixture/target/ci-artifacts/10-a"
  case_check_jobs=1
  case_first='FAIL 10-a (its artifacts directory cannot be made)'
  case_then='PASS 20-b'
  own_case free-check-that-cannot-start-gives-its-place-to-the-next 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$lone" "$pass_a" && add_script r1 20-b.sh "$lone" "$pass_b" &&
    mkdir -p "$fixture/target/ci-artifacts" && printf 'a file\n' >"$fixture/target/ci-artifacts/10-a"
  case_first='FAIL 10-a (its artifacts directory cannot be made)'
  case_then='PASS 20-b'
  own_case alone-check-that-cannot-start-lets-the-next-alone-check-run 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  late_wait='n=0; while [ ! -e "$CI_D_REPO/$1" ] && [ "$n" -lt 300 ]; do sleep 0.1; n=$((n + 1)); done'
  new_repo && add_script r1 10-a.sh "$good" "perl -e 'setpgrp(0, 0); exec @ARGV' sh -c '$late_wait; printf \"late detail of 10-a on stderr\\\\n\" >&2; : >\"\$CI_D_REPO/late-written\"' sh started.30-c >/dev/null & printf '%s\\n' \"\$!\" >>\"\$CI_D_REPO/pids\"; printf 'PASS 10-a\\n'" &&
    add_script r1 20-b.sh "$good" "printf 'PASS 20-b\\n'" &&
    add_script r1 30-c.sh "$good" ": >\"\$CI_D_REPO/started.30-c\"; set -- late-written; $late_wait; printf 'PASS 30-c\\n'"
  case_witness=yes
  case_first="standard error of 10-a after it was printed:${nl}late detail of 10-a on stderr"
  case_then='ci.d macos: 3 passed, 0 failed, 0 skipped'
  own_case standard-error-written-after-the-print-in-the-step-ci.d 0 'late detail of 10-a on stderr' macos
  move_head='r=$CI_D_REPO; t=$(git -C "$r" write-tree); p=$(git -C "$r" rev-parse HEAD); c=$(printf "tree %s\nparent %s\nauthor f <f@example.invalid> 1 +0000\ncommitter f <f@example.invalid> 1 +0000\n\nf\n" "$t" "$p" | git -C "$r" hash-object -t commit -w --stdin); git -C "$r" update-ref HEAD "$c"'
  new_repo && add_script r1 10-a.sh "$good" "$prelude_any; meet || $check_alone; w moved || exit 1; printf '%s\\n' \"\$TMPDIR\" >\"\$CI_D_REPO/tmpdir.10-a\"; $check_passes" &&
    add_script r1 20-b.sh "$free" "$prelude_any; meet || $check_alone; $move_head; : >\"\$CI_D_REPO/moved\"; w tmpdir.10-a || exit 1; read -r t <\"\$CI_D_REPO/tmpdir.10-a\"; n=0; while [ -e \"\$t\" ] && [ \"\$n\" -lt 100 ]; do sleep 0.1; n=\$((n + 1)); done; sleep 1; $check_passes"
  own_case head-moved-beside-another-check-names-both 1 'FAIL run-ci-d (one of 10-a, 20-b moved HEAD)' macos
  new_repo && add_script r1 10-a.sh "$good$nl# lane: free " "$pass_a"
  own_case lane-line-with-a-blank-after-its-value 1 "FAIL 10-a ($lane_refusal)" macos
  new_repo && add_script r1 10-a.sh "$good" "$prelude_any; meet || $check_alone; $check_passes" &&
    add_script r1 20-b.sh "$free$cr" "$prelude_any; meet || $check_alone; $check_passes"
  own_case lane-line-with-a-carriage-return-names-the-free-lane 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  if own_gate checks-without-a-lane-one-at-a-time-beside-the-free-lane; then
  new_repo && add_script r1 10-a.sh "$good" "$prelude_any; $cargo_before; w seen; $cargo_after; $check_passes" &&
    add_script r1 20-b.sh "$good" "$prelude_any; $cargo_before; $cargo_after; $check_passes" &&
    add_script r1 30-c.sh "$free" "$prelude_any; w cargo-start.10-a && [ ! -e \"\$CI_D_REPO/cargo-end.10-a\" ] || $check_alone; : >\"\$CI_D_REPO/seen\"; $check_passes"
  own_case checks-without-a-lane-one-at-a-time-beside-the-free-lane 0 'ci.d macos: 3 passed, 0 failed, 0 skipped' macos
  else
    own_not_run checks-without-a-lane-one-at-a-time-beside-the-free-lane "macos alone: $own_gate_reason"
  fi
  if own_gate lane-line-at-line-11-is-no-lane; then
  new_repo && add_script r1 10-a.sh "$good$nl$seven$nl# lane: free" "$prelude_any; $cargo_before; $cargo_after; $check_passes" &&
    add_script r1 20-b.sh "$good" "$prelude_any; $cargo_before; $cargo_after; $check_passes"
  own_case lane-line-at-line-11-is-no-lane 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  else
    own_not_run lane-line-at-line-11-is-no-lane "macos alone: $own_gate_reason"
  fi
  new_repo &&
    add_script r1 10-a.sh "$free" "$prelude_any; printf \"line 1 of %s\\n\" \"\$me\"; printf \"note of %s\\n\" \"\$me\" >&2; : >\"\$CI_D_REPO/a1\"; w b1 || exit 1; printf \"line 2 of %s\\n\" \"\$me\"; : >\"\$CI_D_REPO/a2\"; $check_passes" &&
    add_script r1 20-b.sh "$free" "$prelude_any; w a1 || exit 1; printf \"line 1 of %s\\n\" \"\$me\"; printf \"note of %s\\n\" \"\$me\" >&2; : >\"\$CI_D_REPO/b1\"; w a2 || exit 1; printf \"line 2 of %s\\n\" \"\$me\"; $check_passes"
  case_first="line 1 of 10-a${nl}line 2 of 10-a${nl}PASS 10-a${nl}note of 10-a${nl}seconds 10-a "
  case_then="line 1 of 20-b${nl}line 2 of 20-b${nl}PASS 20-b${nl}note of 20-b${nl}seconds 20-b "
  own_case outputs-of-free-checks-kept-apart-in-name-order 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$free" "$prelude_any; w end.20-b || $check_alone; printf \"line of %s\\n\" \"\$me\"; $check_passes" &&
    add_script r1 20-b.sh "$good" "$prelude_any; printf \"line of %s\\n\" \"\$me\"; : >\"\$CI_D_REPO/end.\$me\"; $check_passes"
  case_first="line of 10-a${nl}PASS 10-a${nl}seconds 10-a "
  case_then="line of 20-b${nl}PASS 20-b${nl}seconds 20-b "
  own_case output-in-name-order-when-a-later-check-ends-first 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  if own_gate refusal-keeps-its-place-in-the-recap-of-the-step-ci.d; then
  new_repo && add_script r1 10-a.sh "$free" "$prelude_any; sleep 1; $check_passes" &&
    add_script r1 20-x.sh '# rule: A fixture rule.' "$prelude_any; $check_passes" &&
    add_script r1 30-c.sh "$free" "$prelude_any; $check_passes"
  case_first="PASS 10-a${nl}FAIL 20-x (its first 10 lines hold no single '# jobs:' line)${nl}PASS 30-c"
  case_then='ci.d macos: 2 passed, 1 failed, 0 skipped'
  own_case refusal-keeps-its-place-in-the-recap-of-the-step-ci.d 1 "FAIL 20-x (its first 10 lines hold no single '# jobs:' line)" macos
  else
    own_not_run refusal-keeps-its-place-in-the-recap-of-the-step-ci.d "macos alone: $own_gate_reason"
  fi
  two_others="$running; [ \"\$others\" -le 1 ] || { printf \"FAIL %s (two others ran beside it)\\n\" \"\$me\"; exit 1; }"
  if own_gate free-lane-runs-ci-d-check-jobs-at-a-time; then
  new_repo && add_script r1 10-a.sh "$free" "$prelude_any; meet || $check_alone; sleep 1; $two_others; $ends; $check_passes" &&
    add_script r1 20-b.sh "$free" "$prelude_any; $starts; sleep 1; $two_others; $ends; $check_passes" &&
    add_script r1 30-c.sh "$free" "$prelude_any; $starts; sleep 1; $two_others; $ends; $check_passes"
  case_check_jobs=2
  own_case free-lane-runs-ci-d-check-jobs-at-a-time 0 'ci.d macos: 3 passed, 0 failed, 0 skipped' macos
  else
    own_not_run free-lane-runs-ci-d-check-jobs-at-a-time "macos alone: $own_gate_reason"
  fi
  new_repo && add_script r1 10-a.sh "$free" "$prelude_any; $check_passes"
  case_check_jobs=x
  own_case check-jobs-not-a-number 1 'FAIL run-ci-d (CI_D_CHECK_JOBS=x is not a whole number from 1 to 999999999)' macos
  case_check_jobs=0
  own_case check-jobs-zero 1 'FAIL run-ci-d (CI_D_CHECK_JOBS=0 is not a whole number from 1 to 999999999)' macos
  new_repo && add_script r1 10-a.sh "$free" "$prelude_any; meet || $check_alone; $check_passes" && add_script r1 20-b.sh "$free" "$prelude_any; meet || $check_alone; $check_passes"
  case_path=$tmp/no-getconf
  case_procs=2
  own_case default-check-jobs-from-number-of-processors 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$free" "$prelude_any; $alone; $ends; $check_passes" && add_script r1 20-b.sh "$free" "$prelude_any; $alone; $ends; $check_passes"
  case_path=$tmp/no-getconf
  case_procs=none
  own_case default-check-jobs-of-one-without-number-of-processors 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" "$prelude_any; meet || $check_alone; w changed || exit 1; printf '%s\\n' \"\$TMPDIR\" >\"\$CI_D_REPO/tmpdir.10-a\"; $check_passes" &&
    add_script r1 20-b.sh "$free" "$prelude_any; meet || $check_alone; printf 'x\\n' >>\"\$CI_D_REPO/tracked.txt\"; : >\"\$CI_D_REPO/changed\"; w tmpdir.10-a || exit 1; read -r t <\"\$CI_D_REPO/tmpdir.10-a\"; n=0; while [ -e \"\$t\" ] && [ \"\$n\" -lt 100 ]; do sleep 0.1; n=\$((n + 1)); done; sleep 1; $check_passes"
  case_first="PASS 10-a${nl}FAIL run-ci-d (one of 10-a, 20-b changed a tracked file: tracked.txt)"
  case_then='PASS 20-b'
  own_case tracked-file-changed-beside-another-check-names-both 1 'FAIL run-ci-d (one of 10-a, 20-b changed a tracked file: tracked.txt)' macos
  new_repo && track_file && add_script r1 10-a.sh "$good" "$pass_a" &&
    add_script r1 20-b.sh "$good" 'printf "x\n" >>"$CI_D_REPO/tracked.txt"; printf "PASS 20-b\n"'
  case_absent='FAIL run-ci-d (one of 10-a, 20-b changed a tracked file: tracked.txt)'
  own_case tracked-file-changed-by-the-second-check-of-the-cargo-lane-names-it-alone 1 'FAIL run-ci-d (20-b changed a tracked file: tracked.txt)' macos
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 20-b.sh "$good" "$pass_b" &&
    mkdir -p "$fixture/target/ci-artifacts" && printf 'a file\n' >"$fixture/target/ci-artifacts/10-a"
  case_first='FAIL 10-a (its artifacts directory cannot be made)'
  case_then='PASS 20-b'
  own_case check-that-cannot-start-leaves-the-cargo-lane-to-the-next 1 'FAIL 10-a (its artifacts directory cannot be made)' macos
  new_repo && add_script r1 10-a.sh "$good" "$prelude_any; meet || $check_alone; $check_passes" &&
    add_script r1 20-b.sh "$free" "$prelude_any; meet || $check_alone; trap '' TERM; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; wait; $check_passes"
  case_limit=2
  case_max_s=20
  case_first='PASS 10-a'
  case_then='FAIL 20-b (time limit 2 s)'
  own_case free-check-past-its-limit-beside-the-cargo-lane 1 'FAIL 20-b (time limit 2 s)' macos
  new_repo && add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; here=$(pwd); [ "$CI_D_REPO" = "$here" ] && [ "$CI_D_JOB" = windows ] && [ "$CI_D_ARTIFACTS" = "$here/target/ci-artifacts/10-a" ] && [ -d "$CI_D_ARTIFACTS" ] && [ "$LC_ALL" = C ] && [ "${PATH%%:*}" = "$CARGO_HOME/bin" ] && [ "${CALLER_VALUE:-}" = kept ] && [ "${SECOND_CALLER_VALUE:-}" = kept ] && [ "${HOME##*/}" = case-home ] && [ "${job:-}" != from-the-caller ] && ! read -r line || { printf "FAIL self-test:10-a (environment)\n"; exit 1; }; printf "PASS self-test:10-a (1 failing fixtures rejected)\n"'
  case_locale=en_US.UTF-8
  case_env=yes
  own_case environment-of-a-self-test 0 'ci.d windows self-test: 1 passed, 0 failed' windows --self-test 10-a
  for signal in INT QUIT HUP TERM; do
    for place in beside-another alone; do
      sends="kill -s $signal \"\$\$\"; sleep 1; printf 'line after the kill\\n'; if [ \"\${1:-}\" = --self-test ]; then $passes; else printf 'PASS %s\\n' \"\$me\"; fi"
      if [ "$place" = alone ]; then
        new_repo && add_script r1 10-a.sh "$good$nl# self-test: alone" "$prelude_any; $sends" &&
          add_script r1 20-b.sh "$good" "$prelude; $passes"
      else
        new_repo && add_script r1 10-a.sh "$good" "$prelude_any; [ \"\${1:-}\" != --self-test ] || meet || $ran_alone; $sends" &&
          add_script r1 20-b.sh "$good" "$meets"
      fi
      got_out=$(cd "$fixture" && "$perl_bin" -e "$case_perl" sh scripts/r1/ci.d/10-a.sh </dev/null 2>&1)
      got_exit=$?
      foreground="FAIL 10-a (exit $got_exit)"
      [ "$got_exit" -ne 0 ] || foreground=$(printf '%s\n' "$got_out" | tail -n 1)
      after=no
      ! printf '%s\n' "$got_out" | grep -qxF -e 'line after the kill' || after=yes
      case_jobs=2
      case $foreground:$after in
        'PASS 10-a:yes')
          own_not_run "self-test-that-sends-$signal-to-itself-$place-ends-as-in-the-foreground" "the shell runs on after its own $signal on this host"
          case_first='line after the kill'
          case_then='PASS self-test:10-a (1 failing fixtures rejected)'
          own_case "self-test-that-sends-$signal-to-itself-$place-runs-on-as-in-the-foreground" 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
          ;;
        'FAIL 10-a (exit '*'):no')
          own_not_run "self-test-that-sends-$signal-to-itself-$place-runs-on-as-in-the-foreground" "the shell ends at its own $signal on this host"
          case_absent='line after the kill'
          own_case "self-test-that-sends-$signal-to-itself-$place-ends-as-in-the-foreground" 1 "FAIL self-test:10-a (${foreground#FAIL 10-a (}" macos --self-test 10-a 20-b
          ;;
        *)
          printf 'case run-ci-d/self-test-that-sends-%s-to-itself-%s want=PASS got=no-foreground-end-of-%s-%s\n' "$signal" "$place" "${foreground:-none}" "$after"
          wrong="$wrong self-test-that-sends-$signal-to-itself-$place"
          case_jobs=
          ;;
      esac
    done
    case $foreground:$after in
      'PASS 10-a:yes')
        own_not_run "script-that-sends-$signal-to-itself-in-the-step-ci.d-ends-as-in-the-foreground" "the shell runs on after its own $signal on this host"
        case_first='line after the kill'
        case_then='PASS 10-a'
        own_case "script-that-sends-$signal-to-itself-in-the-step-ci.d-runs-on-as-in-the-foreground" 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos 10-a
        ;;
      'FAIL 10-a (exit '*'):no')
        own_not_run "script-that-sends-$signal-to-itself-in-the-step-ci.d-runs-on-as-in-the-foreground" "the shell ends at its own $signal on this host"
        case_absent='line after the kill'
        own_case "script-that-sends-$signal-to-itself-in-the-step-ci.d-ends-as-in-the-foreground" 1 "$foreground" macos 10-a
        ;;
    esac
    new_repo && add_script r1 10-a.sh "$free" "$prelude_any; meet || $check_alone; kill -s $signal \"\$\$\"; sleep 1; printf 'line after the kill\\n'; $check_passes" &&
      add_script r1 20-b.sh "$good" "$prelude_any; meet || $check_alone; $check_passes"
    case $foreground:$after in
      'PASS 10-a:yes')
        own_not_run "free-check-that-sends-$signal-to-itself-ends-as-in-the-foreground" "the shell runs on after its own $signal on this host"
        case_first='line after the kill'
        case_then='PASS 10-a'
        own_case "free-check-that-sends-$signal-to-itself-runs-on-as-in-the-foreground" 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
        ;;
      'FAIL 10-a (exit '*'):no')
        own_not_run "free-check-that-sends-$signal-to-itself-runs-on-as-in-the-foreground" "the shell ends at its own $signal on this host"
        case_absent='line after the kill'
        case_first="$foreground"
        case_then='PASS 20-b'
        own_case "free-check-that-sends-$signal-to-itself-ends-as-in-the-foreground" 1 "$foreground" macos
        ;;
    esac
  done
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes"
  case_path=$tmp/no-perl
  own_case perl-does-not-run 1 "$perl_line" macos --self-test 10-a
  case_path=$tmp/no-perl
  own_case perl-does-not-run-in-the-step-ci.d 1 "$perl_line" macos 10-a
  for signal in INT:130 TERM:143 HUP:129; do
    new_repo && add_script r1 10-a.sh "$good" "sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.10-a\"; wait; printf 'PASS 10-a\\n'" &&
      add_script r1 20-b.sh "$good" "$pass_b"
    case_signal=${signal%:*}
    case_wait=started.10-a
    case_max_s=30
    case_signal_min=2
    case_absent='PASS 20-b'
    own_case "step-ci.d-interrupted-by-${signal%:*}" "${signal#*:}" "FAIL run-ci-d (the step was interrupted by ${signal%:*}; every script that it started is ended)" macos
  done
  for signal in INT:130 TERM:143 HUP:129; do
    new_repo && add_script r1 10-a.sh "$good" "sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.10-a\"; wait; printf 'PASS 10-a\\n'" &&
      add_script r1 20-b.sh "$free" "sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.20-b\"; wait; printf 'PASS 20-b\\n'" &&
      add_script r1 30-c.sh "$good" 'printf "PASS 30-c\n"'
    case_signal=${signal%:*}
    case_wait='started.10-a started.20-b'
    case_max_s=30
    case_signal_min=4
    case_absent='PASS 30-c'
    own_case "step-ci.d-with-a-free-check-interrupted-by-${signal%:*}" "${signal#*:}" "FAIL run-ci-d (the step was interrupted by ${signal%:*}; every script that it started is ended)" macos
  done
  new_repo && add_script r1 10-a.sh "$good" "printf 'line of 10-a\\n'; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.10-a\"; wait; printf 'PASS 10-a\\n'" &&
    add_script r1 20-b.sh "$free" "printf 'line of 20-b\\n'; : >\"\$CI_D_REPO/ended.20-b\"; printf 'PASS 20-b\\n'"
  case_signal=INT
  case_wait='started.10-a ended.20-b'
  case_max_s=30
  case_signal_min=2
  case_first='line of 10-a'
  case_then='line of 20-b'
  own_case output-of-started-checks-printed-when-the-step-is-interrupted 130 'FAIL run-ci-d (the step was interrupted by INT; every script that it started is ended)' macos
  real_git=$(command -v git) || real_git=git
  mkdir -p "$tmp/slow-status" &&
    printf '#!/bin/sh\ncase "$*" in *"status --porcelain"*) : >in-status; %s 3 ;; esac\nexec %s "$@"\n' "$real_sleep" "$real_git" >"$tmp/slow-status/git" &&
    chmod +x "$tmp/slow-status/git"
  new_repo && add_script r1 10-a.sh "$good" "$pass_a"
  case_path=$tmp/slow-status
  case_signal=INT
  case_wait='in-status'
  case_max_s=30
  case_signal_min=0
  own_case interrupt-while-the-driver-reads-the-tracked-files 130 'FAIL run-ci-d (the step was interrupted by INT; every script that it started is ended)' macos
  ignores_term='( trap "" TERM; sleep 60 ) >/dev/null 2>&1 & printf "%s\n" "$!" >>"$CI_D_REPO/pids"'
  new_repo && add_script r1 05-x.sh "$free" "$prelude_any; $ignores_term; w started.10-a && w started.20-b || exit 1; : >\"\$CI_D_REPO/x-ended\"; $check_passes" &&
    add_script r1 10-a.sh "$good" "$prelude_any; : >\"\$CI_D_REPO/started.10-a\"; ( trap '' TERM; w x-ended; sleep 6; : >\"\$CI_D_REPO/signal-now\"; sleep 60 ) >/dev/null 2>&1 & printf '%s\\n' \"\$!\" >>\"\$CI_D_REPO/pids\"; w x-ended || exit 1; sleep 1; $check_passes" &&
    add_script r1 20-b.sh "$free" "$prelude_any; : >\"\$CI_D_REPO/started.20-b\"; sleep 60 >/dev/null 2>&1 & printf '%s\\n' \"\$!\" >>\"\$CI_D_REPO/pids\"; w x-ended || exit 1; sleep 1; $check_passes"
  case_check_jobs=2
  case_signal=INT
  case_wait='signal-now'
  case_max_s=40
  case_signal_min=2
  own_case group-of-a-check-that-ended-and-is-not-yet-finished-is-ended-by-an-interrupt 130 'FAIL run-ci-d (the step was interrupted by INT; every script that it started is ended)' macos
  new_repo && track_file &&
    add_script r1 10-a.sh "$good" "$prelude_any; printf 'x\\n' >>\"\$CI_D_REPO/tracked.txt\"; : >\"\$CI_D_REPO/dirty\"; w tmpdir.20-b || exit 1; read -r t <\"\$CI_D_REPO/tmpdir.20-b\"; n=0; while [ -e \"\$t\" ] && [ \"\$n\" -lt 100 ]; do sleep 0.1; n=\$((n + 1)); done; sleep 1; git -C \"\$CI_D_REPO\" checkout -q -- tracked.txt; $check_passes" &&
    add_script r1 20-b.sh "$free" "$prelude_any; w dirty || exit 1; printf '%s\\n' \"\$TMPDIR\" >\"\$CI_D_REPO/tmpdir.20-b\"; $check_passes"
  case_first='FAIL run-ci-d (one of 10-a, 20-b changed a tracked file: tracked.txt)'
  case_then='ci.d macos: 2 passed, 1 failed, 0 skipped'
  case_absent='FAIL run-ci-d (10-a changed a tracked file: tracked.txt)'
  own_case change-undone-after-its-report-is-not-reported-again 1 'ci.d macos: 2 passed, 1 failed, 0 skipped' macos
  for signal in INT:130 TERM:143 HUP:129; do
    new_repo && add_script r1 10-a.sh "$good" "$prelude; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.\$me\"; wait; $passes" &&
      add_script r1 20-b.sh "$good" "$prelude; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.\$me\"; wait; $passes"
    case_jobs=2
    case_signal=${signal%:*}
    case_wait='started.10-a started.20-b'
    case_max_s=30
    case_signal_min=4
    own_case "step-interrupted-by-${signal%:*}" "${signal#*:}" "FAIL run-ci-d (the step was interrupted by ${signal%:*}; every self-test that it started is ended)" macos --self-test 10-a 20-b
  done
  new_repo && add_script r1 10-a.sh "$good" "$prelude; ( set -- start.20-b; $late_wait; printf 'late detail of %s on stderr\\n' \"\$me\" >&2; : >\"\$CI_D_REPO/late-written\" ) >/dev/null & $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $starts; set -- late-written; $late_wait; [ -e \"\$CI_D_REPO/late-written\" ] || exit 1; $passes"
  case_jobs=1
  case_first="standard error of 10-a after it was printed:${nl}late detail of 10-a on stderr"
  case_then='ci.d macos self-test: 2 passed, 0 failed'
  own_case standard-error-written-after-the-print 0 'late detail of 10-a on stderr' macos --self-test 10-a 20-b

  own_list_case lists-of-the-macos-alone-cases-and-of-the-windows-rules
  printf 'run-ci-d own cases: %s cases, %s s\n' "$own_n" "$(($(date +%s) - own_start))"
  if [ -n "$wrong" ]; then
    record FAIL "FAIL self-test:run-ci-d (wrong verdict in:$wrong)"
  elif [ "$rejected" -eq 0 ]; then
    record FAIL "FAIL self-test:run-ci-d (0 failing fixtures rejected)"
  else
    record PASS "PASS self-test:run-ci-d ($rejected failing fixtures rejected)"
  fi
}

st_n=0
st_next=0
st_running=' '
st_count=0
st_naps=0
st_alone=' '
st_list=$root/ci/self-test-alone.txt
st_d=
st_p=
st_t=
st_k=
st_x=
st_l=
st_word=
st_nm=
st_sleep=
st_beside=' '
st_ending=
st_line=
st_q_file=
st_q_name=
st_pk=
st_f_d=
st_f_name=
st_f_lane=
st_f_td=
st_td=
st_lm=
st_f_other=
tree_to=
cr=$(printf '\r')
tab=$(printf '\t')
processors() {
  procs_n=$(getconf _NPROCESSORS_ONLN 2>/dev/null) || procs_n=
  whole_number "$procs_n" || procs_n=${NUMBER_OF_PROCESSORS:-1}
  whole_number "$procs_n" || procs_n=1
}
if [ "$mode" = self-test ]; then
  if [ -z "$st_jobs_set" ]; then
    processors
    st_jobs=$procs_n
  else
    whole_number "$st_jobs" || {
      printf 'FAIL run-ci-d (CI_D_SELF_TEST_JOBS=%s is not a whole number from 1 to 999999999)\n' "$st_jobs"
      exit 1
    }
  fi
elif [ -z "$ck_jobs_set" ]; then
  processors
  ck_jobs=$procs_n
else
  whole_number "$ck_jobs" || {
    printf 'FAIL run-ci-d (CI_D_CHECK_JOBS=%s is not a whole number from 1 to 999999999)\n' "$ck_jobs"
    exit 1
  }
fi
whole_number "$st_limit" || {
  printf 'FAIL run-ci-d (CI_D_SCRIPT_LIMIT_S=%s is not a whole number from 1 to 999999999)\n' "$st_limit"
  exit 1
}
"$perl_cmd" -e "$st_perl" sh -c 'exit 0' </dev/null >/dev/null 2>&1 || {
  printf 'FAIL run-ci-d (perl does not run; the driver starts each script through perl, for INT and QUIT at their defaults and a process group of its own)\n'
  exit 1
}
st_what=script
trap 'st_interrupt INT 130' USR1
trap 'st_interrupt TERM 143' TERM
trap 'st_interrupt HUP 129' HUP
if [ "$mode" = self-test ]; then
  st_what='self-test'
  if [ -e "$st_list" ] || [ -L "$st_list" ]; then
    { [ -f "$st_list" ] && [ -r "$st_list" ]; } || {
      printf 'FAIL run-ci-d (%s cannot be read)\n' "$st_list"
      exit 1
    }
    while IFS= read -r st_line || [ -n "$st_line" ]; do
      st_entry "$st_line"
      st_alone="$st_alone$st_word "
    done <"$st_list" || {
      printf 'FAIL run-ci-d (%s cannot be read)\n' "$st_list"
      exit 1
    }
  fi
fi

tree_start
ran=' '
if [ "$mode" = self-test ]; then
  ran=' run-ci-d '
  case " $wanted " in
    '  ' | *' run-ci-d '*) st_own ;;
  esac
fi

for file in "$root"/*/ci.d/* "$root"/*/ci.d/.*; do
  is_entry "$file" || continue
  if ! inspect "$file"; then
    note "FAIL $script_name ($refusal)"
    continue
  fi
  names_job "$job" || continue
  if [ -n "$wanted" ]; then
    case " $wanted " in
      *" $script_name "*) ;;
      *) continue ;;
    esac
  fi
  ran="$ran$script_name "
  selected=$((selected + 1))
  if [ "$mode" = self-test ]; then
    st_queue
  else
    st_add "$script_file" "$script_name" "$script_lane" "$script_limit"
  fi
done

if [ "$mode" = self-test ]; then
  if [ -e "$st_list" ]; then
    while IFS= read -r st_line || [ -n "$st_line" ]; do
      st_entry "$st_line"
      case $st_word in
        '' | '#'*) continue ;;
      esac
      [ -n "$st_reason" ] || note "FAIL run-ci-d ($st_list names $st_word without a reason)"
      case $seen in
        *" $st_word "*) ;;
        *) note "FAIL run-ci-d ($st_list names $st_word, which is no script)" ;;
      esac
    done <"$st_list"
  fi
  st_wait_below 1
  st_flush
  wait
  st_groups=
  st_i=0
  while [ "$st_i" -lt "$st_n" ]; do
    eval "st_k=\$st_kind_$st_i st_d=\${st_dir_$st_i-}"
    st_sp=
    [ "$st_k" != script ] || [ ! -s "$st_d/pid" ] || read -r st_sp <"$st_d/pid"
    [ -z "$st_sp" ] || st_groups="$st_groups $st_sp"
    st_i=$((st_i + 1))
  done
  [ -z "$st_groups" ] || st_end_group 50 "$st_groups"
  st_i=0
  while [ "$st_i" -lt "$st_n" ]; do
    eval "st_k=\$st_kind_$st_i"
    [ "$st_k" != script ] || st_leave "$st_i"
    st_i=$((st_i + 1))
  done
else
  st_lanes
fi
st_i=0
while [ "$st_i" -lt "$st_n" ]; do
  eval "st_k=\$st_kind_$st_i"
  if [ "$st_k" != note ]; then
    eval "st_d=\$st_dir_$st_i st_e=\${st_errsize_$st_i:-0} st_nm=\$st_name_$st_i"
    st_now_e=0
    [ ! -e "$st_d/err" ] || st_now_e=$(wc -c <"$st_d/err")
    st_now_e=${st_now_e##* }
    if [ "$st_now_e" -gt "$st_e" ]; then
      printf 'standard error of %s after it was printed:\n' "$st_nm" >&2
      tail -c +"$((st_e + 1))" "$st_d/err" >&2
    fi
  fi
  st_i=$((st_i + 1))
done
[ "$mode" != self-test ] || tree_check 'the step ci.d self-test'

for name in $wanted; do
  case $ran in
    *" $name "*) ;;
    *)
      printf 'FAIL run-ci-d (no script %s for %s)\n' "$name" "$job"
      status=1
      ;;
  esac
done

if [ "$selected" -eq 0 ] && [ "$failed" -eq 0 ] && [ "$wanted" != ' run-ci-d' ]; then
  [ "$status" -ne 0 ] || printf 'FAIL run-ci-d (no scripts for %s)\n' "$job"
  exit 1
fi

cat "$recap"
if [ "$mode" = self-test ]; then
  printf 'ci.d %s self-test: %s passed, %s failed\n' "$job" "$passed" "$failed"
else
  printf 'ci.d %s: %s passed, %s failed, %s skipped\n' "$job" "$passed" "$failed" "$skipped"
fi
if [ "$failed" -gt 0 ] || [ "$passed" -eq 0 ]; then
  status=1
fi
lock_changed=
for lock in Cargo.lock package-lock.json; do
  lock_head=$(own_git rev-parse -q --verify "HEAD:$lock" 2>/dev/null)
  lock_tree=
  [ ! -e "$lock" ] || lock_tree=$(own_git hash-object -- "$lock" 2>/dev/null) || lock_tree=unreadable
  [ "$lock_head" = "$lock_tree" ] || lock_changed="$lock_changed $lock"
done
if ! own_git diff --exit-code HEAD -- Cargo.lock package-lock.json || [ -n "$lock_changed" ]; then
  printf 'FAIL run-ci-d (Cargo.lock or package-lock.json differs from HEAD; a script builds with --locked and changes no tracked file)\n'
  status=1
fi
exit "$status"
