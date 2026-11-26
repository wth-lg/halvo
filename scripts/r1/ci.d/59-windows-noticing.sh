#!/bin/sh
# jobs: windows
# rule: on a hosted Windows machine the built app with its capture service, the owner of the derived store and the search worker notices a repeated task from the system's own input: a program of the check under the file name EXCEL.EXE takes 12 clicks in one window on three occasions, the second by the title that the search carried, with two returns to that window behind the second, one from Halvo's own window and one from a second program of the check that hands the front back at once, each read by the foreground_app and window_title records that the store gained; the third occasion rests on the title of the last return alone, and the reply to the request that the app's command sends holds one row of the category spreadsheet_fill with the count 3; a red run names its occasion and its stage
# lane: alone
set -u
name=59-windows-noticing
target=windows_noticing
walk_test=the_walk_takes_its_steps_from_the_check
state_file='daemon-state.json'
slow_list=scripts/ci/slow-tests.txt
programs='halvo halvo-capture halvo-owner halvo-analysis'
service_s=60
title_s=20
clicks_s=20
witness_s=20
settle_s=15
search_s=300
search_poll_s=5
commit_s=30
second_s=30
early_s=200
step_s=40
row_s=40
poll_s=2
tries=3
# The carriage return that ends a line of a program of Windows; measure reads it.
cr=

# The run's state first, the FAIL line last: the driver reads the last line as the verdict.
fail() {
  show_state
  end_all
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# Writes standard input to standard output with no carriage return and with the name behind a
# home directory masked.
mask() {
  tr -d '\r' | sed -e 's/\([A-Za-z]:[\\/]\{1,2\}[U]sers[\\/]\{1,2\}\)[^\\/ "]*/\1<user>/g' \
    -e 's/\([/][U]sers[/]\)[^/ "]*/\1<user>/g' -e 's/\([/][h]ome[/]\)[^/ "]*/\1<user>/g'
}

# The daemon's own lines and the last ctl status, each whole and as it stands but for the mask
# of a home path: printed, and saved among the job's artifacts where the run has them.
show_state() {
  [ -n "${err:-}" ] || return 0
  ctl status
  printf 'exit=%s\n%s\n' "$ctl_code" "$ctl_out" | mask >"$work/ctl-status.txt"
  if [ -f "$err" ]; then
    mask <"$err" >"$work/daemon.err.txt"
  else
    printf 'the file does not stand\n' >"$work/daemon.err.txt"
  fi
  while IFS= read -r line || [ -n "$line" ]; do
    printf 'daemon.err: %s\n' "$line"
  done <"$work/daemon.err.txt"
  while IFS= read -r line || [ -n "$line" ]; do
    printf 'ctl status: %s\n' "$line"
  done <"$work/ctl-status.txt"
  [ -n "${artifacts:-}" ] || return 0
  cp "$work/daemon.err.txt" "$work/ctl-status.txt" "$artifacts/" || printf 'noticing: the state files could not be saved among the artifacts\n'
  for file in walk.out passer.out app.err; do
    [ ! -f "$work/$file" ] || mask <"$work/$file" >"$artifacts/$file.txt"
  done
}

# Sets native to the path $1 as the programs of the system read it.
to_native() {
  if command -v cygpath >/dev/null 2>&1; then
    native=$(cygpath -w "$1") || fail "the path $1 has no native form"
  else
    native=$1
  fi
}

pause() {
  sleep "$1"
}

# Sets stamp to the clock's second.
now() {
  stamp=$(date +%s) || fail "the clock cannot be read"
}

# Sets ctl_out and ctl_code to what ctl of the run's data directory printed for the verb $1 and
# to its exit code. The check calls the verbs status, inventory and stop alone.
ctl() {
  ctl_out=$("$daemon" ctl --data-dir "$data_native" "$1" 2>&1)
  ctl_code=$?
}

# ctl for a verb that must answer: three tries a second apart, then the run ends at its stage
# with the first line that ctl printed.
ctl_sure() {
  ctl_tries=0
  while :; do
    ctl "$1"
    [ "$ctl_code" -eq 0 ] && return 0
    ctl_tries=$((ctl_tries + 1))
    [ "$ctl_tries" -lt 3 ] || fail "occasion $occasion, stage $stage: ctl $1 exited $ctl_code three times: ${ctl_out%%
*}"
    pause 1
  done
}

# Hands the walk the step $1 and reads the one line that the walk writes for it into
# walk_line, within $2 s.
walk_do() {
  step_n=$((step_n + 1))
  printf '%s\n' "$1" >"$steps/$step_n.go" || fail "occasion $occasion, stage $stage: the step $1 cannot be handed to the walk"
  step_waited=0
  while [ ! -f "$steps/$step_n.done" ]; do
    if [ "$step_waited" -ge "$2" ]; then
      show_walk
      fail "occasion $occasion, stage $stage: the walk wrote no line for its step $1 within $2 s"
    fi
    pause 1
    step_waited=$((step_waited + 1))
  done
  IFS= read -r walk_line <"$steps/$step_n.done" || walk_line=
  walk_line=${walk_line%"$cr"}
  printf '%s\n' "$walk_line"
}

show_walk() {
  for file in walk.out passer.out app.err; do
    [ -f "$work/$file" ] || continue
    tail -n 20 "$work/$file" | LC_ALL=C tr -c '[:print:]\n' '?' | sed "s/^/noticing $file: /"
  done
}

start_app() {
  printf '{"desired":"running"}\n' >"$data/$state_file" || fail "occasion 0, stage service: the state file cannot be written"
  "$app" --data-dir "$data_native" >"$work/app.out" 2>"$work/app.err" &
  started=yes
}

# Starts the copy $1 of the walk's program in the role $2, its output in the file $3.
start_walk() {
  HALVO_NOTICING_DIR=$steps_native HALVO_NOTICING_DATA_DIR=$data_native HALVO_NOTICING_ROLE=$2 \
    "$work/$1" --ignored --exact --nocapture "$walk_test" >"$work/$3" 2>&1 &
}

# Every program of the run that the machine holds, one line each: the name, the process id, the
# id of its parent and its command line.
list_processes() {
  script=$work/list.ps1
  [ -f "$script" ] || cat >"$script" <<'EOF' || return 1
$ErrorActionPreference = "Stop"
$names = "halvo.exe", "halvo-capture.exe", "halvo-owner.exe", "halvo-analysis.exe", "EXCEL.EXE", "noticing-pass.exe"
$found = Get-CimInstance -ClassName Win32_Process | Where-Object { $names -contains $_.Name }
foreach ($p in $found) {
  [Console]::Out.WriteLine(("{0} {1} {2} {3} {4}" -f $p.Name, $p.ProcessId, $p.ParentProcessId, $p.ExecutablePath, $p.CommandLine))
}
EOF
  to_native "$script"
  powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$native" >"$work/list.raw" || return 1
  tr -d '\r' <"$work/list.raw"
}

end_process() {
  taskkill //F //PID "$1" >/dev/null 2>&1
}

