#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=no-ll-hook

tree() {
  made=$gate_scratch/$1
  mkdir -p "$made/crates/halvo-capture/src/macos" "$made/crates/halvo-capture/src/windows" "$made/src-tauri/src"
  printf 'let tap = CGEventTap::new(loc, place, CGEventTapOptions::ListenOnly, mask, cb);\n' > "$made/crates/halvo-capture/src/macos/input.rs"
  printf 'if ty != SCStreamOutputType::Screen {\n    return;\n}\n' > "$made/crates/halvo-capture/src/macos/screen.rs"
  printf 'let ok = RegisterRawInputDevices(&[dev], n).is_ok();\n' > "$made/crates/halvo-capture/src/windows/input.rs"
  printf 'fn main() {}\n' > "$made/src-tauri/src/main.rs"
  if [ $# -ge 3 ]; then
    mkdir -p "$made/${2%/*}"
    printf '%s\n' "$3" > "$made/$2"
  fi
  git init -q "$made" && tracked "$1"
}

tracked() {
  git -C "$gate_scratch/$1" add -A
}

linked() {
  tree "$1"
  mkdir -p "$made/vendor"
  printf 'let h = SetWindowsHookExW(WH_KEYBOARD_LL, Some(cb), hmod, 0);\n' > "$made/vendor/hook.txt"
  ln -s ../../../../vendor/hook.txt "$made/$2"
  tracked "$1"
}

defect_is() {
  defect=$(source_scan_defect "$1")
  [ "$defect" = "$2" ]
}

fails_naming() {
  defect=$(source_scan_defect "$1")
  [ "$defect" != "$2" ]
}

measured_from() {
  ( cd "$gate_scratch/$2" && GATE_ROOT=$1 sh "$DIR/check-no-ll-hook.sh" --measure )
}

measured_from_says() {
  out=$(measured_from "$2" "$3" 2> /dev/null)
  [ "$out" = "$1" ]
}

scanned_from() {
  ( cd "$gate_scratch/$1" && predicate_no_ll_hook "$2" )
}

measured_on() {
  GATE_ROOT=$gate_scratch/$1
  measure
}

measured_line_is() {
  GATE_ROOT=$gate_scratch/$2
  out=$(measure)
  [ "$out" = "$1" ]
}

cases() {
  scratch_root
  tree listen-only
  t listen-only-tree PASS predicate_no_ll_hook "$gate_scratch/listen-only"
  tree windows-hook crates/halvo-capture/src/windows/hook.rs 'let h = SetWindowsHookExW(WH_KEYBOARD_LL, Some(cb), hmod, 0);'
  t windows-hook FAIL predicate_no_ll_hook "$gate_scratch/windows-hook"
  tree keyboard-hook crates/halvo-capture/src/windows/hook.rs 'let id = WH_KEYBOARD_LL;'
  t keyboard-hook-constant FAIL predicate_no_ll_hook "$gate_scratch/keyboard-hook"
  tree mouse-hook crates/halvo-capture/src/windows/hook.rs 'let id = WH_MOUSE_LL;'
  t mouse-hook-constant FAIL predicate_no_ll_hook "$gate_scratch/mouse-hook"
  tree hook-in-app src-tauri/src/hook.rs 'let h = SetWindowsHookExW(id, Some(cb), hmod, 0);'
  t hook-in-src-tauri FAIL predicate_no_ll_hook "$gate_scratch/hook-in-app"
  tree tap-default crates/halvo-capture/src/macos/tap.rs 'let tap = CGEventTap::new(loc, place, CGEventTapOptions::Default, mask, cb);'
  t active-tap-default FAIL predicate_no_ll_hook "$gate_scratch/tap-default"
  t rule-is-named PASS defect_is "$gate_scratch/tap-default" "active-tap at crates/halvo-capture/src/macos/tap.rs:1"
  tree tap-beside crates/halvo-capture/src/macos/tap.rs 'let both = (CGEventTapOptions::ListenOnly, CGEventTapOptions::Default);'
  t listen-only-beside-default FAIL predicate_no_ll_hook "$gate_scratch/tap-beside"
  tree tap-constant crates/halvo-capture/src/macos/tap.rs 'let options = kCGEventTapOptionDefault;'
  t active-tap-constant FAIL predicate_no_ll_hook "$gate_scratch/tap-constant"
  tree tap-raw crates/halvo-capture/src/macos/tap.rs 'let port = unsafe { CGEventTapCreate(a, b, c, d, cb, info) };'
  t raw-tap-create FAIL predicate_no_ll_hook "$gate_scratch/tap-raw"
  tree event-post crates/halvo-capture/src/macos/tap.rs 'unsafe { CGEventPost(tap, event) };'
  t event-post FAIL predicate_no_ll_hook "$gate_scratch/event-post"
  posted=crates/halvo-capture/src/macos/post.rs
  tree post-to-a-location "$posted" 'ev.post(CGEventTapLocation::HID);'
  t post-to-a-tap-location FAIL fails_naming "$gate_scratch/post-to-a-location" "active-tap at $posted:1"
  tree post-with-a-module-path "$posted" 'ev.post(core_graphics::event::CGEventTapLocation::HID);'
  t post-to-a-location-with-its-module-path FAIL fails_naming "$gate_scratch/post-with-a-module-path" "active-tap at $posted:1"
  tree post-on-the-type "$posted" 'CGEvent::post(&ev, CGEventTapLocation::HID);'
  t post-called-on-the-type FAIL fails_naming "$gate_scratch/post-on-the-type" "active-tap at $posted:1"
  tree post-to-a-pid "$posted" 'ev.post_to_pid(pid);'
  t post-to-a-pid FAIL fails_naming "$gate_scratch/post-to-a-pid" "active-tap at $posted:1"
  tree post-from-a-tap "$posted" 'ev.post_from_tap(proxy);'
  t post-from-a-tap FAIL fails_naming "$gate_scratch/post-from-a-tap" "active-tap at $posted:1"
  tree keyboard-event "$posted" 'let ev = CGEvent::new_keyboard_event(source, 0, true).unwrap();'
  t new-keyboard-event FAIL fails_naming "$gate_scratch/keyboard-event" "active-tap at $posted:1"
  tree mouse-event "$posted" 'let ev = CGEvent::new_mouse_event(source, kind, at, button).unwrap();'
  t new-mouse-event FAIL fails_naming "$gate_scratch/mouse-event" "active-tap at $posted:1"
  tree scroll-event "$posted" 'let ev = CGEvent::new_scroll_event(source, unit, 1, 3, 0, 0).unwrap();'
  t new-scroll-event FAIL fails_naming "$gate_scratch/scroll-event" "active-tap at $posted:1"
  tree audio crates/halvo-capture/src/macos/audio.rs 'config.setCapturesAudio(true);'
  t audio-captured FAIL predicate_no_ll_hook "$gate_scratch/audio"
  tree microphone crates/halvo-capture/src/macos/mic.rs 'config.setCaptureMicrophone(true);'
  t microphone-captured FAIL predicate_no_ll_hook "$gate_scratch/microphone"
  tree audio-output crates/halvo-capture/src/macos/audio.rs 'stream.addStreamOutput(out, SCStreamOutputType::Audio, queue);'
  t audio-output-type FAIL predicate_no_ll_hook "$gate_scratch/audio-output"
  tree machine-key crates/halvo-capture/src/custody/windows.rs 'let flags = CRYPTPROTECT_UI_FORBIDDEN | CRYPTPROTECT_LOCAL_MACHINE;'
  t local-machine-key FAIL predicate_no_ll_hook "$gate_scratch/machine-key"
  tree comment crates/halvo-capture/src/windows/note.rs '    // SetWindowsHookExW and CGEventPost are never called here'
  t comment-only-match PASS predicate_no_ll_hook "$gate_scratch/comment"
  tree build-script src-tauri/build.rs 'fn main() { let id = WH_KEYBOARD_LL; }'
  t hook-in-a-build-script FAIL predicate_no_ll_hook "$gate_scratch/build-script"
  t build-script-is-named PASS defect_is "$gate_scratch/build-script" "windows-hook at src-tauri/build.rs:1"
  tree other-dir src-tauri/tests/hook.rs 'let h = SetWindowsHookExW(WH_MOUSE_LL, Some(cb), hmod, 0);'
  t hook-outside-crates-and-src-tauri-src FAIL predicate_no_ll_hook "$gate_scratch/other-dir"
  linked link crates/halvo-capture/src/windows/hook.rs
  t hook-behind-a-tracked-link FAIL predicate_no_ll_hook "$gate_scratch/link"
  t tracked-link-is-named PASS defect_is "$gate_scratch/link" "tracked symbolic link: crates/halvo-capture/src/windows/hook.rs"
  tree no-app; mv "$gate_scratch/no-app/src-tauri" "$gate_scratch/no-app/moved"; tracked no-app
  t dir-missing FAIL predicate_no_ll_hook "$gate_scratch/no-app"
  t repo-missing FAIL predicate_no_ll_hook "$gate_scratch/no-such-repo"
  t root-missing-is-not-the-cwd FAIL measured_from "$gate_scratch/no-such-root" listen-only
  t root-missing-is-named PASS measured_from_says "FAIL no-ll-hook (source scan did not run: $gate_scratch/no-such-root is missing)" "$gate_scratch/no-such-root" listen-only
  t empty-root-is-not-the-cwd FAIL scanned_from listen-only ""
  mkdir -p "$gate_scratch/no-rs/crates/halvo-capture/src/macos" "$gate_scratch/no-rs/src-tauri/src"
  printf 'CGEventTapOptions::ListenOnly\n' > "$gate_scratch/no-rs/crates/halvo-capture/src/macos/notes.txt"
  git init -q "$gate_scratch/no-rs" && tracked no-rs
  t no-rs-file FAIL predicate_no_ll_hook "$gate_scratch/no-rs"
  t no-rs-file-is-named PASS defect_is "$gate_scratch/no-rs" "not scanned: no .rs file"
  tree no-control crates/halvo-capture/src/macos/input.rs 'let tap = make_tap();'
  t listen-only-absent FAIL predicate_no_ll_hook "$gate_scratch/no-control"
  tree trailing-comment crates/halvo-capture/src/windows/hook.rs 'let h = SetWindowsHookExW(id, Some(cb), hmod, 0); // a note after the call'
  t hook-before-a-trailing-comment FAIL predicate_no_ll_hook "$gate_scratch/trailing-comment"
  tree after-a-block-comment crates/halvo-capture/src/macos/post.rs '/* a block comment
// that ends here */ CGEventPost(tap, event);'
  t call-after-a-block-comment-on-a-slash-line FAIL predicate_no_ll_hook "$gate_scratch/after-a-block-comment"
  tree inside-a-block-comment crates/halvo-capture/src/macos/note.rs '/* CGEventPost(tap, event) is never called
// and SetWindowsHookExW neither */'
  t calls-inside-a-block-comment PASS predicate_no_ll_hook "$gate_scratch/inside-a-block-comment"
  tree comment-control crates/halvo-capture/src/macos/input.rs '// CGEventTapOptions::ListenOnly is what the tap uses
let tap = make_tap();'
  t listen-only-only-in-a-comment FAIL predicate_no_ll_hook "$gate_scratch/comment-control"
  tree post-variable "$posted" 'let place = CGEventTapLocation::HID; event.post(place);'
  t post-through-a-variable FAIL fails_naming "$gate_scratch/post-variable" "untied-post at $posted:1"
  tree post-space "$posted" 'ev.post (CGEventTapLocation::HID);'
  t post-behind-a-space FAIL fails_naming "$gate_scratch/post-space" "untied-post at $posted:1"
  tree post-second "$posted" 'ev.post(pick(), CGEventTapLocation::HID);'
  t post-after-another-argument FAIL fails_naming "$gate_scratch/post-second" "untied-post at $posted:1"
  tree quoted-slashes crates/halvo-capture/src/windows/hook.rs 'let url = "https://example.invalid"; let h = SetWindowsHookExW(id, Some(cb), hmod, 0);'
  t hook-after-a-string-with-slashes FAIL predicate_no_ll_hook "$gate_scratch/quoted-slashes"
  t measured-listen-only-tree PASS measured_on listen-only
  t measured-active-tap FAIL measured_on tap-default
  t measured-windows-hook FAIL measured_on windows-hook
  t measured-tree-not-scanned FAIL measured_on no-rs
  t measured-line-names-the-rule PASS measured_line_is "FAIL no-ll-hook (source scan measured: active-tap at crates/halvo-capture/src/macos/tap.rs:1)" tap-default
  tree untracked
  printf 'let id = WH_KEYBOARD_LL;\n' > "$gate_scratch/untracked/crates/halvo-capture/src/windows/hook.rs"
  t untracked-file-is-not-read PASS predicate_no_ll_hook "$gate_scratch/untracked"
  tree staged crates/halvo-capture/src/windows/hook.rs 'let id = WH_KEYBOARD_LL;'
  printf 'let id = 1;\n' > "$gate_scratch/staged/crates/halvo-capture/src/windows/hook.rs"
  t index-is-read-not-the-work-tree FAIL predicate_no_ll_hook "$gate_scratch/staged"
  tree broken-index
  printf 'not an index\n' > "$gate_scratch/broken-index/.git/index"
  t index-unreadable FAIL predicate_no_ll_hook "$gate_scratch/broken-index"
  t directory-inside-another-repository FAIL predicate_no_ll_hook "$gate_scratch/listen-only/crates"
}

measure() {
  if defect=$(source_scan_defect "$GATE_ROOT"); then emit_pass "$name"; return 0; fi
  case $defect in
    'not scanned: '*) emit_fail "$name" "source scan did not run: ${defect#not scanned: }" ;;
    *) emit_fail "$name" "source scan measured: $defect" ;;
  esac
  return 1
}

check_main "$@"
