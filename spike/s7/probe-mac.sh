#!/bin/sh
# S7 probe for the hosted macos-15 runner (run 20261108-halvo-r1-capture, scratch branch only).
# The log is public: every line is a count, a size, a time in ms or a boolean.
# Part 1: which key events reach a listen-only tap while another process holds secure input,
#         with events that this job generates; the HID counters; the listen preflight.
# Part 2: the product's own timed run (debug build) with posted keys, without and with its hold.
# Part 3: the hidden app (spike patch applied in the job, never committed): does the page-load
#         event arrive, does the tray item install, how many windows does the process own.
set -u
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=$(mktemp -d)
cd "$root" || exit 1
echo "S7 probe: macOS $(sw_vers -productVersion) build $(sw_vers -buildVersion) arch $(uname -m)"
cc -O2 -o "$work/si-probe" spike/s7/si-probe.c -framework ApplicationServices -framework Carbon || exit 1
cc -O2 -o "$work/wins-pid" spike/s7/wins-pid.c -framework CoreGraphics -framework CoreFoundation || exit 1
P="$work/si-probe"

echo "== part 1: generated keys and a listen-only tap"
"$P" state
phase() {
  name=$1; hold=$2
  echo "-- phase $name (secure-input hold by another process: $hold)"
  "$P" counter | sed "s/^/$name before /"
  hp=''
  if [ "$hold" = yes ]; then "$P" hold 9 > "$work/hold-$name.txt" & hp=$!; sleep 1; fi
  "$P" tap 6 > "$work/tap-$name.txt" &
  tp=$!
  sleep 1.5
  "$P" state | sed "s/^/$name during /"
  "$P" post 5 | sed "s/^/$name /"
  wait "$tp"; echo "$name tap_exit=$?"
  sed "s/^/$name /" "$work/tap-$name.txt"
  "$P" counter | sed "s/^/$name after /"
  if [ -n "$hp" ]; then wait "$hp"; echo "$name hold_exit=$?"; sed "s/^/$name /" "$work/hold-$name.txt"; fi
  "$P" state | sed "s/^/$name end /"
}
phase A-before no
phase B-held yes
phase C-released no

echo "== part 2: the product's timed run with posted keys"
export CARGO_BUILD_JOBS=4
cargo build --locked -q -p halvo-capture --bin halvo-capture || echo "product_build_failed=1"
B=target/debug/halvo-capture
if [ -x "$B" ]; then
  for mode in plain held; do
    if [ "$mode" = held ]; then HALVO_GATE_SECURE_INPUT=1 "$B" --timed-run 8 > "$work/p-$mode.out" 2> "$work/p-$mode.err" & else "$B" --timed-run 8 > "$work/p-$mode.out" 2> "$work/p-$mode.err" & fi
    bp=$!
    sleep 3
    "$P" state | sed "s/^/product-$mode during /"
    "$P" post 5 | sed "s/^/product-$mode /"
    wait "$bp"; echo "product-$mode exit=$?"
    sed "s/^/product-$mode /" "$work/p-$mode.out"
    echo "product-$mode stderr_lines=$(wc -l < "$work/p-$mode.err" | tr -d ' ')"
    grep -o 'source [a-z_]* did not start' "$work/p-$mode.err" | sed "s/^/product-$mode /"
  done
fi

echo "== part 3: the hidden app (window created at launch, hidden)"
npm ci --no-audit --no-fund > "$work/npm.log" 2>&1; echo "npm_ci_exit=$?"
python3 -I spike/s7/s7-patch.py "$root" || exit 1
npm run tauri build -- --config spike/s7/s7-config.json --bundles app > "$work/build.log" 2>&1
echo "app_build_exit=$?"
git checkout -- src-tauri/src/main.rs
A=target/release/bundle/macos/Halvo.app/Contents/MacOS/halvo
if [ -x "$A" ]; then
  for run in 1 2 3; do
    "$A" > "$work/app-$run.out" 2> "$work/app-$run.err" &
    ap=$!
    sleep 12
    alive=$(kill -0 "$ap" 2>/dev/null && echo true || echo false)
    echo "app run=$run alive_at_12s=$alive"
    "$work/wins-pid" "$ap" | sed "s/^/app run=$run /"
    wk=$(pgrep -f 'com.apple.WebKit.WebContent' | wc -l | tr -d ' ')
    echo "app run=$run webcontent_processes_on_runner=$wk"
    kill -TERM "$ap" 2>/dev/null; wait "$ap" 2>/dev/null
    echo "app run=$run page_load_finished=$(grep -c '^spike page_load=finished' "$work/app-$run.err") tray_installed=$(grep -c '^spike tray=installed' "$work/app-$run.err")"
    grep -oE '^spike (page_load=finished|tray=installed) ms=[0-9]+' "$work/app-$run.err" | sed "s/^/app run=$run /"
    echo "app run=$run stderr_lines=$(wc -l < "$work/app-$run.err" | tr -d ' ') panics=$(grep -c 'panicked' "$work/app-$run.err")"
  done
else
  echo "app_binary_present=false"
  tail -5 "$work/build.log" | grep -oE 'error[^/]*' | head -5
fi
echo "S7 probe done"