# Ends what the run started: the walk and its second copy by their end file, the service by ctl
# stop, then every program whose line holds the name of the run's directory, and each child of
# one of those.
end_all() {
  [ -n "${started:-}" ] || return 0
  started=
  trap - EXIT
  printf 'end\n' >"$steps/end.go"
  ctl stop
  pause 2
  for round in 1 2; do
    list_processes >"$work/snapshot" 2>/dev/null || return 0
    pids=
    while read -r _ pid _ rest; do
      case $rest in
        *"$token"*) pids="$pids $pid" ;;
      esac
    done <"$work/snapshot"
    while read -r _ pid parent _; do
      case " $pids " in
        *" $parent "*) pids="$pids $pid" ;;
      esac
    done <"$work/snapshot"
    [ -n "$pids" ] || return 0
    [ "$round" = 1 ] || printf 'noticing: still running at the end:%s\n' "$pids"
    for pid in $pids; do
      end_process "$pid"
    done
    pause 2
  done
}

# Sets value to the word behind the key $1 in the line $2, or to - where the line has no such key.
field() {
  rest=" $2"
  case $rest in
    *" $1="*)
      value=${rest#*" $1="}
      value=${value%% *}
      ;;
    *) value=- ;;
  esac
}

# Reads ctl inventory: the events of the pointer, window_title and foreground_app rows of every
# day into c_pointer, c_titles and c_fa, and the bytes of the window_title rows into c_tbytes.
read_counts() {
  ctl_sure inventory
  c_pointer=0
  c_titles=0
  c_fa=0
  c_tbytes=0
  while IFS= read -r line; do
    line=${line%"$cr"}
    case $line in
      'row '*) ;;
      *) continue ;;
    esac
    field modality "$line"
    modality=$value
    field events "$line"
    events=$value
    field bytes "$line"
    bytes=$value
    case $events$bytes in
      *[!0-9]*) continue ;;
    esac
    case $modality in
      pointer) c_pointer=$((c_pointer + events)) ;;
      window_title)
        c_titles=$((c_titles + events))
        c_tbytes=$((c_tbytes + bytes))
        ;;
      foreground_app) c_fa=$((c_fa + events)) ;;
    esac
  done <<EOF
$ctl_out
EOF
}

# Reads ctl status into the s_ variables, the running sources into s_sources, and the four
# figures that tell "not yet written" from "not recorded" into status_text.
read_status() {
  if [ "$stage" = service ]; then
    ctl status
  else
    ctl_sure status
  fi
  s_desired=-
  s_capture=-
  s_reason=-
  s_lease=-
  s_store=-
  s_pending=-
  s_lost=-
  s_errors=-
  s_flush=-
  s_sources=
  while IFS= read -r line; do
    line=${line%"$cr"}
    case $line in
      desired=*) s_desired=${line#desired=} ;;
      capture=*) s_capture=${line#capture=} ;;
      capture_reason=*) s_reason=${line#capture_reason=} ;;
      lease=*) s_lease=${line#lease=} ;;
      store=*) s_store=${line#store=} ;;
      events_pending=*) s_pending=${line#events_pending=} ;;
      events_lost=*) s_lost=${line#events_lost=} ;;
      title_read_errors=*) s_errors=${line#title_read_errors=} ;;
      last_flush_at=*) s_flush=${line#last_flush_at=} ;;
      'source name='*)
        field name "$line"
        source=$value
        field state "$line"
        s_sources="$s_sources $source=$value"
        ;;
    esac
  done <<EOF
$ctl_out
EOF
  status_text="events_pending=$s_pending events_lost=$s_lost title_read_errors=$s_errors last_flush_at=$s_flush"
  s_shown=${s_sources# }
  [ -n "$s_shown" ] || s_shown=none
}

# Reads logs/daemon.err of the data directory, its lines into n_lines. Each batch line and each
# analysis line that no read has shown yet is printed with the second of the walk at which this
# read first saw it. Behind the line err_base, the occasion's baseline: own_batch is the FIRST
# batch line, the occasion's own; own_commit says whether a line analysis end=committed stands
# behind it; later_batches and later_commits count the batch lines behind the first and the
# commit lines behind its commit, which are those of the second worker that the service starts
# at once from the new position and that reads nothing; analysis_since holds every analysis line.
read_err() {
  n_lines=0
  own_batch=
  own_commit=no
  later_batches=0
  later_commits=0
  analysis_since=
  [ -f "$err" ] || return 0
  seen_at=
  while IFS= read -r line || [ -n "$line" ]; do
    line=${line%"$cr"}
    n_lines=$((n_lines + 1))
    case $line in
      *'batch end='*) kind=worker ;;
      *'halvo-capture: analysis '*) kind=service ;;
      *) continue ;;
    esac
    if [ "$n_lines" -gt "$err_shown" ]; then
      if [ -z "$seen_at" ]; then
        now
        seen_at=$((stamp - walk_began))
      fi
      printf 'daemon.err line %s, first seen at second %s: %s\n' "$n_lines" "$seen_at" "$line"
    fi
    [ "$n_lines" -gt "$err_base" ] || continue
    if [ "$kind" = worker ]; then
      if [ -z "$own_batch" ]; then
        own_batch="end=${line#*batch end=}"
      else
        later_batches=$((later_batches + 1))
      fi
      continue
    fi
    analysis_since="$analysis_since | ${line#*halvo-capture: }"
    case $own_batch:$own_commit:$line in
      :*) ;;
      *:no:*'analysis end=committed'*) own_commit=yes ;;
      *:yes:*'analysis end=committed'*) later_commits=$((later_commits + 1)) ;;
    esac
  done <"$err"
  err_shown=$n_lines
}

# Reads the counts every poll_s until the count $1 (pointer, titles or fa) is $2 or more, at
# most $3 s; met says whether it was.
wait_count() {
  waited=0
  while :; do
    read_counts
    case $1 in
      pointer) have=$c_pointer ;;
      titles) have=$c_titles ;;
      *) have=$c_fa ;;
    esac
    if [ "$have" -ge "$2" ]; then
      met=yes
      return 0
    fi
    if [ "$waited" -ge "$3" ]; then
      met=no
      return 0
    fi
    pause "$poll_s"
    waited=$((waited + poll_s))
  done
}

# Prints the line of a green stage with the seconds since the stage before it.
stage_ok() {
  now
  printf 'stage %s seconds=%s\n' "$1" "$((stamp - mark))"
  mark=$stamp
}

# A red stage that lets the walk go on: the run ends FAIL with the first of them and the row.
red() {
  printf 'RED occasion %s, stage %s: %s\n' "$occasion" "$1" "$2"
  [ -n "$first_red" ] || first_red="occasion $occasion, stage $1: $2"
}

# Notes the seconds since the lap before under the name $1, at the clock's last reading.
lap() {
  laps="$laps $1=$((stamp - lap_mark))"
  lap_mark=$stamp
}

# The walk must have made its step: where another window covers its own, the run ends at the
# stage title with that window's class and the file name of its program.
need_own() {
  field ok "$walk_line"
  [ "$value" = yes ] && return 0
  field why "$walk_line"
  [ "$value" = covered ] || fail "occasion $occasion, stage $stage: the walk could not make its step: $walk_line"
  field front_class "$walk_line"
  class=$value
  field front_file "$walk_line"
  file=$value
  read_status
  fail "occasion $occasion, stage title: another window covers the walk's window: class=$class file=$file; $status_text"
}

