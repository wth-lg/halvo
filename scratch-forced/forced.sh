#!/bin/sh
# Scratch only, never merged: the forced forms of the unit check-page-line on a hosted windows job.
# Each form changes the checkout, commits it on this runner alone, runs check 32-app-check-page through the
# driver, prints the check's lines, compares them with the form's expected text, and goes back to the head.
# With --apply <change...> it applies the named changes to the checkout and stops.
set +e
dir=scratch-forced

hold() {
  [ "$(grep -c '^        \.setup(move |app| {' src-tauri/src/app.rs)" = 1 ] || return 1
  perl -pi -e 's/^(        \.setup\(move \|app\| \{)(\r?)$/$1$2\n            std::thread::sleep(std::time::Duration::from_secs(65));$2/' src-tauri/src/app.rs
  grep -q '^            std::thread::sleep(std::time::Duration::from_secs(65));' src-tauri/src/app.rs
}
at_base() {
  cp "$dir/base-check_page.rs" src-tauri/src/check_page.rs && cp "$dir/base-32-app-check-page.sh" scripts/r1/ci.d/32-app-check-page.sh
}
no_ready_print() {
  perl -0pi -e 's/\r?\n[ \t]*eprintln!\(\r?\n[ \t]*"halvo: --check-page: the event loop was ready at \{\} ms",\r?\n[ \t]*ready\.as_millis\(\)\r?\n[ \t]*\);//' src-tauri/src/check_page.rs
  ! grep -q 'the event loop was ready at' src-tauri/src/check_page.rs
}
no_ready_send() {
  [ "$(grep -c 'let _ = ready_tx.send(ready);' src-tauri/src/check_page.rs)" = 1 ] || return 1
  perl -ni -e 'print unless /\Qlet _ = ready_tx.send(ready);\E/' src-tauri/src/check_page.rs
  ! grep -q 'ready_tx.send(ready)' src-tauri/src/check_page.rs
}
never_answers() {
  perl -pi -e 's/\Qif let Some(line) = line(&seen, &version, guard.as_deref()) {\E/if let Some(line) = line(&seen, &version, guard.as_deref()).filter(|_| false) {/' src-tauri/src/check_page.rs
  grep -q 'filter(|_| false)' src-tauri/src/check_page.rs
}
old_order() {
  [ "$(grep -c '^    ready.map(|ready| ready + within)' src-tauri/src/check_page.rs)" = 1 ] || return 1
  perl -pi -e 's/^    ready\.map\(\|ready\| ready \+ within\)(\r?)$/    let _ = ready;$1\n    Some(within)$1/' src-tauri/src/check_page.rs
  grep -q '^    let _ = ready;' src-tauri/src/check_page.rs
}

if [ "${1:-}" = --apply ]; then
  shift
  for change in "$@"; do
    "$change" || { echo "change $change not applied"; exit 1; }
  done
  exit 0
fi

orig=$(git rev-parse HEAD)
branch=$(git symbolic-ref -q --short HEAD)
bad=0

form() {
  name=$1
  want=$2
  want2=$3
  shift 3
  git checkout -q "$orig"
  applied=yes
  for change in "$@"; do
    "$change" || applied="no ($change)"
  done
  if git diff --quiet HEAD --; then
    [ "$applied" = yes ] && applied="no (no change)"
  else
    git commit -q -a -m "scratch forced form $name" || { [ "$applied" = yes ] && applied="no (commit)"; }
  fi
  started=$(date +%s)
  sh scripts/ci/run-ci-d.sh windows 32-app-check-page >"$RUNNER_TEMP/form-$name.log" 2>&1
  code=$?
  ended=$(date +%s)
  echo "form $name: changes applied: $applied; driver exit $code; $((ended - started)) s by the clock"
  grep -E '^(check-page (seconds|clock|ready|exit|err lines)|check-page err: halvo|PASS 32-app|FAIL 32-app|seconds 32-app)' "$RUNNER_TEMP/form-$name.log"
  log=$(cat "$RUNNER_TEMP/form-$name.log")
  case $log in
    *"$want"*) case $log in *"$want2"*) verdict=as-expected ;; *) verdict=NOT-as-expected ;; esac ;;
    *) verdict=NOT-as-expected ;;
  esac
  [ "$applied" = yes ] || verdict=NOT-as-expected
  echo "form $name: $verdict"
  [ "$verdict" = as-expected ] || bad=$((bad + 1))
}

form base-held-before-ready 'check-page exit: 1, lines: 0, line matched: false, first line: (none)' 'FAIL 32-app-check-page (halvo --check-page exited 1)' at_base hold
form head-held-before-ready 'PASS 32-app-check-page' 'check-page ready ms: 6' hold
form change-ready-print-out 'FAIL 32-app-check-page (halvo --check-page printed no time of the Ready of its event loop)' 'check-page ready ms: none' no_ready_print
form control-page-never-answers 'FAIL 32-app-check-page (halvo --check-page exited 1)' 'within 60 s after the event loop was ready' never_answers
form change-ready-send-out 'FAIL 32-app-check-page (halvo --check-page ran past 90 s and was ended, its event loop ready at' 'check-page ready ms: ' never_answers no_ready_send
form change-old-order-held 'FAIL 32-app-check-page (halvo --check-page exited 1)' 'check-page ready ms: none' old_order hold

git checkout -q "$orig"
[ -n "$branch" ] && git checkout -q -B "$branch" "$orig"
echo "forced forms: $bad not as expected; head $(git rev-parse HEAD), status lines $(git status --porcelain | wc -l)"
[ "$bad" -eq 0 ]
