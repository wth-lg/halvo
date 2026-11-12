#!/bin/sh
# jobs: macos
# rule: TLC finds none of the eleven invariants of the spend model's Spend.cfg violated, the machine of one hold gives every committed trace with all 30 pairs and 8 causes, and each of the two controls breaks its one invariant; the macos job alone runs it, because the model and the checker, one pinned jar, are the same on both systems and the windows job printed the same lines in run 43294110696.
set -u
name=55-r6-formal
model=formal/r6-spend
inputs='README.md Hold.tla Spend.tla Spend.cfg ControlRuling.cfg ControlResend.cfg Replay.tla Replay.cfg traces.jsonl'
invariants='S1 S2 S3 S4 S5 S6 S7 S8 S9 S10 S11'
jar_bytes=2274532
jar_sha256=936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88
tlc_banner='TLC2 Version 2.19 of 08 August 2024 (rev: 5a47802)'
config='CONSTANTS
Null = Null
Runs = {r1, r2}
MaxRequests = 2
MaxAdmissions = 2
MaxCapChanges = 2
MaxOtherSpends = 2
MaxLostAnswers = 1
MaxStops = 1
Hurry = TRUE
TraceMoves = 12
Fault = ?
SYMMETRY RunSymmetry
INIT Init
NEXT Next
CHECK_DEADLOCK FALSE'
wrong_values='{"seed":1,"count":6}
{"cap":3,"active":"yes","spent":0,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":0,"held":1,"cost":0}]}
{"cap":3,"active":"yes","spent":0,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"over_cap","state":"held","spent":0,"held":1,"cost":0}]}
{"cap":3,"active":"yes","spent":0,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"sent","spent":0,"held":1,"cost":0}]}
{"cap":3,"active":"yes","spent":0,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":1,"held":1,"cost":0}]}
{"cap":3,"active":"yes","spent":0,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":0,"held":2,"cost":0}]}
{"cap":3,"active":"yes","spent":0,"held":0,"moves":[{"move":"reserve","arg":1,"answer":"ok","state":"held","spent":0,"held":1,"cost":1}]}'
cause=

setup_failed() {
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

first() {
  [ -n "$cause" ] || cause=$1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || setup_failed
  fi
  cd "$repo" || setup_failed
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || setup_failed
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || setup_failed
  work=$(cd "$work" && pwd) || setup_failed
  mkdir "$work/in" || setup_failed
  printf '%s\n' "$config" >"$work/config.want" || setup_failed
}

read_inputs() {
  missing=
  for file in $inputs; do
    git cat-file blob "HEAD:$model/$file" >"$work/in/$file" 2>/dev/null || missing="$missing $file"
  done
}

present() {
  for file in "$@"; do
    case "$missing " in
      *" $file "*) return 1 ;;
    esac
  done
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256
  else
    return 1
  fi
}

get_jar() {
  present README.md || return 2
  pin=$(sed -n 's/^tlc url=\([^ ]*\) bytes=\([1-9][0-9]*\) sha256=\([0-9a-f]\{64\}\)$/\1 \2 \3/p' "$work/in/README.md")
  case $pin in
    '' | *"
"*) return 2 ;;
  esac
  url=${pin%% *}
  sha=${pin##* }
  bytes=${pin#* }
  bytes=${bytes% *}
  [ "$bytes" = "$jar_bytes" ] && [ "$sha" = "$jar_sha256" ] || return 2
  command -v curl >/dev/null 2>&1 || return 1
  curl -q -fsSL --retry 2 --max-time 300 -o "$work/tla2tools.jar" "$url" >"$artifacts/curl.txt" 2>&1 </dev/null || return 1
  [ -f "$work/tla2tools.jar" ] || return 1
  size=$(wc -c <"$work/tla2tools.jar") || return 2
  size=$((size + 0))
  sum=$(sha256 <"$work/tla2tools.jar") || return 2
  sum=${sum%% *}
  [ "$size" = "$bytes" ] && [ "$sum" = "$sha" ] || return 2
}

find_java() {
  unset JAVA_TOOL_OPTIONS JDK_JAVA_OPTIONS _JAVA_OPTIONS
  if [ -n "${JAVA_HOME:-}" ]; then
    java=$JAVA_HOME/bin/java
  else
    java=java
  fi
  found=$(command -v "$java") || return 1
  case $found in
    /* | [A-Za-z]:*) java=$found ;;
    *) java=$(pwd)/$found ;;
  esac
  "$java" -version >"$work/java.raw" 2>&1 </dev/null || return 1
  tr -d '\r' <"$work/java.raw" >"$artifacts/java.txt" || return 1
  major=$(sed -n 's/.* version "\([0-9][0-9]*\).*/\1/p' "$artifacts/java.txt")
  major=${major%%"
"*}
  case $major in
    '' | *[!0-9]*) return 1 ;;
  esac
  [ "$major" -ge 17 ]
}

prepare() {
  get_jar
  case $? in
    0) ;;
    1) first fetch ;;
    *) first hash ;;
  esac
  [ -z "$cause" ] || return 1
  find_java || first java
  [ -z "$cause" ]
}

count_workers() {
  workers=$(nproc 2>/dev/null) || workers=
  [ -n "$workers" ] || workers=$(getconf _NPROCESSORS_ONLN 2>/dev/null) || workers=
  [ -n "$workers" ] || workers=$(sysctl -n hw.ncpu 2>/dev/null) || workers=
  [ -n "$workers" ] || workers=${NUMBER_OF_PROCESSORS:-}
  case $workers in
    '' | *[!0-9]* | 0*) workers=1 ;;
  esac
  [ "$workers" -le 4 ] || workers=4
}

