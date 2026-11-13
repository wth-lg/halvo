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
st_limit=${CI_D_SCRIPT_LIMIT_S-1200}
st_perl="setpgrp(0, 0); \$SIG{INT} = \$SIG{QUIT} = 'DEFAULT'; exec { \$ARGV[0] } @ARGV; exit 127;"
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

# shellcheck disable=SC2329
st_top() {
  trap '' INT TERM
  if [ -n "$st_worker" ]; then
    kill -s "$3" "$st_worker" 2>/dev/null
    wait "$st_worker"
  fi
  exit "$2"
}

if [ "$mode" = self-test ] && [ "${RUN_CI_D_WORKER:-}" != "$PPID" ]; then
  st_worker=
  trap 'st_top INT 130 USR1' INT
  trap 'st_top TERM 143 TERM' TERM
  RUN_CI_D_WORKER=$$ sh "$0" "$job" "$@" &
  st_worker=$!
  wait "$st_worker"
  exit "$?"
fi

cdup=$(git rev-parse --show-cdup 2>/dev/null)
code=$?
if [ "$code" -ne 0 ] || [ -n "$cdup" ]; then
  printf 'FAIL run-ci-d (start it at the root of the git work tree)\n'
  exit 1
fi
repo=$(pwd)

inspect() {
  script_file=$1
  script_name=${script_file##*/}
  script_name=${script_name%.sh}
  script_jobs=
  script_rule=
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
recap=$tmp/recap
: >"$recap"
passed=0
failed=0
skipped=0
selected=0
status=0

cargo_home=${CARGO_HOME:-$HOME/.cargo}
if command -v cygpath >/dev/null 2>&1; then
  cargo_home=$(cygpath -u "$cargo_home")
fi
PATH=$cargo_home/bin:$PATH
export PATH

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

run_script() {
  out=$tmp/$script_name.out
  code_file=$tmp/$script_name.code
  artifacts=$repo/target/ci-artifacts/$script_name
  mkdir -p "$artifacts" || {
    record FAIL "FAIL $script_name (its artifacts directory cannot be made)"
    return
  }
  started=$(date +%s)
  (
    CI_D_REPO=$repo
    CI_D_JOB=$job
    CI_D_ARTIFACTS=$artifacts
    export CI_D_REPO CI_D_JOB CI_D_ARTIFACTS
    sh "$script_file" </dev/null
    printf '%s\n' "$?" >"$code_file"
  ) | tee "$out"
  ended=$(date +%s)
  printf 'seconds %s %s\n' "$script_name" "$((ended - started))"
  code=$(cat "$code_file" 2>/dev/null)
  last=$(tail -n 1 "$out")
  judge
}

note() {
  if [ "$mode" = self-test ]; then
    eval "st_kind_$st_n=note st_state_$st_n=done st_line_$st_n=\$1"
    st_n=$((st_n + 1))
  else
    record FAIL "$1"
  fi
}

st_track() {
  st_now=$(date +%s)
  eval "st_kind_$st_n=\$1 st_pid_$st_n=\$2 st_dir_$st_n=\$3 st_name_$st_n=\$4 st_since_$st_n=\$st_now st_state_$st_n=running"
  st_running="$st_running$st_n "
  st_n=$((st_n + 1))
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
  st_track own "$!" "$st_d" run-ci-d
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
  st_q_artifacts=$repo/target/ci-artifacts/$st_q_name
  mkdir -p "$st_q_artifacts" || {
    note "FAIL $st_q_name (its artifacts directory cannot be made)"
    return
  }
  st_d=$tmp/self-test.$st_q_name
  mkdir -p "$st_d/tmp" || {
    note "FAIL self-test:$st_q_name (its temporary directory cannot be made)"
    return
  }
  (
    CI_D_REPO=$repo
    CI_D_JOB=$job
    CI_D_ARTIFACTS=$st_q_artifacts
    TMPDIR=$st_d/tmp
    export CI_D_REPO CI_D_JOB CI_D_ARTIFACTS TMPDIR
    st_t0=$(date +%s)
    {
      perl -e "$st_perl" sh "$st_q_file" --self-test </dev/null 2>"$st_d/err" &
      printf '%s\n' "$!" >"$st_d/pid"
      wait "$!"
      printf '%s\n' "$?" >"$st_d/status"
    } | cat >"$st_d/out"
    st_t1=$(date +%s)
    st_c=none
    [ ! -s "$st_d/status" ] || read -r st_c <"$st_d/status"
    printf '%s %s\n' "$st_c" "$((st_t1 - st_t0))" >"$st_d/code"
    : >"$st_d/done"
  ) </dev/null >/dev/null 2>&1 &
  st_track script "$!" "$st_d" "$st_q_name"
  [ -z "$st_q_solo" ] || st_wait_below 1
}

# shellcheck disable=SC2329
st_group() {
  perl -e "exit(kill(\$ARGV[0], -\$ARGV[1]) ? 0 : 1)" "$1" "$2" 2>/dev/null
}

# shellcheck disable=SC2329
st_interrupt() {
  trap '' INT TERM
  [ -z "$st_sleep" ] || kill -s TERM "$st_sleep" 2>/dev/null
  st_groups=
  for st_i in $st_running; do
    eval "st_d=\$st_dir_$st_i st_p=\$st_pid_$st_i"
    st_sp=
    [ ! -s "$st_d/pid" ] || read -r st_sp <"$st_d/pid"
    [ -z "$st_sp" ] || st_groups="$st_groups $st_sp"
    kill -s TERM "$st_p" 2>/dev/null
  done
  st_sp=
  [ ! -s "$tmp/nested.pid" ] || read -r st_sp <"$tmp/nested.pid"
  [ -z "$st_sp" ] || st_groups="$st_groups $st_sp"
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
  printf 'FAIL run-ci-d (the step was interrupted by %s; every self-test that it started is ended)\n' "$1"
  exit "$2"
}

st_stop() {
  eval "st_d=\$st_dir_$1 st_p=\$st_pid_$1 st_state_$1=timeout"
  st_sp=
  [ ! -s "$st_d/pid" ] || read -r st_sp <"$st_d/pid"
  kill -TERM "${st_sp:-$st_p}" 2>/dev/null
  st_grace=0
  while [ ! -e "$st_d/done" ] && kill -0 "$st_p" 2>/dev/null && [ "$st_grace" -lt 50 ]; do
    sleep 0.1
    st_grace=$((st_grace + 1))
  done
  if [ ! -e "$st_d/done" ]; then
    [ -z "$st_sp" ] || kill -KILL "$st_sp" 2>/dev/null
    kill -KILL "$st_p" 2>/dev/null
  fi
  wait "$st_p" 2>/dev/null
}

st_poll() {
  st_now=$(date +%s)
  st_left=' '
  st_count=0
  for st_i in $st_running; do
    eval "st_d=\$st_dir_$st_i st_p=\$st_pid_$st_i st_t=\$st_since_$st_i"
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
    elif [ "$((st_now - st_t))" -ge "$st_limit" ]; then
      st_stop "$st_i"
      st_naps=0
    else
      st_left="$st_left$st_i "
      st_count=$((st_count + 1))
    fi
  done
  st_running=$st_left
}

st_print() {
  eval "st_d=\$st_dir_$1 st_k=\$st_kind_$1 st_x=\$st_state_$1 st_t=\$st_since_$1 script_name=\$st_name_$1"
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
      timeout:*) record FAIL "FAIL self-test:run-ci-d (time limit $st_limit s)" ;;
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
    timeout:*) record FAIL "FAIL self-test:$script_name (time limit $st_limit s)" ;;
    lost:* | *:none) record FAIL "FAIL self-test:$script_name (it ended without an exit status)" ;;
    *) judge ;;
  esac
}