# The 12 clicks of the occasion: the pointer count grows by 12 or more and no event is lost.
# The batch and the commit lines that stand before the clicks are the baseline of the occasion's
# search, so that a search which runs while the returns are read is not missed.
clicks_stage() {
  stage=clicks
  read_err
  err_base=$n_lines
  read_status
  k_lost=$s_lost
  read_counts
  k_pointer=$c_pointer
  walk_do clicks "$step_s"
  need_own
  now
  input_at=$stamp
  field sent "$walk_line"
  sent=$value
  wait_count pointer $((k_pointer + 12)) "$clicks_s"
  read_status
  [ "$met" = yes ] || fail "occasion $occasion, stage clicks: pointer=+$((c_pointer - k_pointer)) of 12 within $clicks_s s; $status_text; walk sent=$sent"
  [ "$s_lost" = "$k_lost" ] || fail "occasion $occasion, stage clicks: events_lost went from $k_lost to $s_lost; pointer=+$((c_pointer - k_pointer)); $status_text; walk sent=$sent"
  stage_ok "clicks occasion=$occasion ok pointer=+$((c_pointer - k_pointer)) sent=$sent events_lost=$s_lost"
}

# The search behind the occasion's last input: the occasion's OWN batch line, the first batch
# line behind the occasion's baseline, read into b_end, b_records and b_episodes. A batch line
# that stands less than early_s behind the occasion's last input is no line of this occasion,
# since no worker starts before 240 s with no input: it is printed, and the baseline moves
# behind it.
search_stage() {
  stage=search
  read_counts
  q_pointer=$c_pointer
  q_titles=$c_titles
  q_fa=$c_fa
  waited=0
  while :; do
    read_err
    if [ -n "$own_batch" ]; then
      now
      [ $((stamp - input_at)) -lt "$early_s" ] || break
      printf 'note: a batch line stands %s s behind the last input of occasion %s, before a worker of this occasion can have started: the baseline moves behind it\n' "$((stamp - input_at))" "$occasion"
      err_base=$n_lines
    fi
    if [ "$waited" -ge "$search_s" ]; then
      walk_do look "$step_s"
      field idle_ms "$walk_line"
      idle=$value
      read_counts
      fail "occasion $occasion, stage search: no batch line within $search_s s of the last input; walk idle_ms=$idle; in the pause foreground_app=+$((c_fa - q_fa)) window_title=+$((c_titles - q_titles)) pointer=+$((c_pointer - q_pointer)); analysis lines:${analysis_since:- none}"
    fi
    pause "$search_poll_s"
    waited=$((waited + search_poll_s))
  done
  field end "$own_batch"
  b_end=$value
  field records "$own_batch"
  b_records=$value
  field episodes "$own_batch"
  b_episodes=$value
  stage_ok "search occasion=$occasion ok batch $own_batch"
}

# Whether the batch line of the occasion reads end=newest, episodes=1 and $1 records or more.
episode_holds() {
  case $b_records in
    *[!0-9]* | '') return 1 ;;
  esac
  [ "$b_end" = newest ] && [ "$b_episodes" = 1 ] && [ "$b_records" -ge "$1" ]
}

# The commit of the occasion's own batch: a line analysis end=committed of the service behind
# that batch line. Then the lines of the second worker, which the service starts at once from
# the new position and which reads nothing: its batch line, with no record, and its commit line
# are waited for, second_s at most, printed as every line is, and never read as the occasion's;
# the next occasion's baseline stands behind both. Last, what the store gained of
# foreground_app and of titles in the pause.
commit_stage() {
  stage=commit
  waited=0
  while :; do
    read_err
    [ "$own_commit" = yes ] && break
    [ "$waited" -lt "$commit_s" ] || fail "occasion $occasion, stage commit: no analysis end=committed line within $commit_s s of the occasion's batch line; analysis lines:${analysis_since:- none}"
    pause "$poll_s"
    waited=$((waited + poll_s))
  done
  waited=0
  while [ "$later_batches:$later_commits" = 0:0 ] || [ "$later_batches" -ne "$later_commits" ]; do
    [ "$waited" -lt "$second_s" ] || break
    pause "$poll_s"
    waited=$((waited + poll_s))
    read_err
  done
  printf 'second worker occasion=%s: %s batch lines behind the own one, %s commit lines behind its commit, waited %s s\n' "$occasion" "$later_batches" "$later_commits" "$waited"
  read_counts
  printf 'pause %s foreground_app=+%s window_title=+%s\n' "$occasion" "$((c_fa - q_fa))" "$((c_titles - q_titles))"
  stage_ok "commit occasion=$occasion ok"
}

# Ends the run where the occasion's own batch line holds no record, no other batch line stands
# in the pause, and the store counted the occasion's clicks: the search read none of what the
# store holds, and that is the product's fault.
one_empty_batch_is_the_products() {
  [ "$b_records:$later_batches" = 0:0 ] || return 0
  fail "occasion $occasion, stage episode: the product's fault: the pause shows one batch line with records=0 and the records counted in the store: pointer=+$((c_pointer - k_pointer)) behind the occasion's baseline; batch $own_batch"
}

# Before the clicks of a carry: the walk's window is in front, and the store gained no
# foreground_app record and no title since the last title of that window.
carry_holds() {
  stage=carry
  walk_do look "$step_s"
  field front "$walk_line"
  front=$value
  field last_other_class "$walk_line"
  other_class=$value
  field last_other_file "$walk_line"
  other_file=$value
  read_counts
  [ "$front" = own ] && [ "$c_fa" -eq "$f_hold" ] && [ "$c_titles" -eq "$t_hold" ]
}

# The walk-on: the walk's second window in front and its first again, so that the base's own
# rule writes the title of the first once more; t_hold and f_hold are the counts behind it.
walk_on() {
  stage=title
  form=walk-on
  read_counts
  n_pointer=$c_pointer
  n_titles=$c_titles
  n_fa=$c_fa
  walk_do notes "$step_s"
  need_own
  wait_count titles $((n_titles + 1)) "$title_s"
  if [ "$met" = no ]; then
    read_status
    fail "occasion $occasion, stage title: window_title=+$((c_titles - n_titles)) within $title_s s behind the click into the second window; pointer=+$((c_pointer - n_pointer)) foreground_app=+$((c_fa - n_fa)); $status_text"
  fi
  m_titles=$c_titles
  walk_do front "$step_s"
  need_own
  wait_count titles $((m_titles + 1)) "$title_s"
  if [ "$met" = no ]; then
    read_status
    fail "occasion $occasion, stage title: window_title=+$((c_titles - m_titles)) within $title_s s behind the click back into the window; pointer=+$((c_pointer - n_pointer)) foreground_app=+$((c_fa - n_fa)); $status_text"
  fi
  t_hold=$c_titles
  f_hold=$c_fa
  stage_ok "title occasion=$occasion ok form=walk-on window_title=+$((c_titles - n_titles)) foreground_app=+$((c_fa - n_fa)) pointer=+$((c_pointer - n_pointer))"
}