read_config() {
  awk -v norm="$2" -v want="$3" '
    BEGIN {
      split("CONSTANT CONSTANTS INIT NEXT SPECIFICATION INVARIANT INVARIANTS PROPERTY PROPERTIES SYMMETRY VIEW CONSTRAINT CONSTRAINTS ACTION_CONSTRAINT ACTION_CONSTRAINTS CHECK_DEADLOCK POSTCONDITION ALIAS", list, " ")
      for (i in list) key[list[i]] = 1
      wanted = split(want, list, " ")
      for (i in list) wants[list[i]] = 1
      printf "" >norm
    }
    {
      line = $0
      if (index(line, "(*") || index(line, "*)") || index(line, "\r")) bad++
      at = index(line, "\\*")
      if (at) line = substr(line, 1, at - 1)
      n = split(line, word, /[ \t]+/)
      out = ""
      for (i = 1; i <= n; i++) {
        w = word[i]
        if (w == "") continue
        if (w in key) section = w
        if (section == "INVARIANT" || section == "INVARIANTS") {
          if (w in key) continue
          if (w in seen) bad++
          seen[w] = 1
          count++
          if (!(w in wants)) bad++
          continue
        }
        if (w == "Fault") {
          faults++
          if (word[i + 1] == "=" && word[i + 2] ~ /^"[a-z_]+"$/) fault = word[i + 2]
          else bad++
          out = out " Fault = ?"
          i += 2
          continue
        }
        out = out " " w
      }
      if (out != "") print substr(out, 2) >norm
    }
    END {
      if (faults != 1 || count != wanted) bad++
      printf "%d %s %d\n", count, (fault == "" ? "-" : fault), bad
    }' "$1"
}

run_tlc() {
  mkdir "$1/jtmp" || return 1
  (
    cd "$1" || exit 1
    "$java" -Xmx2g -XX:+UseParallelGC -Djava.io.tmpdir=jtmp -cp ../tla2tools.jar tlc2.TLC \
      -workers "$4" -metadir meta -config "$3" "$2" >out.raw 2>&1 </dev/null
  )
  status=$?
  tr -d '\r' <"$1/out.raw" >"$artifacts/${1##*/}.txt" || return 1
  banners=$(grep -cxF "$tlc_banner" "$artifacts/${1##*/}.txt") || return 1
  [ "$banners" = 1 ] || return 1
  awk '
    /^[0-9]+ states generated, [0-9]+ distinct states found, [0-9]+ states left on queue\.$/ { lines++; distinct = $4; left = $8 }
    /^Error: Invariant [A-Za-z0-9_]+ is violated\.$/ { broken = broken (broken == "" ? "" : ",") $3 }
    $0 == "Model checking completed. No error has been found." { done = 1 }
    END {
      if (lines != 1) { distinct = "-"; left = "-" }
      printf "%s %s %d %s\n", distinct, left, done, (broken == "" ? "-" : broken)
    }' "$artifacts/${1##*/}.txt" >"$1/summary.txt" || return 1
  read -r distinct left finished broken <"$1/summary.txt"
}

