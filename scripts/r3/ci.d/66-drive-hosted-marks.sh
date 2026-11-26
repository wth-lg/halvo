#!/bin/sh
# jobs: windows
# rule: The form of the driver trial with the runner's mask (r3 U0.10) holds against its scripted host on 127.0.0.1: hosted.sh --marks --standin, at a checkout with no changed tracked file, reads as expected with the door under the egress check; no run reaches a host. It joins no slice and no merge gate: it runs by hand with named checks, the windows job alone, at each change of the trial tool, of the door or of the runner's maker of a snapshot. It builds the tool and the door for itself.
# lane: alone
set -u
name=66-drive-hosted-marks

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# The defect of the one case: $1 the case, $2 its exit status, $3 its output; none when the case holds.
defect_of() {
  case $1 in
    standin-marks)
      last=$(awk 'END { print }' "$3")
      case $2:$last in
        '0:control mode=hosted-marks machine='*' result=as_expected') return 0 ;;
      esac
      printf 'hosted.sh --marks --standin exited %s and its last line is no control line with result=as_expected: read %s/standin-marks.txt of the artifacts, whose line control readings names the reading that differs\n' "$2" "$name"
      return 1
      ;;
  esac
  return 1
}

seconds() {
  date +%s
}

# Keeps the files of a failed stand-in round that name its cause: the round's lines stand in
# standin-marks.txt already; each cell's trace.txt and model-stderr.txt (the door's standard error with
# the egress check's line) of the round's directory under TMPDIR are copied beside them. No copied
# file holds the made-up key, which hosted.sh writes into a directory of its own, or a request line.
keep_round() {
  for cell in "${TMPDIR:-/tmp}"/r3-hosted-round.*/*-seed-*; do
    [ -d "$cell" ] || continue
    for file in trace.txt model-stderr.txt; do
      if [ -f "$cell/$file" ]; then
        cp "$cell/$file" "$CI_D_ARTIFACTS/standin-${cell##*/}-$file"
      fi
    done
  done
}

measure() {
  cd "$CI_D_REPO" || exit 2
  [ -n "${HALVO_BROWSER:-}" ] && [ -f "$HALVO_BROWSER" ] || fail "HALVO_BROWSER names no browser; the job's step gives it"
  # The control of the form with the runner's mask, at the commit of the checkout, which must hold no
  # changed tracked file.
  e=$(seconds)
  sh tools/r3-trials/drive/hosted.sh --marks --standin --ceiling 2000 --browser "$HALVO_BROWSER" --port 18471 \
    >"$CI_D_ARTIFACTS/standin-marks.txt" 2>"$CI_D_ARTIFACTS/standin-marks-err.txt"
  code=$?
  f=$(seconds)
  defect=$(defect_of standin-marks "$code" "$CI_D_ARTIFACTS/standin-marks.txt") || {
    keep_round
    fail "$defect"
  }
  printf 'case %s/standin-marks cells=%s whole_s=%s\n' "$name" "$(grep -c '^round cell task=' "$CI_D_ARTIFACTS/standin-marks-err.txt")" "$((f - e))"
  printf 'PASS %s\n' "$name"
}

case_of() {
  printf '%s\n' "$4" >"$work/$1.txt"
  got=PASS
  defect_of "$2" "$3" "$work/$1.txt" >/dev/null || got=FAIL
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$5" "$got"
  if [ "$got" != "$5" ]; then wrong="$wrong $1"; elif [ "$5" = FAIL ]; then rejected=$((rejected + 1)); fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  case_of standin-marks-as-expected standin-marks 0 "control readings lines=as_expected sent=153 past_the_ceiling=0 unmarked=0 slot_texts=24 held=0 read_words=60 marks=3 files=300 keyed=0 block=as_expected
control mode=hosted-marks machine=hosted-win result=as_expected" PASS
  case_of standin-marks-differs standin-marks 1 "control readings lines=as_expected sent=153 past_the_ceiling=0 unmarked=0 slot_texts=24 held=2 read_words=60 marks=3 files=300 keyed=0 block=as_expected
control mode=hosted-marks machine=hosted-win result=differs" FAIL
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