# Whether the $1 titles that a return gained, of $2 bytes together, are all titles of the
# workbook's window: their bytes lie within 4 a title of the title record of occasion 1, whose
# bytes book_bytes holds. The inventory gives the sum alone, so two titles may lie 8 bytes off
# together: the title of the walk's second window is 14 signs shorter than the workbook's and
# that of the second program's window 40 signs longer, and neither passes beside a title of the
# workbook. A title record of one window differed by 1 byte on a hosted machine (311 and 312).
# It sets books to the words that the stage line prints, and books_hold to yes where every
# title is the workbook's.
books_of() {
  books_hold=no
  if [ "$1" -lt 1 ]; then
    books='no title'
  elif [ "$2" -ge $(($1 * (book_bytes - 4))) ] && [ "$2" -le $(($1 * (book_bytes + 4))) ]; then
    books="the titles are the workbook's"
    books_hold=yes
  else
    books='a title of another window is among them'
  fi
}

# The return from Halvo's own window, whose title and foreground_app record the floor leaves
# out: the store gains the one foreground_app record of the return, and the workbook's title
# once, or twice where the title source decided the return before it read that the record was
# placed and then wrote the title again behind the record.
return_own() {
  stage=return-own
  read_counts
  r_titles=$c_titles
  r_fa=$c_fa
  r_bytes=$c_tbytes
  walk_do own "$step_s"
  need_own
  field away "$walk_line"
  away=$value
  field stay_ms "$walk_line"
  stay=$value
  field fg_events "$walk_line"
  fg_events=$value
  wait_count fa $((r_fa + 1)) "$witness_s"
  pause "$settle_s"
  read_counts
  read_status
  gained_fa=$((c_fa - r_fa))
  gained_titles=$((c_titles - r_titles))
  books_of "$gained_titles" "$((c_tbytes - r_bytes))"
  facts="foreground_app=+$gained_fa window_title=+$gained_titles title_bytes=+$((c_tbytes - r_bytes)), $books, wanted +1 and +1 or +2; walk away=$away fg_events=$fg_events stay_ms=$stay; $status_text"
  if [ "$away" = refused ]; then
    own=not-met
    red return-own "return-own=not-met, Halvo's window did not come in front: $facts"
  elif [ "$gained_fa:$books_hold" = 1:yes ] && [ "$gained_titles" -le 2 ]; then
    own=green
    stage_ok "return-own occasion=2 ok $facts"
  elif [ "$gained_titles:$fg_events" = 0:0 ] && [ "$gained_fa" -ge 1 ]; then
    own=red
    red return-own "order=O4 $facts"
  else
    own=red
    red return-own "$facts"
  fi
}

# The return from the second program of the check, which hands the front back at once: the
# store gains two foreground_app records, and the workbook's title once, or twice where the
# title source decided the return before it read that a record was placed. Where a title of
# another window is among them, the gate decided the second program's window at once, and
# where the store gains fewer than two foreground_app records, the tracker saw no other
# program: the walk makes the return again, tries times at most.
return_short() {
  stage=return-short
  try=0
  while [ "$try" -lt "$tries" ]; do
    try=$((try + 1))
    read_counts
    r_titles=$c_titles
    r_fa=$c_fa
    r_bytes=$c_tbytes
    walk_do short "$step_s"
    need_own
    field stay_ms "$walk_line"
    stay=$value
    field fg_events "$walk_line"
    fg_events=$value
    wait_count fa $((r_fa + 2)) "$witness_s"
    pause "$settle_s"
    read_counts
    read_status
    gained_fa=$((c_fa - r_fa))
    gained_titles=$((c_titles - r_titles))
    books_of "$gained_titles" "$((c_tbytes - r_bytes))"
    facts="foreground_app=+$gained_fa window_title=+$gained_titles title_bytes=+$((c_tbytes - r_bytes)), $books, wanted +2 and +1 or +2; walk fg_events=$fg_events stay_ms=$stay try=$try; $status_text"
    printf 'return=short try=%s %s\n' "$try" "$facts"
    if [ "$gained_fa" -ge 2 ] && [ "$books_hold" = yes ] && [ "$gained_titles" -le 2 ]; then
      short=green
      stage_ok "return-short occasion=2 ok $facts"
      return 0
    fi
    if [ "$gained_fa" -ge 2 ] && [ "$gained_titles" -eq 0 ]; then
      short=red
      if [ "$fg_events" = 0 ]; then
        red return-short "order=O4 $facts"
      else
        red return-short "$facts"
      fi
      return 0
    fi
  done
  short=not-met
  red return-short "return-short=not-met in $tries tries: $facts"
}