spend() {
  states=-
  listed=-
  violated=-
  fault=-
  bad=1
  dir=$work/spend
  if present Hold.tla Spend.tla Spend.cfg && mkdir "$dir"; then
    cp "$work/in/Hold.tla" "$work/in/Spend.tla" "$work/in/Spend.cfg" "$dir/" || setup_failed
    read_config "$dir/Spend.cfg" "$work/spend.norm" "$invariants" >"$work/spend.config" || setup_failed
    read -r count fault bad <"$work/spend.config"
    cmp -s "$work/config.want" "$work/spend.norm" || bad=1
    if run_tlc "$dir" Spend.tla Spend.cfg "$workers"; then
      if [ "$status" -eq 0 ] && [ "$finished" -eq 1 ] && [ "$left" = 0 ] && [ "$broken" = - ]; then
        states=$distinct
        listed=$count
        violated=no
      elif [ "$broken" != - ] && [ "$distinct" != - ]; then
        states=$distinct
        listed=$count
        violated=yes
      fi
    fi
  fi
  printf 'formal states=%s invariants=%s violated=%s\n' "$states" "$listed" "$violated"
  if [ "$violated" != no ] || [ "$listed" != 11 ] || [ "$fault" != '"none"' ] || [ "$bad" -ne 0 ]; then
    first model
  fi
}

module() {
  {
    printf -- '---- MODULE Committed ----\nEXTENDS Hold\nTraces == <<\n'
    sed -e 1d -e 's/\[/<</g' -e 's/\]/>>/g' -e 's/{/[/g' -e 's/}/]/g' \
      -e 's/"\([a-z]*\)":/\1 |-> /g' -e 's/null/Null/g' -e '$!s/$/,/' "$1"
    printf '>>\n====\n'
  } >"$2/Committed.tla" || setup_failed
}

told() {
  told=-
  if run_tlc "$1" Replay.tla Replay.cfg 1 && [ "$status" -eq 0 ]; then
    told=$(sed -n 's/^"replay count=\([0-9]*\) accepted=\([0-9]*\) pairs=\([0-9]*\) causes=\([0-9]*\)"$/\1 \2 \3 \4/p' "$artifacts/${1##*/}.txt")
    case $told in
      '' | *"
"*) told=- ;;
    esac
  fi
}

replay_control() {
  dir=$work/replay-control
  wrong=-
  if present Hold.tla Replay.tla Replay.cfg && mkdir "$dir"; then
    cp "$work/in/Hold.tla" "$work/in/Replay.tla" "$work/in/Replay.cfg" "$dir/" || setup_failed
    printf '%s\n' "$wrong_values" >"$work/wrong-values.jsonl" || setup_failed
    module "$work/wrong-values.jsonl" "$dir"
    told "$dir"
    wrong=$told
  fi
  w_count=-
  w_accepted=-
  w_pairs=-
  w_causes=-
  [ "$wrong" = - ] || read -r w_count w_accepted w_pairs w_causes <<EOF
$wrong
EOF
  printf 'formal replay control count=%s accepted=%s pairs=%s causes=%s\n' "$w_count" "$w_accepted" "$w_pairs" "$w_causes"
  [ "$wrong" = '6 1 1 1' ] || first traces
}

