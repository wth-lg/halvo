#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=windows-ci

facts() {
  printf '%s\n' "$@" > "$gate_scratch/facts"
}

verdict() {
  facts_sha=$1; shift
  facts "$@"
  predicate_ci "$facts_sha" "$gate_scratch/facts"
}

defect_is() {
  facts_want=$1; facts_sha=$2; shift 2
  facts "$@"
  defect=$(ci_defect "$facts_sha" "$gate_scratch/facts")
  [ "$defect" = "$facts_want" ]
}

with_a_stub_gh() {
  stub_bin=$gate_scratch/bin-$st_case
  mkdir -p "$stub_bin"
  printf '%s\n' "$2" > "$stub_bin/runs"
  printf '%s\n' "$3" > "$stub_bin/jobs"
  cat > "$stub_bin/gh" <<'STUB'
#!/bin/sh
here=${0%/*}
case "$1 $2" in
  'run list') cat "$here/runs" ;;
  'run view') cat "$here/jobs" ;;
  *) exit 1 ;;
esac
STUB
  chmod 755 "$stub_bin/gh"
  PATH="$stub_bin:$PATH"
  gate_sha=$1
  measure
}

cases() {
  scratch_root
  sha=5d71834b43d3cedf77edf36ac4dcf5d62b728ad0
  other=685d8460000000000000000000000000000000aa
  push="run 42619829679 push $sha completed success"
  pull="run 42619929912 pull_request $sha completed failure"
  t push-run-green PASS verdict "$sha" "$push" "job macos success 21" "job windows success 18"
  t failed-pull-request-run-beside-a-green-push-run PASS verdict "$sha" "$pull" "$push" "job macos success 21" "job windows success 18"
  t pull-request-run-only FAIL verdict "$sha" "$pull"
  t no-push-run FAIL verdict "$sha"
  t sha-empty FAIL verdict "" "$push" "job windows success 18"
  t sha-HEAD FAIL verdict HEAD "$push" "job windows success 18"
  t sha-39 FAIL verdict 5d71834b43d3cedf77edf36ac4dcf5d62b728ad "$push" "job windows success 18"
  t sha-uppercase FAIL verdict 5D71834B43D3CEDF77EDF36AC4DCF5D62B728AD0 "$push" "job windows success 18"
  t head-sha-differs FAIL verdict "$sha" "run 1 push $other completed success" "job windows success 18"
  t windows-zero-steps FAIL verdict "$sha" "$push" "job macos success 21" "job windows success 0"
  t windows-job-failed FAIL verdict "$sha" "$push" "job macos success 21" "job windows failure 18"
  t windows-job-absent FAIL verdict "$sha" "$push" "job macos success 21"
  t windows-job-still-running FAIL verdict "$sha" "run 2 push $sha in_progress none" "job macos success 21" "job windows none 9"
  t one-of-two-push-runs-failed FAIL verdict "$sha" "$push" "job windows success 18" "run 3 push $sha completed failure" "job windows failure 18"
  t two-green-push-runs PASS verdict "$sha" "$push" "job windows success 18" "run 3 push $sha completed success" "job windows success 18"
  t windows-job-of-a-pull-request-run-does-not-count FAIL verdict "$sha" "$push" "job macos success 21" "$pull" "job windows success 18"
  t zero-steps-is-named PASS defect_is "push run 42619829679: the windows job did not start (zero steps)" "$sha" "$push" "job windows success 0"
  t measured-green-run PASS with_a_stub_gh "$sha" "$push" "job windows success 18"
  t measured-windows-job-failed FAIL with_a_stub_gh "$sha" "$push" "job windows failure 18"
  t measured-windows-zero-steps FAIL with_a_stub_gh "$sha" "$push" "job windows success 0"
  t measured-pull-request-run-only FAIL with_a_stub_gh "$sha" "$pull" "job windows success 18"
  t measured-sha-is-the-word-HEAD FAIL with_a_stub_gh HEAD "$push" "job windows success 18"
  t no-push-run-is-named PASS defect_is "no push run for $sha" "$sha" "$pull"
  t sha-empty-is-named PASS defect_is "the commit to read is no 40 hex digits: []" "" "$push" "job windows success 18"
  t sha-39-is-named PASS defect_is "the commit to read is no 40 hex digits: [5d71834b43d3cedf77edf36ac4dcf5d62b728ad]" 5d71834b43d3cedf77edf36ac4dcf5d62b728ad "$push" "job windows success 18"
  t windows-job-absent-is-named PASS defect_is "push run 42619829679 has no job named windows" "$sha" "$push" "job macos success 21"
}

measure() {
  sha=$(measured_sha)
  if ! predicate_sha "$sha"; then
    emit_fail "$name" "the commit to read is no 40 hex digits: [$sha]"; return 1
  fi
  if ! command -v gh > /dev/null 2>&1; then
    emit_fail "$name" "gh is not available to read the runs of $sha"; return 1
  fi
  scratch_root
  if ! ci_facts "$sha" "$gate_scratch/facts"; then
    emit_fail "$name" "gh could not list the runs and jobs of $sha"; return 1
  fi
  pulls=$(grep -c '^run [0-9]* pull_request ' "$gate_scratch/facts")
  pushes=$(grep -c '^run [0-9]* push ' "$gate_scratch/facts")
  line="commit $sha: $pushes push runs decide, $pulls pull_request runs are listed and do not"
  if defect=$(ci_defect "$sha" "$gate_scratch/facts"); then emit_pass "$name" "$line"; return 0; fi
  emit_fail "$name" "$defect; $line"; return 1
}

check_main "$@"