# The walk on the programs of the directory $1, with the run's files under $2.
cases() {
  bin=$1
  work=$2
  suffix=
  [ -f "$bin/halvo.exe" ] && suffix=.exe
  app=$bin/halvo$suffix
  daemon=$bin/halvo-capture$suffix
  occasion=0
  stage=service
  step_n=0
  err_base=0
  err_shown=0
  input_at=0
  first_red=
  laps=
  c_pointer=0
  c_titles=0
  c_fa=0
  c_tbytes=0
  for program in halvo halvo-capture halvo-owner halvo-analysis; do
    [ -f "$bin/$program$suffix" ] || fail "occasion 0, stage service: no $program$suffix in $bin"
  done
  data=$work/data
  steps=$work/steps
  if [ ! -d "$data" ] || [ ! -d "$steps" ]; then
    mkdir -p "$data" "$steps" || fail "occasion 0, stage service: the run's directories cannot be made"
  fi
  to_native "$data"
  data_native=$native
  to_native "$steps"
  steps_native=$native
  err=$data/logs/daemon.err
  token=${work##*/}
  printf 'walked: a program of the check under the file name EXCEL.EXE, not Microsoft Excel\n'
  trap end_all EXIT
  now
  mark=$stamp
  lap_mark=$stamp
  walk_began=$stamp
  start_app
  waited=0
  while :; do
    read_status
    ready=no
    if [ "$ctl_code" -eq 0 ] && [ "$s_desired:$s_capture:$s_lease:$s_store" = running:running:fresh:open ]; then
      ready=yes
      for wanted in pointer window_title foreground; do
        case " $s_sources " in
          *" $wanted=running "*) ;;
          *) ready=no ;;
        esac
      done
    fi
    [ "$ready" = yes ] && break
    [ "$waited" -lt "$service_s" ] || fail "occasion 0, stage service: within $service_s s ctl status read desired=$s_desired capture=$s_capture capture_reason=$s_reason lease=$s_lease store=$s_store sources=$s_shown; ctl exit=$ctl_code"
    pause "$poll_s"
    waited=$((waited + poll_s))
  done
  start_walk EXCEL.EXE workbook walk.out
  walk_do ready "$step_s"
  need_own
  field halvo_window "$walk_line"
  [ "$value" = yes ] || fail "occasion 0, stage service: no window of the app within 20 s of the walk's start: $walk_line"
  start_walk noticing-pass.exe passer passer.out
  walk_do passer "$step_s"
  field ok "$walk_line"
  [ "$value" = yes ] || fail "occasion 0, stage service: the second program of the check showed no window: $walk_line"
  stage_ok "service occasion=0 ok desired=$s_desired capture=$s_capture lease=$s_lease store=$s_store sources=$s_shown"
  lap service

  occasion=1
  stage=title
  form=new-title
  read_counts
  o_pointer=$c_pointer
  o_titles=$c_titles
  o_fa=$c_fa
  o_bytes=$c_tbytes
  walk_do front "$step_s"
  need_own
  wait_count titles $((o_titles + 1)) "$title_s"
  if [ "$met" = no ]; then
    read_status
    field front "$walk_line"
    fail "occasion 1, stage title: window_title=+$((c_titles - o_titles)) within $title_s s behind the click into the window; pointer=+$((c_pointer - o_pointer)) foreground_app=+$((c_fa - o_fa)); $status_text; walk front=$value"
  fi
  book_bytes=$(((c_tbytes - o_bytes) / (c_titles - o_titles)))
  stage_ok "title occasion=1 ok window_title=+$((c_titles - o_titles)) foreground_app=+$((c_fa - o_fa)) pointer=+$((c_pointer - o_pointer)) title_bytes=+$((c_tbytes - o_bytes))"
  clicks_stage
  t_hold=$c_titles
  f_hold=$c_fa
  search_stage
  commit_stage
  one_empty_batch_is_the_products
  episode_holds 13 || fail "occasion 1, stage episode: form=new-title batch end=$b_end records=$b_records episodes=$b_episodes, wanted end=newest episodes=1 records>=13; in the occasion window_title=+$((c_titles - o_titles)) foreground_app=+$((c_fa - o_fa)) pointer=+$((c_pointer - o_pointer))"
  stage_ok "episode occasion=1 ok form=new-title batch end=$b_end records=$b_records episodes=$b_episodes"
  lap occasion1

  occasion=2
  carry=walked
  form=carry
  if carry_holds; then
    stage_ok "carry occasion=2 ok front=own foreground_app=+0 window_title=+0 since the title of occasion 1"
  else
    carry=not-walked
    red carry "not walked: foreground_app=+$((c_fa - f_hold)) window_title=+$((c_titles - t_hold)) in the pause before it; the window that passed: class=$other_class file=$other_file; walk front=$front"
    walk_on
  fi
  clicks_stage
  carry_titles=$((c_titles - t_hold))
  carry_fa=$((c_fa - f_hold))
  clicks_form=$form
  if [ "$carry:$carry_titles:$carry_fa" != walked:0:0 ] && [ "$carry" = walked ]; then
    carry=not-walked
    red carry "not walked: foreground_app=+$carry_fa window_title=+$carry_titles behind its clicks"
  fi
  lap clicks2
  return_own
  return_short
  if [ "$short" = green ]; then
    t_hold=$c_titles
    f_hold=$c_fa
  else
    walk_on
  fi
  lap returns
  search_stage
  commit_stage
  one_empty_batch_is_the_products
  again=no
  if episode_holds 12; then
    stage_ok "episode occasion=2 ok form=$clicks_form carry=$carry batch end=$b_end records=$b_records episodes=$b_episodes"
  elif [ "$b_end:$b_episodes:$carry" = newest:0:walked ]; then
    again=yes
    red carry "carry=failed batch end=$b_end records=$b_records episodes=$b_episodes; behind its clicks window_title=+$carry_titles foreground_app=+$carry_fa; the walk went on by a title of its second window"
  else
    fail "occasion 2, stage episode: form=$clicks_form carry=$carry batch end=$b_end records=$b_records episodes=$b_episodes, wanted end=newest episodes=1 records>=12"
  fi
  if [ "$again" = yes ]; then
    walk_on
    clicks_stage
    search_stage
    commit_stage
    one_empty_batch_is_the_products
    episode_holds 13 || fail "occasion 2, stage episode: form=walk-on batch end=$b_end records=$b_records episodes=$b_episodes, wanted end=newest episodes=1 records>=13"
    stage_ok "episode occasion=2 ok form=walk-on batch end=$b_end records=$b_records episodes=$b_episodes"
  fi
  lap search2

  occasion=3
  carry=walked
  form=carry
  if carry_holds; then
    stage_ok "carry occasion=3 ok front=own foreground_app=+0 window_title=+0 since the last title behind occasion 2"
  else
    carry=not-walked
    red carry "not walked: foreground_app=+$((c_fa - f_hold)) window_title=+$((c_titles - t_hold)) in the pause before it; the window that passed: class=$other_class file=$other_file; walk front=$front"
    walk_on
  fi
  clicks_stage
  search_stage
  commit_stage
  one_empty_batch_is_the_products
  if episode_holds 12; then
    stage_ok "episode occasion=3 ok form=$form carry=$carry batch end=$b_end records=$b_records episodes=$b_episodes"
  elif [ "$b_end:$b_episodes:$carry:$own:$short" = newest:0:walked:green:green ]; then
    red episode "order=O5 batch end=$b_end records=$b_records episodes=$b_episodes: both returns read green by their counts, and the title of the last return gave the clicks behind the commit no window"
  elif [ "$b_end:$b_episodes:$carry" = newest:0:walked ]; then
    red carry "carry=failed batch end=$b_end records=$b_records episodes=$b_episodes behind the title that the walk's second window had written"
  else
    fail "occasion 3, stage episode: form=$form carry=$carry batch end=$b_end records=$b_records episodes=$b_episodes, wanted end=newest episodes=1 records>=12"
  fi
  lap occasion3

  stage=row
  walk_do row "$row_s"
  row_line=${walk_line#walk: step=row }
  row_line=${row_line#ok=yes }
  field state "$walk_line"
  row=$value
  field proposals "$walk_line"
  row=$row:$value
  field category "$walk_line"
  row=$row:$value
  field count "$walk_line"
  row=$row:$value
  field by_hand "$walk_line"
  row=$row:$value
  now
  if [ -n "$first_red" ]; then
    printf 'seconds:%s walk=%s\n' "$laps" "$((stamp - walk_began))"
    fail "$first_red; row: $row_line"
  fi
  [ "$row" = running:1:spreadsheet_fill:3:no ] || fail "occasion 3, stage row: $row_line, wanted state=running proposals=1 category=spreadsheet_fill count=3 by_hand=no"
  stage_ok "row occasion=3 ok $row_line"
  lap row
  printf 'orders: O4=not-met O5=not-met\n'
  printf 'seconds:%s walk=%s\n' "$laps" "$((stamp - walk_began))"
  show_state
  end_all
}

# The packages whose test targets the merge gate's 12-test leaves out in the windows job, as
# --exclude words: the lines "targets <package> <group>" of the slow list, read as 12-test reads
# them, without the lines whose fourth word is macos. A line that names halvo-capture is passed
# over, so that the call still builds the walk's target.
gate_excludes() {
  tr -d '\r' <"$slow_list" | awk '$1 == "targets" && $4 != "macos" && $2 != "halvo-capture" { printf " --exclude %s", $2 }'
}

# Prints the executable that the build's JSON $1 names for the target of the kind $2 (test or
# bin) and of the name $3, with / between its parts: nothing where the build named none.
named_exe() {
  sed -n 's/.*"kind":\["'"$2"'"\][^}]*"name":"'"$3"'".*"executable":"\([^"]*\)".*/\1/p' "$1" | sed 's/\\\\/\//g'
}

# Prints the features that the build's JSON $1 names for the program $2, with a comma between
# them: nothing where the program has none or the build named no such program.
named_features() {
  sed -n 's/.*"kind":\["bin"\][^}]*"name":"'"$2"'".*"features":\[\([^]]*\)\].*/\1/p' "$1" | tr -d '"'
}

# Behind the first call: notes into $2 each of the four programs that the call's JSON $1 names,
# with its features and the checksum of its file. A test build writes the programs of the
# package of its test target.
first_programs() {
  : >"$2"
  for program in $programs; do
    path=$(named_exe "$1" bin "$program")
    [ -n "$path" ] || continue
    features=$(named_features "$1" "$program")
    sum=$(cksum <"$path") || fail "the program $program that the first call wrote cannot be read: $path"
    printf '%s features=%s %s\n' "$program" "$features" "$sum" >>"$2"
  done
}

# Behind the plain call, by cargo's own words: each of the four programs is named by that
# call's JSON $1 and stands in the directory $3; and where the first call wrote a program of
# the same name with other features ($2 holds its notes), the file there is now another one, so
# no program of the first call is started.
plain_programs_hold() {
  here=$(cd "$3" && pwd -P) || fail "the builds left no directory $3"
  for program in $programs; do
    path=$(named_exe "$1" bin "$program")
    [ -n "$path" ] || fail "the plain build named no program $program; see build.txt"
    there=$(cd "${path%/*}" && pwd -P) || fail "the directory of the program $program cannot be read: $path"
    [ "$there" = "$here" ] || fail "cargo put the program $program outside $3: $path"
    features=$(named_features "$1" "$program")
    sum=$(cksum <"$path") || fail "the program $program cannot be read: $path"
    note=
    while read -r first_name first_features first_crc first_size; do
      [ "$first_name" = "$program" ] || continue
      if [ "$first_features" = "features=$features" ]; then
        note='; the first call built the same program'
      elif [ "$first_crc $first_size" = "$sum" ]; then
        fail "the program $program in $3 is still the one that the test build wrote ($first_features), not the plain one (features=$features)"
      else
        note="; it took the place of the program that the test build wrote under that name ($first_features)"
      fi
    done <"$2"
    printf 'program %s: of the plain build, features=%s, %s/%s%s\n' "$program" "$features" "$3" "${path##*/}" "$note"
  done
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  cd "$repo" || exit 2
  artifacts=${CI_D_ARTIFACTS:-}
  if [ -z "$artifacts" ]; then
    artifacts=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  cr=$(printf '\r')
  now
  built=$stamp
  # Two calls. The first builds the walk's test target, and it is the target call of the merge
  # gate's 12-test narrowed to that one target: it leaves the same packages out, so cargo
  # resolves the same features and plans the same builds, and the build cache that the job
  # restored, which a push run of main saved behind that call, holds each crate of crates.io
  # that it needs. With another selection (the whole workspace, or -p halvo-capture) cargo
  # plans another build of syn 2 and of what stands on it, the windows crate among them: 14
  # crates of crates.io in the hosted run 45821167654.
  # The second call is the plain build of the four programs, and it stands LAST. A build with
  # no test target is resolved with no dev-dependency, and no call of the merge gate builds a
  # plain program of the four, so the cache holds little of it: cargo compiled 100 crates of
  # crates.io for it in 137 s in that run.
  # The first call writes one program too, for its test target: halvo-capture with the two
  # seams that the package turns on for its own tests, as target/debug/halvo-capture. The plain
  # call puts its own program there, and plain_programs_hold holds that: the programs that the
  # walk starts are the plain ones.
  excludes=$(gate_excludes) || fail "$slow_list cannot be read"
  [ -n "$excludes" ] || fail "$slow_list names no package whose test targets the merge gate leaves out"
  # shellcheck disable=SC2086
  cargo test --workspace $excludes --locked --test "$target" --no-run --message-format json >"$work/build.json" 2>"$artifacts/build.txt" ||
    fail "the build of the test target $target failed; see build.txt"
  exe=$(named_exe "$work/build.json" test "$target")
  [ -n "$exe" ] || fail "cargo named no executable for the test target $target"
  case $exe in
    *"
"*) fail "cargo named more than one executable for the test target $target" ;;
  esac
  [ -f "$exe" ] || fail "the test executable that cargo named does not exist"
  cp "$exe" "$work/EXCEL.EXE" || fail "the walk's program cannot be copied to EXCEL.EXE"
  cp "$exe" "$work/noticing-pass.exe" || fail "the walk's program cannot be copied to noticing-pass.exe"
  first_programs "$work/build.json" "$work/first-programs.txt"
  now
  test_built=$stamp
  test_crates=$(grep -c '^ *Compiling ' "$artifacts/build.txt") || test_crates=0
  test_outside=$(grep -c '^ *Compiling [^(]*$' "$artifacts/build.txt") || test_outside=0
  cargo build --locked --workspace --bin halvo --bin halvo-capture --bin halvo-owner --bin halvo-analysis --message-format json >"$work/programs.json" 2>>"$artifacts/build.txt" ||
    fail "cargo build --locked --workspace --bin halvo --bin halvo-capture --bin halvo-owner --bin halvo-analysis failed; see build.txt"
  both_crates=$(grep -c '^ *Compiling ' "$artifacts/build.txt") || both_crates=0
  both_outside=$(grep -c '^ *Compiling [^(]*$' "$artifacts/build.txt") || both_outside=0
  plain_programs_hold "$work/programs.json" "$work/first-programs.txt" target/debug
  now
  printf 'build seconds: %s (the test target: %s s, %s crates compiled, %s of them of crates.io; the four programs: %s s, %s crates compiled, %s of them of crates.io)\n' \
    "$((stamp - built))" "$((test_built - built))" "$test_crates" "$test_outside" \
    "$((stamp - test_built))" "$((both_crates - test_crates))" "$((both_outside - test_outside))"
  [ "$test_outside" = 0 ] ||
    printf 'note: the build cache of the job did not hold %s crates of crates.io that the build of the test target needs; build.txt names them\n' "$test_outside"
  cases target/debug "$work"
  git diff --quiet HEAD -- || fail "a tracked file differs from HEAD after the builds and the walk"
  printf 'PASS %s\n' "$name"
}

# The cases on the stand-ins of the self-test: a made world in the place of the service, of the
# walk and of the clock, so that a case starts no program and waits for no second. The mode $1
# names what that world leaves out; the stages, their bounds and their lines are those of cases.
fixture() {
  mode=$1
  service_s=4
  title_s=4
  clicks_s=4
  witness_s=4
  settle_s=2
  search_s=80
  search_poll_s=5
  commit_s=4
  second_s=4
  early_s=30
  step_s=5
  row_s=5
  w_clock=1000
  w_idle=40
  w_pointer=0
  w_titles=0
  w_tbytes=0
  w_fa=0
  w_lost=0
  w_records=0
  w_last_input=1000
  w_unsearched=no
  w_batches=0
  w_front=halvo
  w_last_title=none
  w_second_due=
  to_native() {
    native=$1
  }
  mask() {
    while IFS= read -r line || [ -n "$line" ]; do
      printf '%s\n' "$line"
    done
  }
  now() {
    stamp=$w_clock
  }
  pause() {
    w_clock=$((w_clock + $1))
    if [ -n "$w_second_due" ] && [ "$w_clock" -ge "$w_second_due" ]; then
      w_second_due=
      world_second_pair
    fi
    world_search
  }
  start_app() {
    : >"$err"
  }
  start_walk() {
    :
  }
  end_all() {
    :
  }
  # The world's search: behind w_idle s with no input it writes the worker's batch line and
  # the service's commit line for the records that no batch has read. Then, as the service
  # does, it starts a second worker at once from the new position: that one reads nothing and
  # writes a second batch line, with no record, and a second commit line.
  world_search() {
    [ "$w_unsearched" = yes ] || return 0
    [ $((w_clock - w_last_input)) -ge "$w_idle" ] || return 0
    w_unsearched=no
    w_batches=$((w_batches + 1))
    episodes=1
    case $mode:$w_batches in
      no-batch-line:1) return 0 ;;
      carry-not-walked:1) w_fa=$((w_fa + 1)) ;;
      episodes-zero-at-occasion-one:1 | plain-carry-fails:2 | order-O5:3) episodes=0 ;;
      one-empty-batch-line:1)
        {
          printf 'batch end=newest run_open=no start_first=no records=0 episodes=0\n'
          printf 'halvo-capture: analysis end=committed code=0 peak_bytes=1048576\n'
        } >>"$err"
        return 0
        ;;
    esac
    printf 'batch end=newest run_open=no start_first=no records=%s episodes=%s\n' "$w_records" "$episodes" >>"$err"
    w_records=0
    case $mode:$w_batches in
      no-commit-line:1)
        printf 'halvo-capture: analysis end=cap code=- peak_bytes=268435456\n' >>"$err"
        return 0
        ;;
    esac
    printf 'halvo-capture: analysis end=committed code=0 peak_bytes=1048576\n' >>"$err"
    if [ "$mode" = second-pair-comes-late ]; then
      w_second_due=$((w_clock + 2))
      return 0
    fi
    world_second_pair
  }
  # The second worker's batch line, with no record, and its commit line. In the mode
  # second-pair-comes-late the world writes them 2 s behind the first pair, as a machine did.
  world_second_pair() {
    {
      printf 'batch end=newest run_open=no start_first=no records=0 episodes=0\n'
      printf 'halvo-capture: analysis end=committed code=0 peak_bytes=524288\n'
    } >>"$err"
  }
  # The world records $2 events of the kind $1; a title adds the bytes $3.
  world_adds() {
    case $1 in
      pointer) w_pointer=$((w_pointer + $2)) ;;
      fa) w_fa=$((w_fa + $2)) ;;
      *)
        w_titles=$((w_titles + $2))
        w_tbytes=$((w_tbytes + $3))
        ;;
    esac
    w_records=$((w_records + $2))
    w_unsearched=yes
  }
  ctl() {
    ctl_code=0
    case $1 in
      status)
        lease=fresh
        [ "$mode" = service-no-fresh-lease ] && lease=none
        ctl_out="protocol=1