st_flush() {
  while [ "$st_next" -lt "$st_n" ]; do
    eval "st_k=\$st_kind_$st_next st_x=\$st_state_$st_next"
    if [ "$st_k" = note ]; then
      eval "st_l=\$st_line_$st_next"
      record FAIL "$st_l"
    elif [ "$st_x" = running ]; then
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
    if [ "$st_naps" -lt 10 ]; then
      sleep 0.1 &
    else
      sleep 0.5 &
    fi
    st_sleep=$!
    wait "$st_sleep"
    st_sleep=
    st_naps=$((st_naps + 1))
  done
}

whole_number() {
  case $1 in
    '' | 0* | *[!0-9]* | ??????????*) return 1 ;;
  esac
}

st_first_word() {
  st_word=${1%"$cr"}
  st_word=${st_word#"${st_word%%[![:blank:]]*}"}
  st_word=${st_word%%[[:blank:]]*}
}

new_repo() {
  fixture=$(mktemp -d "$tmp/repo.XXXXXX") || return 1
  git -C "$fixture" init -q >/dev/null 2>&1 || return 1
  printf 'version = 4\n' >"$fixture/Cargo.lock"
  git -C "$fixture" add Cargo.lock >/dev/null 2>&1 && fixture_commit
}

fixture_commit() {
  fixture_tree=$(git -C "$fixture" write-tree) || return 1
  fixture_head=$(printf 'tree %s\nauthor ci.d fixture <fixture@example.invalid> 1 +0000\ncommitter ci.d fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$fixture_tree" | git -C "$fixture" hash-object -t commit -w --stdin) || return 1
  git -C "$fixture" update-ref HEAD "$fixture_head"
}

add_script() {
  mkdir -p "$fixture/scripts/$1/ci.d" || return 1
  printf '#!/bin/sh\n%s\n%s\n' "$3" "$4" >"$fixture/scripts/$1/ci.d/$2"
}

own_case() {
  case_name=$1
  want_exit=$2
  want_line=$3
  shift 3
  own_run "$@"
  own_judge
}

own_run() {
  [ -z "$case_max_s" ] || case_t0=$(date +%s)
  got_out=$(
    cd "$fixture/$case_cd" || exit 99
    unset CI CI_D_SCRIPTS CI_D_SELF_TEST_JOBS CI_D_SCRIPT_LIMIT_S
    [ -z "$case_ci" ] || { CI=$case_ci; export CI; }
    [ -z "$case_scripts" ] || { CI_D_SCRIPTS=$case_scripts; export CI_D_SCRIPTS; }
    [ -z "$case_locale" ] || { LC_ALL=$case_locale; export LC_ALL; }
    [ -z "$case_jobs" ] || { CI_D_SELF_TEST_JOBS=$case_jobs; export CI_D_SELF_TEST_JOBS; }
    [ -z "$case_limit" ] || { CI_D_SCRIPT_LIMIT_S=$case_limit; export CI_D_SCRIPT_LIMIT_S; }
    [ -z "$case_no_perl" ] || { PATH=$tmp/no-perl:$PATH; export PATH; }
    CARGO_HOME=$tmp/cargo-home
    TMPDIR=${case_tmpdir:-$tmp}
    export CARGO_HOME TMPDIR
    printf 'a line on standard input\n' | RUN_CI_D_NESTED=${case_nested:-yes} "$perl_bin" -e "$case_perl" sh "$self" "$@" 2>&1 &
    case_pid=$!
    printf '%s\n' "$case_pid" >"$tmp/nested.pid"
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
  : >"$tmp/nested.pid"
  [ -z "$case_max_s" ] || case_t1=$(date +%s)
  left=0
  if [ -n "$case_signal" ] && [ -f "$fixture/pids" ]; then
    n=0
    while :; do
      left=0
      while IFS= read -r case_left; do
        [ -z "$case_left" ] || ! kill -0 "$case_left" 2>/dev/null || left=$((left + 1))
      done <"$fixture/pids"
      [ "$left" -gt 0 ] && [ "$n" -lt 30 ] || break
      sleep 0.1
      n=$((n + 1))
    done
  fi
}

own_judge() {
  before=
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
  elif [ "$left" -gt 0 ]; then
    got=$left-processes-left
  elif [ -n "$case_signal" ] && [ "${before:-0}" -lt "$case_signal_min" ]; then
    got=${before:-no}-processes-before-the-signal
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
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  case_ci=
  case_scripts=
  case_cd=
  case_locale=
  case_tmpdir=
  case_jobs=
  case_limit=
  case_first=
  case_then=
  case_max_s=
  case_nested=
  case_no_perl=
  case_signal=
  case_wait=
  case_absent=
  case_signal_min=
}

# shellcheck disable=SC2016
own_self_test() {
  good="# jobs: macos windows$nl# rule: A fixture rule."
  seven="#$nl#$nl#$nl#$nl#$nl#$nl#"
  pass_a='[ $# -eq 0 ] || exit 3; printf "PASS 10-a\n"'
  pass_b='printf "PASS 20-b\n"'
  fail_a='printf "FAIL 10-a (planned)\n"; exit 1'
  skip_a='printf "SKIP 10-a (no tool)\n"; exit 77'
  rejected=0
  wrong=
  case_ci=
  case_scripts=
  case_cd=
  case_locale=
  case_tmpdir=
  case_jobs=
  case_limit=
  case_first=
  case_then=
  case_max_s=
  case_nested=
  case_no_perl=
  case_signal=
  case_wait=
  case_absent=
  case_signal_min=
  perl_bin=$(command -v perl) || perl_bin=perl
  case_perl="setpgrp(0, 0); \$SIG{QUIT} = 'DEFAULT'; \$SIG{INT} = 'DEFAULT'; exec { \$ARGV[0] } @ARGV; exit 127;"
  mkdir -p "$tmp/no-perl" && printf '#!/bin/sh\nexit 1\n' >"$tmp/no-perl/perl" && chmod +x "$tmp/no-perl/perl"
  usage_line='usage: sh scripts/ci/run-ci-d.sh <job> [--self-test] [NAME...]'
  name_refusal='the file name is not NN-name.sh, two digits and a name of a-z, 0-9 and -'
  guard_line='FAIL run-ci-d (Cargo.lock or package-lock.json differs from HEAD; a script builds with --locked and changes no tracked file)'

  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case all-pass 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  if [ -n "${RUN_CI_D_NESTED:-}" ]; then
    [ "$RUN_CI_D_NESTED" != fail ] || wrong="$wrong nested-run-asked-to-fail"
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
  fi
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r1 .20-hidden.sh "$good" 'printf "PASS .20-hidden\n"'
  own_case hidden-entry 1 "FAIL .20-hidden ($name_refusal)" macos
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
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && add_script r2 10-a.sh "$good" "$pass_a"
  own_case name-used-twice 1 'FAIL 10-a (a second script has this name: scripts/r2/ci.d/10-a.sh)' macos
  own_case rules-name-used-twice 1 'FAIL 10-a (a second script has this name: scripts/r2/ci.d/10-a.sh)' --rules

  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS 10-a\n"; exit 1'
  own_case pass-line-with-exit-1 1 'FAIL 10-a (exit 1)' macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "done\n"'
  own_case exit-0-without-pass-line 1 'FAIL 10-a (exit 0)' macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS 10-a (3 checks)\n"'
  own_case pass-line-with-a-tail 1 'FAIL 10-a (exit 0)' macos
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS 10-a\n"; exit 77'
  own_case pass-line-with-exit-77 1 'FAIL 10-a (exit 77)' macos
  new_repo && add_script r1 10-a.sh "$good" 'if read -r line; then printf "FAIL 10-a (it read a line)\n"; exit 1; fi; printf "PASS 10-a\n"'
  own_case no-standard-input 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" '[ "$CI_D_JOB" = windows ] || { printf "FAIL 10-a (job)\n"; exit 1; }; printf "PASS 10-a\n"'
  own_case job-handed-to-the-script 0 'ci.d windows: 1 passed, 0 failed, 0 skipped' windows
  new_repo && add_script r1 10-a.sh "$good" "$pass_a" && printf 'a file\n' >"$fixture/target"
  own_case artifacts-directory-cannot-be-made 1 'FAIL 10-a (its artifacts directory cannot be made)' macos
  new_repo && add_script r1 10-a.sh "$good" "$fail_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case fail-does-not-stop-the-run 1 'ci.d macos: 1 passed, 1 failed, 0 skipped' macos
  own_case name-selects-one 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos 20-b
  new_repo && add_script r1 10-a.sh "$good" "$skip_a" && add_script r1 20-b.sh "$good" "$pass_b"
  own_case skip-outside-ci 0 'ci.d macos: 1 passed, 0 failed, 1 skipped' macos
  case_ci=true
  own_case skip-in-ci 1 'FAIL 10-a (skipped in CI: no tool)' macos
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

  new_repo && add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; printf "PASS self-test:10-a (2 failing fixtures rejected)\n"'
  own_case self-test-rejects-two 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS self-test:10-a (0 failing fixtures rejected)\n"'
  own_case self-test-rejects-nothing 1 'FAIL self-test:10-a (0 failing fixtures rejected)' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS self-test:10-a (no failing fixtures rejected)\n"'
  own_case self-test-count-is-no-number 1 'FAIL self-test:10-a (exit 0)' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" 'printf "FAIL self-test:10-a (planned)\n"; exit 1'
  own_case self-test-fails 1 'FAIL self-test:10-a (planned)' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" 'exit 2'
  own_case self-test-exits-2 1 'FAIL self-test:10-a (exit 2)' macos --self-test 10-a
  new_repo && add_script r1 10-a.sh "$good" 'printf "PASS self-test:10-a (2 failing fixtures rejected)\n"; exit 1'
  own_case self-test-pass-line-with-exit-1 1 'FAIL self-test:10-a (exit 1)' macos --self-test 10-a

  new_repo && add_script B 10-x.sh "$good" ': >"$CI_D_REPO/first-ran"; printf "PASS 10-x\n"' &&
    add_script a 20-y.sh "$good" '[ -e "$CI_D_REPO/first-ran" ] || { printf "FAIL 20-y (it ran first)\n"; exit 1; }; printf "PASS 20-y\n"'
  case_locale=en_US.UTF-8
  own_case byte-order 0 'ci.d macos: 2 passed, 0 failed, 0 skipped' macos
  new_repo && add_script r1 10-a.sh "$good" 'here=$(pwd); [ "$CI_D_REPO" = "$here" ] && [ "$CI_D_JOB" = macos ] && [ "$CI_D_ARTIFACTS" = "$here/target/ci-artifacts/10-a" ] && [ -d "$CI_D_ARTIFACTS" ] && [ "$LC_ALL" = C ] && [ "${PATH%%:*}" = "$CARGO_HOME/bin" ] || { printf "FAIL 10-a (environment)\n"; exit 1; }; printf "PASS 10-a\n"'
  case_locale=en_US.UTF-8
  own_case environment 0 'ci.d macos: 1 passed, 0 failed, 0 skipped' macos

  new_repo && add_script r1 10-a.sh "$good" '[ "${1:-}" = --self-test ] || exit 3; printf "PASS self-test:10-a (2 failing fixtures rejected)\n"'
  own_case bare-self-test-runs-the-own-cases 0 'PASS self-test:run-ci-d (a nested run stops after its first case)' macos --self-test
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  own_case self-test-without-scripts 1 'FAIL run-ci-d (no scripts for macos)' macos --self-test
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  own_case self-test-of-the-driver-alone 0 'ci.d macos self-test: 1 passed, 0 failed' macos --self-test run-ci-d

  prelude='[ "${1:-}" = --self-test ] || exit 3; me=${0##*/}; me=${me%.sh}; w() { n=0; while [ ! -e "$CI_D_REPO/$1" ]; do [ "$n" -lt 100 ] || return 1; sleep 0.1; n=$((n + 1)); done; }; meet() { : >"$CI_D_REPO/start.$me"; n=0; while [ "$n" -lt 100 ]; do for f in "$CI_D_REPO"/start.*; do [ "$f" = "$CI_D_REPO/start.$me" ] || [ ! -e "$f" ] || return 0; done; sleep 0.1; n=$((n + 1)); done; return 1; }'
  passes='printf "PASS self-test:%s (1 failing fixtures rejected)\n" "$me"'
  ran_alone='{ printf "FAIL self-test:%s (it ran alone)\n" "$me"; exit 1; }'
  starts=': >"$CI_D_REPO/start.$me"'
  ends=': >"$CI_D_REPO/end.$me"'
  running='others=0; for f in "$CI_D_REPO"/start.*; do o=${f##*/start.}; [ "$o" = "$me" ] || [ ! -e "$f" ] || [ -e "$CI_D_REPO/end.$o" ] || others=$((others + 1)); done'
  alone='before=0; for f in "$CI_D_REPO"/start.*; do [ -e "$f" ] || continue; o=${f##*/start.}; [ -e "$CI_D_REPO/end.$o" ] || { printf "FAIL self-test:%s (it started beside %s)\n" "$me" "$o"; exit 1; }; before=$((before + 1)); done; : >"$CI_D_REPO/start.$me"; sleep 2; after=0; for f in "$CI_D_REPO"/start.*; do after=$((after + 1)); done; [ "$after" -eq "$((before + 1))" ] || { printf "FAIL self-test:%s (another self-test started beside it)\n" "$me"; exit 1; }'
  meets="$prelude; meet || $ran_alone; $passes"
  alone_list=scripts/ci/self-test-alone.txt

  new_repo && add_script r1 10-a.sh "$good" "$meets" && add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case self-tests-run-at-the-same-time 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
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
    mkdir -p "$fixture/scripts/ci" && printf '10-a\r\n' >"$fixture/$alone_list"
  case_jobs=2
  own_case alone-list-line-with-a-carriage-return 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $starts; sleep 1; $running; [ \"\$others\" -le 1 ] || { printf \"FAIL self-test:%s (two others ran beside it)\\n\" \"\$me\"; exit 1; }; $ends; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $starts; sleep 1; $running; [ \"\$others\" -le 1 ] || { printf \"FAIL self-test:%s (two others ran beside it)\\n\" \"\$me\"; exit 1; }; $ends; $passes" &&
    add_script r1 30-c.sh "$good" "$prelude; $starts; sleep 1; $running; [ \"\$others\" -le 1 ] || { printf \"FAIL self-test:%s (two others ran beside it)\\n\" \"\$me\"; exit 1; }; $ends; $passes"
  case_jobs=2
  own_case two-at-a-time 0 'ci.d macos self-test: 3 passed, 0 failed' macos --self-test 10-a 20-b 30-c
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || $ran_alone; printf \"FAIL self-test:%s (planned)\\n\" \"\$me\"; exit 1" &&
    add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case failing-self-test-beside-another 1 'FAIL self-test:10-a (planned)' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; printf \"%s\\n\" \"\$me\" >\"\$TMPDIR/shared\"; meet || $ran_alone; read -r got <\"\$TMPDIR/shared\"; [ \"\$got\" = \"\$me\" ] || { printf \"FAIL self-test:%s (another self-test wrote its file)\\n\" \"\$me\"; exit 1; }; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; printf \"%s\\n\" \"\$me\" >\"\$TMPDIR/shared\"; meet || $ran_alone; read -r got <\"\$TMPDIR/shared\"; [ \"\$got\" = \"\$me\" ] || { printf \"FAIL self-test:%s (another self-test wrote its file)\\n\" \"\$me\"; exit 1; }; $passes"
  case_jobs=2
  own_case two-self-tests-write-one-path 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || exit 1; exit 0" && add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case self-test-prints-nothing 1 'FAIL self-test:10-a (exit 0)' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || $ran_alone; kill -s TERM \"\$\$\"; $passes" && add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case self-test-ended-by-a-signal 1 'FAIL self-test:10-a (exit 143)' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; meet || $ran_alone; kill -s KILL \"\$PPID\"; sleep 1; $passes" && add_script r1 20-b.sh "$good" "$meets"
  case_jobs=2
  own_case driver-job-of-a-self-test-killed 1 'FAIL self-test:10-a (it ended without an exit status)' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; trap 'printf \"ended by TERM\\n\"; exit 143' TERM; sleep 30 & wait \"\$!\"; $passes" && add_script r1 20-b.sh "$good" "$prelude; $passes"
  case_jobs=2
  case_limit=2
  case_first="ended by TERM${nl}seconds 10-a "
  case_then='FAIL self-test:10-a (time limit 2 s)'
  own_case self-test-past-its-limit 1 'FAIL self-test:10-a (time limit 2 s)' macos --self-test 10-a 20-b
  new_repo && add_script r1 10-a.sh "$good" "$prelude; printf '%s\\n' \"\$\$\" >\"\$CI_D_REPO/pid.10-a\"; trap '' TERM; sleep 60; $passes" &&
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
  fi
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '99-gone it was renamed\n' >"$fixture/$alone_list"
  own_case alone-list-names-no-script 1 "FAIL run-ci-d ($alone_list names 99-gone, which is no script)" macos --self-test 10-a
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
  new_repo && add_script r1 10-a.sh "$good" "$prelude; sleep 1; $passes" &&
    add_script r1 20-x.sh '# rule: A fixture rule.' "$prelude; $passes" &&
    add_script r1 30-c.sh "$good" "$prelude; $passes"
  case_jobs=2
  case_first="PASS self-test:10-a (1 failing fixtures rejected)${nl}FAIL 20-x (its first 10 lines hold no single '# jobs:' line)${nl}PASS self-test:30-c (1 failing fixtures rejected)"
  case_then='ci.d macos self-test: 2 passed, 1 failed'
  own_case refusal-keeps-its-place-in-the-recap 1 "FAIL 20-x (its first 10 lines hold no single '# jobs:' line)" macos --self-test 10-a 30-c
  case_scripts=$(mktemp -d "$tmp/empty.XXXXXX")
  case_nested=fail
  own_case own-cases-that-fail-in-a-nested-run 1 'ci.d macos self-test: 0 passed, 1 failed' macos --self-test run-ci-d
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $alone; $ends; $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; $starts; sleep 1; $ends; $passes" &&
    mkdir -p "$fixture/scripts/ci" && printf '  10-a\tits self-test fails beside another one\n' >"$fixture/$alone_list"
  case_jobs=2
  own_case alone-list-line-with-leading-blanks 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
  prelude_any=${prelude#*exit 3; }
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
      own_run macos 10-a
      foreground=$(printf '%s\n' "$got_out" | grep -E '^(PASS|FAIL) 10-a( |$)' | tail -n 1)
      after=no
      ! printf '%s\n' "$got_out" | grep -qxF -e 'line after the kill' || after=yes
      case_jobs=2
      case $foreground:$after in
        'PASS 10-a:yes')
          case_first='line after the kill'
          case_then='PASS self-test:10-a (1 failing fixtures rejected)'
          own_case "self-test-that-sends-$signal-to-itself-$place-runs-on-as-in-the-foreground" 0 'ci.d macos self-test: 2 passed, 0 failed' macos --self-test 10-a 20-b
          ;;
        'FAIL 10-a (exit '*'):no')
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
  done
  new_repo && add_script r1 10-a.sh "$good" "$prelude; $passes"
  case_no_perl=yes
  own_case perl-does-not-run 1 'FAIL run-ci-d (perl does not run; the step ci.d self-test starts each self-test through perl, for INT and QUIT at their defaults and a process group of its own)' macos --self-test 10-a
  for signal in INT:130 TERM:143; do
    new_repo && add_script r1 10-a.sh "$good" "$prelude; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.\$me\"; wait; $passes" &&
      add_script r1 20-b.sh "$good" "$prelude; sleep 60 & printf '%s\\n%s\\n' \"\$\$\" \"\$!\" >>\"\$CI_D_REPO/pids\"; : >\"\$CI_D_REPO/started.\$me\"; wait; $passes"
    case_jobs=2
    case_signal=${signal%:*}
    case_wait='started.10-a started.20-b'
    case_max_s=30
    case_signal_min=4
    own_case "step-interrupted-by-${signal%:*}" "${signal#*:}" "FAIL run-ci-d (the step was interrupted by ${signal%:*}; every self-test that it started is ended)" macos --self-test 10-a 20-b
  done
  new_repo && add_script r1 10-a.sh "$good" "$prelude; ( sleep 2; printf 'late detail of %s on stderr\\n' \"\$me\" >&2 ) >/dev/null & $passes" &&
    add_script r1 20-b.sh "$good" "$prelude; sleep 4; $passes"
  case_jobs=2
  case_first="standard error of 10-a after it was printed:${nl}late detail of 10-a on stderr"
  case_then='ci.d macos self-test: 2 passed, 0 failed'
  own_case standard-error-written-after-the-print 0 'late detail of 10-a on stderr' macos --self-test 10-a 20-b

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
cr=$(printf '\r')
tab=$(printf '\t')
if [ "$mode" = self-test ]; then
  if [ -z "$st_jobs_set" ]; then
    st_jobs=$(getconf _NPROCESSORS_ONLN 2>/dev/null) || st_jobs=
    whole_number "$st_jobs" || st_jobs=${NUMBER_OF_PROCESSORS:-1}
    whole_number "$st_jobs" || st_jobs=1
  else
    whole_number "$st_jobs" || {
      printf 'FAIL run-ci-d (CI_D_SELF_TEST_JOBS=%s is not a whole number from 1 to 999999999)\n' "$st_jobs"
      exit 1
    }
  fi
  whole_number "$st_limit" || {
    printf 'FAIL run-ci-d (CI_D_SCRIPT_LIMIT_S=%s is not a whole number from 1 to 999999999)\n' "$st_limit"
    exit 1
  }
  perl -e "$st_perl" sh -c 'exit 0' </dev/null >/dev/null 2>&1 || {
    printf 'FAIL run-ci-d (perl does not run; the step ci.d self-test starts each self-test through perl, for INT and QUIT at their defaults and a process group of its own)\n'
    exit 1
  }
  trap 'st_interrupt INT 130' USR1
  trap 'st_interrupt TERM 143' TERM
  if [ -e "$st_list" ]; then
    while IFS= read -r st_line || [ -n "$st_line" ]; do
      st_first_word "$st_line"
      st_alone="$st_alone$st_word "
    done <"$st_list" || {
      printf 'FAIL run-ci-d (%s cannot be read)\n' "$st_list"
      exit 1
    }
  fi
fi

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
    run_script
  fi
done

if [ "$mode" = self-test ]; then
  if [ -e "$st_list" ]; then
    while IFS= read -r st_line || [ -n "$st_line" ]; do
      st_first_word "$st_line"
      [ -n "$st_word" ] || continue
      case $seen in
        *" $st_word "*) ;;
        *) note "FAIL run-ci-d ($st_list names $st_word, which is no script)" ;;
      esac
    done <"$st_list"
  fi
  st_wait_below 1
  st_flush
  wait
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
fi

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
  lock_head=$(git rev-parse -q --verify "HEAD:$lock" 2>/dev/null)
  lock_tree=
  [ ! -e "$lock" ] || lock_tree=$(git hash-object -- "$lock" 2>/dev/null) || lock_tree=unreadable
  [ "$lock_head" = "$lock_tree" ] || lock_changed="$lock_changed $lock"
done
if ! git diff --exit-code HEAD -- Cargo.lock package-lock.json || [ -n "$lock_changed" ]; then
  printf 'FAIL run-ci-d (Cargo.lock or package-lock.json differs from HEAD; a script builds with --locked and changes no tracked file)\n'
  status=1
fi
exit "$status"
