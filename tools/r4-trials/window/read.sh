#!/bin/sh
# shellcheck disable=SC2016
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r4-trials/window/read.sh [--root DIR]

Prints the reading lines of the window trial and of the state-sync trial
(contracts I11.1, I11.3): what the builder read in the sources and did not
measure. It starts no app. Each line is printed only when its source says it;
otherwise a line "read ... found=no" names it.

The sources: the trial app (tools/r4-trials/app/src/app.rs and src/call.js),
the samplers (tools/r4-trials/window/sample.sh, footprint.swift and
sample.ps1), and wry 0.57.1, the version that tools/r4-trials/app/Cargo.lock
pins, in the cargo registry. --root takes another copy of
tools/r4-trials. Exit: 0 when every line was printed, 1 when one was not,
2 on a refusal.
EOF
}

here=$(cd "$(dirname "$0")" && pwd) || exit 1
root="$here/.."
case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  --root)
    [ "$#" -eq 2 ] && [ -d "$2" ] || {
      printf 'REFUSED usage: --root takes a directory\n' >&2
      exit 2
    }
    root=$2
    ;;
  '') ;;
  *)
    printf 'REFUSED usage: read.sh takes --root DIR or nothing\n' >&2
    exit 2
    ;;
esac

missed=0

says() {
  [ -f "$1" ] || return 1
  flat=$(tr -s '[:space:]' ' ' <"$1") || return 1
  case "$flat" in
    *"$2"*) return 0 ;;
    *) return 1 ;;
  esac
}

reading() {
  if says "$3" "$4"; then
    printf '%s\n' "$1"
  else
    printf '%s\n' "$2"
    missed=1
  fi
}

app="$root/app/src/app.rs"
call="$root/app/src/call.js"
mac="$root/window/sample.sh"
fp="$root/window/footprint.swift"
win="$root/window/sample.ps1"

wry=
if says "$root/app/Cargo.lock" 'name = "wry" version = "0.57.1"'; then
  for dir in "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/wry-0.57.1; do
    if [ -f "$dir/src/wkwebview/mod.rs" ]; then
      wry="$dir/src/wkwebview/mod.rs"
    fi
  done
fi

w='reading trial=window machine=mac'
reading "$w arm=a2 system=macos call=background_throttling value=suspend" \
  'read trial=window call=background_throttling found=no' "$app" \
  'let builder = if self.options.arm == Arm::A2 { builder.background_throttling(tauri::utils::config::BackgroundThrottlingPolicy::Suspend) }'
reading "$w arm=a2 system=windows call=set_is_visible value=false" \
  'read trial=window call=set_is_visible found=no' "$app" \
  'quiet_line("set_is_visible", controller.SetIsVisible(!quiet).is_ok());'
reading "$w arm=a2 system=windows call=set_memory_usage_target_level value=low" \
  'read trial=window call=set_memory_usage_target_level found=no' "$app" \
  'let level = if quiet { COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW } else {'
reading "$w arm=a2 system=macos expect=same_as_a source=wry-0.57.1 policy_set=only_when_named" \
  'read trial=window source=wry-0.57.1 found=no' "$wry" \
  'if let Some(policy) = attributes.background_throttling {'
reading "$w figure=key_bytes system=macos command=ps" \
  'read trial=window figure=key_bytes system=macos found=no' "$mac" 'rows=$(ps -o pid=,rss=,time= -p "$pids")'
reading "$w figure=second_bytes system=macos command=proc_pid_rusage" \
  'read trial=window figure=second_bytes system=macos found=no' "$fp" 'if rc == 0 { print("\(pid) \(info.ri_phys_footprint)") }'
reading "$w figure=cpu_ms system=macos command=ps" \
  'read trial=window figure=cpu_ms system=macos found=no' "$mac" 'rows=$(ps -o pid=,rss=,time= -p "$pids")'
reading "$w figure=key_bytes system=windows command=Get-Process" \
  'read trial=window figure=key_bytes system=windows found=no' "$win" 'Working = [int64]$p.WorkingSet64'
reading "$w figure=second_bytes system=windows command=Get-Process" \
  'read trial=window figure=second_bytes system=windows found=no' "$win" 'Private = [int64]$p.PrivateMemorySize64'
reading "$w figure=cpu_ms system=windows command=Get-Process" \
  'read trial=window figure=cpu_ms system=windows found=no' "$win" 'Cpu = [int64][Math]::Round($p.TotalProcessorTime.TotalMilliseconds)'
reading 'reading trial=sync machine=mac object=__TAURI_INTERNALS__' \
  'read trial=sync object=__TAURI_INTERNALS__ found=no' "$call" 'var internals = window.__TAURI_INTERNALS__;'
reading 'reading trial=sync machine=mac way=channel order_by=index' \
  'read trial=sync way=channel found=no' "$call" 'pending.set(raw.index, raw.message); while (pending.has(next)) {'
exit "$missed"
