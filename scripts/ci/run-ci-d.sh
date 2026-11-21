#!/bin/sh
set -u
LC_ALL=C
export LC_ALL

usage() {
  cat <<'EOF'
usage: sh scripts/ci/run-ci-d.sh <job> [--self-test] [NAME...]
       sh scripts/ci/run-ci-d.sh <job> --merge-gate
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
st_perl="setpgrp(0, 0); my \$f = delete \$ENV{RUN_CI_D_PID_FILE}; if (defined \$f) { open(my \$h, '>', \$f) or exit 127; print \$h \"\$\$\\n\"; close(\$h) or exit 127; } \$SIG{INT} = \$SIG{QUIT} = 'DEFAULT'; exec { \$ARGV[0] } @ARGV; exit 127;"
mode=run
job=
wanted=
gate=
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
        --merge-gate) gate=yes ;;
        '' | -* | *[!a-z0-9-]*) usage >&2; exit 64 ;;
        *) wanted="$wanted $arg" ;;
      esac
    done
    if [ -n "$gate" ] && { [ "$mode" != run ] || [ -n "$wanted" ]; }; then
      usage >&2
      exit 64
    fi
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
unset CI_D_MERGE_GATE
if [ -n "$gate" ]; then
  CI_D_MERGE_GATE=1
  export CI_D_MERGE_GATE
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
  st_q_solo=$(sed -n '1,10s/^# self-test: alone$/alone/p' "$script_file")
  case $st_alone in
    *" $script_name "*) st_q_solo=alone ;;
  esac
  st_add "$script_file" "$script_name" "$script_lane" "$script_limit"
  eval "st_solo_$((st_n - 1))=$st_q_solo"
  st_poll
  st_fill
  st_flush
}

st_pick() {
  st_pick_i=0
  st_pick_alone=
  while [ "$st_pick_i" -lt "$st_n" ]; do
    eval "st_pk=\$st_kind_$st_pick_i:\$st_state_$st_pick_i:\${st_solo_$st_pick_i-}"
    case $st_pk in
      script:waiting:) return 0 ;;
      script:waiting:alone) [ -n "$st_pick_alone" ] || st_pick_alone=$st_pick_i ;;
    esac
    st_pick_i=$((st_pick_i + 1))
  done
  [ -n "$st_ready" ] && [ "$st_count" -eq 0 ] && [ -n "$st_pick_alone" ] || return 1
  st_pick_i=$st_pick_alone
}

st_fill() {
  while [ "$st_count" -lt "$st_jobs" ] && st_pick; do
    st_start "$st_pick_i" || continue
    st_count=$((st_count + 1))
  done
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
  st_sp=
  [ ! -s "$st_d/pid" ] || read -r st_sp <"$st_d/pid"
  if [ -n "$st_sp" ]; then
    st_end_group 0 "$st_sp"
  else
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

st_n=0
st_next=0
st_running=' '
st_count=0
st_naps=0
st_alone=' '
st_list=$root/ci/self-test-alone.txt
gate_list=$root/ci/merge-gate.txt
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
st_ready=
st_v_k=
st_v_x=
st_v_d=
st_v_lm=
st_pick_i=
st_pick_alone=
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

if [ -n "$gate" ]; then
  { [ -f "$gate_list" ] && [ -r "$gate_list" ]; } || {
    printf 'FAIL run-ci-d (%s cannot be read)\n' "$gate_list"
    exit 1
  }
  gate_n=0
  while IFS= read -r st_line || [ -n "$st_line" ]; do
    gate_n=$((gate_n + 1))
    st_entry "$st_line"
    case $st_word in
      '' | '#'*) continue ;;
    esac
    case $st_word:$st_reason in
      macos:*[!a-z0-9-]* | windows:*[!a-z0-9-]* | macos: | windows:) gate_n=-$gate_n ;;
      macos:* | windows:*) ;;
      *) gate_n=-$gate_n ;;
    esac
    if [ "$gate_n" -lt 0 ]; then
      printf 'FAIL run-ci-d (%s line %s is not a job, macos or windows, a blank and the name of a check)\n' "$gate_list" "${gate_n#-}"
      exit 1
    fi
    [ "$st_word" != "$job" ] || wanted="$wanted $st_reason"
  done <"$gate_list" || {
    printf 'FAIL run-ci-d (%s cannot be read)\n' "$gate_list"
    exit 1
  }
  [ -n "$wanted" ] || {
    printf 'FAIL run-ci-d (%s names no check for %s)\n' "$gate_list" "$job"
    exit 1
  }
fi

tree_start
ran=' '

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
  st_ready=yes
  while :; do
    st_poll
    st_fill
    st_flush
    [ "$st_running" != ' ' ] || break
    st_nap
  done
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
  if [ -n "$gate" ]; then
    for name in $wanted; do
      case $ran in
        *" $name "*) ;;
        *)
          printf 'FAIL run-ci-d (%s names %s, which is no check of the job %s)\n' "$gate_list" "$name" "$job"
          status=1
          ;;
      esac
    done
    [ "$status" -eq 0 ] || exit 1
  fi
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

if [ "$selected" -eq 0 ] && [ "$failed" -eq 0 ]; then
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