desired=running
capture=running
capture_reason=-
lease=$lease
store=open
events_pending=0
events_lost=$w_lost
title_read_errors=0
last_flush_at=$w_clock
source name=foreground state=running
source name=keyboard state=running
source name=pointer state=running
source name=window_title state=running"
        ;;
      inventory)
        ctl_out="store_id=018f6f2e-0000-7000-8000-000000000001
row day=2026-11-26 modality=foreground_app events=$w_fa bytes=$((w_fa * 40))
row day=2026-11-26 modality=pointer events=$w_pointer bytes=$((w_pointer * 30))
row day=2026-11-26 modality=window_title events=$w_titles bytes=$w_tbytes
total events=$((w_fa + w_pointer + w_titles)) bytes=0 disk_bytes=0"
        ;;
      *)
        ctl_out=
        ctl_code=64
        ;;
    esac
  }
  walk_do() {
    case $1 in
      ready)
        window=yes
        [ "$mode" = service-no-window-of-the-app ] && window=no
        walk_line="walk: step=ready ok=yes pid=4242 halvo_window=$window"
        ;;
      passer)
        walk_line='walk: step=passer ok=yes pid=4343'
        [ "$mode" != service-no-window-of-the-passer ] || walk_line='walk: step=passer ok=no why=no-passer-window'
        ;;
      front)
        if [ "$mode:$w_pointer" = another-window-covers:0 ]; then
          walk_line='walk: step=front ok=no why=covered front_class=Shell_TrayWnd front_file=explorer.exe'
          printf '%s\n' "$walk_line"
          return 0
        fi
        world_adds pointer 1
        [ "$w_front" = own ] || world_adds fa 1
        w_front=own
        if [ "$w_last_title" != workbook ] && [ "$mode:$w_titles" != no-title-count:0 ]; then
          world_adds title 1 60
          w_last_title=workbook
        fi
        w_last_input=$w_clock
        walk_line='walk: step=front ok=yes front=own'
        ;;
      clicks)
        sent=12
        [ "$mode" = eleven-clicks ] && sent=11
        world_adds pointer "$sent"
        case $mode:$w_batches in
          an-event-lost:0) w_lost=1 ;;
          a-window-passes-behind-the-clicks:1) world_adds fa 1 ;;
        esac
        w_last_input=$w_clock
        walk_line='walk: step=clicks ok=yes sent=12 front=own'
        ;;
      own)
        world_adds fa 1
        case $mode in
          return-own-no-title) ;;
          two-titles-at-each-return) world_adds title 2 120 ;;
          a-shorter-title-beside-the-workbooks) world_adds title 2 106 ;;
          return-own-two-records)
            world_adds fa 1
            world_adds title 1 60
            ;;
          *) world_adds title 1 60 ;;
        esac
        world_adds pointer 1
        w_last_input=$w_clock
        walk_line='walk: step=own ok=yes away=call stay_ms=3004 fg_events=2 front=own'
        ;;
      short)
        events=2
        world_adds fa 2
        case $mode in
          return-short-no-title) ;;
          order-O4) events=0 ;;
          return-short-not-met-in-three-tries) world_adds title 2 180 ;;
          two-titles-at-each-return) world_adds title 2 120 ;;
          *) world_adds title 1 60 ;;
        esac
        walk_line="walk: step=short ok=yes stay_ms=1 fg_events=$events front=own"
        ;;
      notes)
        world_adds pointer 1
        world_adds title 1 50
        w_last_title=notes
        w_last_input=$w_clock
        walk_line='walk: step=notes ok=yes front=own'
        ;;
      look)
        other='last_other_class=- last_other_file=-'
        [ "$mode:$w_batches" = carry-not-walked:1 ] && other='last_other_class=Shell_TrayWnd last_other_file=explorer.exe'
        walk_line="walk: step=look ok=yes front=own idle_ms=250000 $other"
        ;;
      row)
        row='proposals=1 stored=1 category=spreadsheet_fill count=3 by_hand=no'
        case $mode in
          no-row) row='proposals=0 stored=0 category=- count=0 by_hand=-' ;;
          row-of-count-two | order-O5) row='proposals=0 stored=0 category=- count=0 by_hand=-' ;;
        esac
        [ "$mode" = row-of-count-two ] && row='proposals=1 stored=1 category=spreadsheet_fill count=2 by_hand=no'
        walk_line="walk: step=row ok=yes state=running $row"
        ;;
      *) walk_line="walk: step=$1 ok=no why=unknown-step" ;;
    esac
    printf '%s\n' "$walk_line"
  }
  mkdir -p "$2/bin" "$2/data/logs" "$2/steps" || exit 2
  for program in halvo halvo-capture halvo-owner halvo-analysis; do
    : >"$2/bin/$program"
  done
  cases "$2/bin" "$2"
  printf 'PASS %s\n' "$name"
}

