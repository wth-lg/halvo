#!/bin/sh
# S8, Windows half: the address-field read in Edge, Chrome and Firefox, then the public-suffix
# question: licence of what psl adds and what it adds to the release binary and its memory.
set -u
root=$(pwd)
probe="$root/spike/r1-win-b/target/release/r1probe.exe"
d=$(cygpath -w "$RUNNER_TEMP/s8")
mkdir -p "$RUNNER_TEMP/s8"
part=${1:-all}

if [ "$part" = all ] || [ "$part" = browsers ]; then
  "$probe" site --browser edge --exe 'C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe' --dir "$d" --secs 60 --reads 20
  "$probe" site --browser chrome --exe 'C:\Program Files\Google\Chrome\Application\chrome.exe' --dir "$d" --secs 60 --reads 20
  "$probe" site --browser firefox --exe 'C:\Program Files\Mozilla Firefox\firefox.exe' --dir "$d" --secs 60 --reads 20
fi

if [ "$part" = all ] || [ "$part" = psl ]; then
  wt="$RUNNER_TEMP/psl-wt"
  git worktree add --detach "$wt" HEAD >/dev/null 2>&1
  cd "$wt" || exit 1
  bin=crates/halvo-capture/src/bin/halvo-capture.rs
  node "$root/spike/r1-win-b/psl/patch.mjs" stub "$bin"
  cargo build --release --locked -p halvo-capture --bin halvo-capture >"$RUNNER_TEMP/psl-base.log" 2>&1
  echo "S8 build variant=base exit=$? bytes=$(wc -c <target/release/halvo-capture.exe)"
  cp target/release/halvo-capture.exe "$RUNNER_TEMP/base.exe"
  cargo metadata --format-version 1 --locked >"$RUNNER_TEMP/meta-base.json" 2>/dev/null
  git checkout -- crates
  cargo add psl -p halvo-capture >"$RUNNER_TEMP/psl-add.log" 2>&1
  echo "S8 cargo_add exit=$?"
  node "$root/spike/r1-win-b/psl/patch.mjs" psl "$bin"
  cargo build --release -p halvo-capture --bin halvo-capture >"$RUNNER_TEMP/psl-psl.log" 2>&1
  echo "S8 build variant=psl exit=$? bytes=$(wc -c <target/release/halvo-capture.exe)"
  cp target/release/halvo-capture.exe "$RUNNER_TEMP/psl.exe"
  cargo metadata --format-version 1 >"$RUNNER_TEMP/meta-psl.json" 2>/dev/null
  node "$root/spike/r1-win-b/psl/licences.mjs" "$RUNNER_TEMP/meta-base.json" "$RUNNER_TEMP/meta-psl.json"
  echo "S8 psl_cut example_co_uk_len=$("$RUNNER_TEMP/psl.exe" --r1spike-site www.example.co.uk) stub_len=$("$RUNNER_TEMP/base.exe" --r1spike-site www.example.co.uk)"
  b=$(cygpath -w "$RUNNER_TEMP/base.exe")
  p=$(cygpath -w "$RUNNER_TEMP/psl.exe")
  for i in 1 2 3; do
    "$probe" peak --label "base-version-$i" --cmd "\"$b\" --version"
    "$probe" peak --label "psl-version-$i" --cmd "\"$p\" --version"
    "$probe" peak --label "base-timed10-$i" --cmd "\"$b\" --timed-run 10"
    "$probe" peak --label "psl-timed10-$i" --cmd "\"$p\" --timed-run 10"
  done
  cd "$root" || exit 1
fi