replay() {
  traces=-
  accepted=-
  pairs=-
  causes=-
  dir=$work/replay
  if present Hold.tla Replay.tla Replay.cfg traces.jsonl && mkdir "$dir"; then
    cp "$work/in/Hold.tla" "$work/in/Replay.tla" "$work/in/Replay.cfg" "$dir/" || setup_failed
    lines=$(awk '
      NR == 1 {
        if ($0 !~ /^\{"seed":[1-9][0-9]*,"count":[1-9][0-9]*\}$/) bad++
        header = $0
        sub(/^.*"count":/, "", header)
        sub(/\}$/, "", header)
        next
      }
      {
        moves = $0
        n = gsub(/\{"move":"(reserve|settle)","arg":[0-3],"answer":"[a-z_]+","state":"[a-z_]+","spent":(0|[1-9][0-9]*),"held":(0|[1-9][0-9]*),"cost":(0|[1-9][0-9]*)\}|\{"move":"(dispatch|void|expire)","arg":null,"answer":"[a-z_]+","state":"[a-z_]+","spent":(0|[1-9][0-9]*),"held":(0|[1-9][0-9]*),"cost":(0|[1-9][0-9]*)\}/, "M", moves)
        count++
        if (n != (count <= 6 ? 5 : 12)) bad++
        if (moves !~ /^\{"cap":(null|[0-3]),"active":"(yes|no)","spent":[0-2],"held":[0-2],"moves":\[M(,M)*\]\}$/) bad++
      }
      END {
        if (NR < 2 || header + 0 != count || count < 206) bad++
        print (bad ? "-" : count)
      }' "$work/in/traces.jsonl") || lines=-
    if [ "$lines" != - ]; then
      module "$work/in/traces.jsonl" "$dir"
      told "$dir"
      [ "$told" = - ] || read -r traces accepted pairs causes <<EOF
$told
EOF
    fi
  fi
  printf 'formal traces count=%s accepted=%s pairs=%s causes=%s\n' "$traces" "$accepted" "$pairs" "$causes"
  if [ "$traces" = - ] || [ "$traces" != "$lines" ] || [ "$accepted" != "$traces" ] || [ "$pairs" != 30 ] || [ "$causes" != 8 ]; then
    first traces
  fi
  replay_control
}

control() {
  violated=-
  dir=$work/control-$1
  if present Hold.tla Spend.tla Spend.cfg "$2" && mkdir "$dir"; then
    cp "$work/in/Hold.tla" "$work/in/Spend.tla" "$work/in/$2" "$dir/" || setup_failed
    read_config "$dir/$2" "$dir/control.norm" "$3" >"$dir/control.config" || setup_failed
    read -r count fault bad <"$dir/control.config"
    if [ "$bad" -eq 0 ] && [ "$fault" = "\"$4\"" ] && cmp -s "$work/config.want" "$dir/control.norm" &&
      run_tlc "$dir" Spend.tla "$2" "$workers"; then
      if [ "$status" -eq 12 ] && [ "$broken" = "$3" ]; then
        violated=yes
      elif [ "$status" -eq 0 ] && [ "$finished" -eq 1 ] && [ "$broken" = - ]; then
        violated=no
      fi
    fi
  fi
  printf 'formal control=%s invariant=%s violated=%s\n' "$1" "$3" "$violated"
  if [ "$violated" = yes ]; then
    rejected=$((rejected + 1))
  else
    first control
  fi
}

skipped() {
  printf 'formal states=- invariants=- violated=-\n'
  printf 'formal traces count=- accepted=- pairs=- causes=-\n'
  printf 'formal replay control count=- accepted=- pairs=- causes=-\n'
  printf 'formal control=ruling invariant=S1 violated=-\n'
  printf 'formal control=resend invariant=S5 violated=-\n'
}

measure() {
  enter_repo
  read_inputs
  count_workers
  rejected=0
  if prepare; then
    printf 'formal workers=%s\n' "$workers"
    spend
    replay
    control ruling ControlRuling.cfg S1 grant_without_ruling
    control resend ControlResend.cfg S5 resend_new_id
  else
    skipped
  fi
  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

self_test() {
  enter_repo
  read_inputs
  count_workers
  rejected=0
  if prepare; then
    control ruling ControlRuling.cfg S1 grant_without_ruling
    control resend ControlResend.cfg S5 resend_new_id
  else
    printf 'formal control=ruling invariant=S1 violated=-\n'
    printf 'formal control=resend invariant=S5 violated=-\n'
  fi
  if [ -n "$cause" ]; then
    printf 'FAIL self-test:%s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $# in
  0) measure ;;
  *)
    if [ "$#" -eq 1 ] && [ "$1" = --self-test ]; then
      self_test
    else
      printf 'FAIL %s (usage)\n' "$name"
      exit 1
    fi
    ;;
esac