# Runs the case $1 on the stand-ins and holds its verdict against $2, PASS or FAIL. A FAIL must
# hold the text $3 in its last line, and the text $4 there too where the case gives one; a PASS
# must hold $3 in any line.
run_case() {
  sh "$self" --fixture "$1" "$work/$1" >"$work/$1.out" 2>&1
  code=$?
  last=
  holds=no
  while IFS= read -r line; do
    last=$line
    case $line in
      *"$3"*) [ "$2" = FAIL ] || holds=yes ;;
    esac
  done <"$work/$1.out"
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $2:$last in
    FAIL:*"$3"*) holds=yes ;;
  esac
  if [ -n "${4:-}" ]; then
    case $last in
      *"$4"*) ;;
      *) holds=no ;;
    esac
  fi
  [ "$holds" = yes ] || got=$got-without-the-text
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    while IFS= read -r line; do
      printf '  %s: %s\n' "$1" "$line"
    done <"$work/$1.out"
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  dir=$(cd "$(dirname "$0")" && pwd) || exit 2
  self=$dir/$(basename "$0")
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  wrong=
  rejected=0
  run_case green PASS 'stage row occasion=3 ok state=running proposals=1 stored=1 category=spreadsheet_fill count=3 by_hand=no'
  run_case two-titles-at-each-return PASS 'stage return-short occasion=2 ok foreground_app=+2 window_title=+2 title_bytes=+120, the titles are the workbook'"'"'s, wanted +2 and +1 or +2;'
  run_case second-pair-comes-late PASS 'second worker occasion=3: 1 batch lines behind the own one, 1 commit lines behind its commit, waited 2 s'
  run_case service-no-fresh-lease FAIL '(occasion 0, stage service: within 4 s ctl status read desired=running capture=running capture_reason=- lease=none'
  run_case service-no-window-of-the-app FAIL '(occasion 0, stage service: no window of the app'
  run_case no-title-count FAIL '(occasion 1, stage title: window_title=+0 within 4 s behind the click into the window; pointer=+1 foreground_app=+1; events_pending=0 events_lost=0 title_read_errors=0 last_flush_at='
  run_case eleven-clicks FAIL '(occasion 1, stage clicks: pointer=+11 of 12 within 4 s; events_pending=0 events_lost=0 title_read_errors=0 last_flush_at='
  run_case no-batch-line FAIL '(occasion 1, stage search: no batch line within 80 s of the last input; walk idle_ms=250000;'
  run_case episodes-zero-at-occasion-one FAIL '(occasion 1, stage episode: form=new-title batch end=newest records=15 episodes=0, wanted end=newest episodes=1 records>=13'
  run_case no-commit-line FAIL '(occasion 1, stage commit: no analysis end=committed line within 4 s of the occasion'"'"'s batch line; analysis lines: | analysis end=cap code=- peak_bytes=268435456)'
  # A red stage that lets the walk go on ends with the row behind it in the same last line.
  row_three='; row: state=running proposals=1 stored=1 category=spreadsheet_fill count=3 by_hand=no)'
  run_case service-no-window-of-the-passer FAIL '(occasion 0, stage service: the second program of the check showed no window: walk: step=passer ok=no why=no-passer-window)'
  run_case another-window-covers FAIL '(occasion 1, stage title: another window covers the walk'"'"'s window: class=Shell_TrayWnd file=explorer.exe; events_pending=0 events_lost=0 title_read_errors=0 last_flush_at='
  run_case an-event-lost FAIL '(occasion 1, stage clicks: events_lost went from 0 to 1; pointer=+12; events_pending=0 events_lost=1 title_read_errors=0 last_flush_at='
  run_case one-empty-batch-line FAIL '(occasion 1, stage episode: the product'"'"'s fault: the pause shows one batch line with records=0 and the records counted in the store: pointer=+12 behind the occasion'"'"'s baseline; batch end=newest run_open=no start_first=no records=0 episodes=0)'
  run_case a-window-passes-behind-the-clicks FAIL '(occasion 2, stage carry: not walked: foreground_app=+1 window_title=+0 behind its clicks' "$row_three"
  run_case return-own-no-title FAIL '(occasion 2, stage return-own: foreground_app=+1 window_title=+0 title_bytes=+0, no title, wanted +1 and +1 or +2; walk away=call fg_events=2 stay_ms=3004;' "$row_three"
  run_case return-own-two-records FAIL '(occasion 2, stage return-own: foreground_app=+2 window_title=+1 title_bytes=+60, the titles are the workbook'"'"'s, wanted +1 and +1 or +2; walk away=call fg_events=2 stay_ms=3004;' "$row_three"
  run_case a-shorter-title-beside-the-workbooks FAIL '(occasion 2, stage return-own: foreground_app=+1 window_title=+2 title_bytes=+106, a title of another window is among them, wanted +1 and +1 or +2; walk away=call fg_events=2 stay_ms=3004;' "$row_three"
  run_case return-short-no-title FAIL '(occasion 2, stage return-short: foreground_app=+2 window_title=+0 title_bytes=+0, no title, wanted +2 and +1 or +2; walk fg_events=2 stay_ms=1 try=1;' "$row_three"
  run_case return-short-not-met-in-three-tries FAIL '(occasion 2, stage return-short: return-short=not-met in 3 tries: foreground_app=+2 window_title=+2' "$row_three"
  run_case order-O4 FAIL '(occasion 2, stage return-short: order=O4 foreground_app=+2 window_title=+0' "$row_three"
  run_case order-O5 FAIL '(occasion 3, stage episode: order=O5 batch end=newest records=12 episodes=0' '; row: state=running proposals=0 stored=0 category=- count=0 by_hand=-)'
  run_case plain-carry-fails FAIL '(occasion 2, stage carry: carry=failed batch end=newest records=' "$row_three"
  run_case carry-not-walked FAIL '(occasion 2, stage carry: not walked: foreground_app=+1 window_title=+0 in the pause before it; the window that passed: class=Shell_TrayWnd file=explorer.exe' "$row_three"
  run_case no-row FAIL '(occasion 3, stage row: state=running proposals=0 stored=0 category=- count=0 by_hand=-, wanted state=running proposals=1 category=spreadsheet_fill count=3 by_hand=no)'
  run_case row-of-count-two FAIL '(occasion 3, stage row: state=running proposals=1 stored=1 category=spreadsheet_fill count=2 by_hand=no, wanted state=running proposals=1 category=spreadsheet_fill count=3 by_hand=no)'
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --fixture) fixture "$2" "$3" ;;
  '') measure ;;
  *) exit 64 ;;
esac
