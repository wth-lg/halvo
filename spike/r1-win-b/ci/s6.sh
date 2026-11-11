#!/bin/sh
# S6, Windows hooks: build the per-user NSIS bundle with a generated include and each hook
# variant, install it silently, run uninstall.exe /S with the helper running, read the marks.
set -u
root=$(pwd)
gen=spike/r1-win-b/nsis/gen
mkdir -p "$gen" target/helpers
printf '!define R1SPIKE_HELPER_EXE "halvo-capture.exe"\r\n!define R1SPIKE_GENERATED_AT_BUILD 1\r\n' >"$gen/helpers.nsh"
cp spike/r1-win-b/target/release/r1probe.exe target/helpers/halvo-capture-x86_64-pc-windows-msvc.exe
inst="$LOCALAPPDATA/Halvo"

for variant in direct plugin; do
  cfg='{"bundle":{"targets":["nsis"],"externalBin":["../target/helpers/halvo-capture"],"windows":{"webviewInstallMode":{"type":"skip"},"nsis":{"installMode":"currentUser","installerHooks":"../spike/r1-win-b/nsis/hooks-'"$variant"'.nsh"}}}}'
  log="$RUNNER_TEMP/build-$variant.log"
  rm_dir="target/release/bundle/nsis"
  [ -d "$rm_dir" ] && mv "$rm_dir" "$RUNNER_TEMP/nsis-before-$variant"
  npm run tauri build -- --bundles nsis --config "$cfg" >"$log" 2>&1
  rc=$?
  echo "S6 build variant=$variant exit=$rc makensis_error_lines=$(grep -c -i 'error' "$log") already_declared=$(grep -c -i 'already declared\|already defined' "$log") invalid_command=$(grep -c -i 'Invalid command' "$log") include_not_found=$(grep -c -i 'could not find\|can.t open' "$log")"
  setup=$(ls target/release/bundle/nsis/*-setup.exe 2>/dev/null | head -n 1)
  [ -n "$setup" ] || { echo "S6 variant=$variant setup_built=false"; continue; }
  echo "S6 variant=$variant setup_built=true setup_bytes=$(wc -c <"$setup")"
  cp "$setup" "$RUNNER_TEMP/setup-$variant.exe"
  for scenario in ctl rm; do
    d="$RUNNER_TEMP/s6-$variant-$scenario"
    mkdir -p "$d"
    dw=$(cygpath -w "$d")
    R1SPIKE_S6_DIR="$dw" "$RUNNER_TEMP/setup-$variant.exe" /S
    echo "S6 install variant=$variant scenario=$scenario exit=$? app=$(test -f "$inst/halvo.exe" && echo true || echo false) helper=$(test -f "$inst/halvo-capture.exe" && echo true || echo false) uninstaller=$(test -f "$inst/uninstall.exe" && echo true || echo false)"
    flag=""
    [ "$scenario" = rm ] && flag="--ignore-stop"
    "$inst/halvo-capture.exe" s6-hold --dir "$dw" $flag &
    sleep 3
    before=$(tasklist //FI "IMAGENAME eq halvo-capture.exe" //NH | grep -c -i 'halvo-capture.exe')
    t0=$(date +%s%N)
    R1SPIKE_S6_DIR="$dw" "$inst/uninstall.exe" /S
    urc=$?
    t1=$(date +%s%N)
    n=0
    while ! grep -q 'hook=postuninstall' "$d/hook.txt" 2>/dev/null && [ $n -lt 120 ]; do sleep 0.5; n=$((n + 1)); done
    t2=$(date +%s%N)
    sleep 2
    after=$(tasklist //FI "IMAGENAME eq halvo-capture.exe" //NH | grep -c -i 'halvo-capture.exe')
    echo "S6 uninstall variant=$variant scenario=$scenario exit=$urc returned_ms=$(((t1 - t0) / 1000000)) postuninstall_after_ms=$(((t2 - t0) / 1000000)) helper_processes_before=$before helper_processes_after=$after helper_file_left=$(test -e "$inst/halvo-capture.exe" && echo true || echo false) app_file_left=$(test -e "$inst/halvo.exe" && echo true || echo false) dir_left=$(test -d "$inst" && echo true || echo false)"
    sed -e "s/^/S6 hookline variant=$variant scenario=$scenario /" "$d/hook.txt" 2>/dev/null | tr -d '\r'
    sed -e 's/ t_us=[0-9]*//' -e "s/^/S6 holderline variant=$variant scenario=$scenario /" "$d/holder.txt" 2>/dev/null | tr -d '\r'
    if [ "$after" -gt 0 ]; then taskkill //F //IM halvo-capture.exe >/dev/null 2>&1; echo "S6 holder_killed_by_job=true"; fi
    wait 2>/dev/null
  done
done
cd "$root" || exit 1
