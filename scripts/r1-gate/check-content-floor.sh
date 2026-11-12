#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=content-floor

marker_word_of() {
  sed -n 's/^floor marker=\([a-z_]*\) .*/\1/p' "$1" 2>/dev/null
}

floor_head() {
  sed -n "1s/.* $2=\\([^ ]*\\).*/\\1/p" "$1" 2>/dev/null
}

floor_shape_defect() {
  floor_first=$(sed -n '1s/^\(floor records=\).*/\1/p' "$1" 2>/dev/null)
  [ "$floor_first" = 'floor records=' ] || { printf 'the floor report holds no first line floor records=\n'; return 1; }
  for floor_key in undecodable forbidden_fields; do
    floor_n=$(floor_head "$1" "$floor_key")
    [ "$floor_n" = 0 ] || { printf 'the floor report says %s=%s\n' "$floor_key" "${floor_n:-absent}"; return 1; }
  done
  floor_kinds=$(sed -n 's/^floor kind=\([a-z_]*\) records=[0-9]*$/\1/p' "$1" | tr '\n' ' ')
  [ "$floor_kinds" = 'screen_frame keystroke pointer foreground_app window_title system_audio microphone camera ' ] ||
    { printf 'the floor report holds the kind lines [%s], and C13.9 has the eight kinds of 0.2 in their order\n' "${floor_kinds% }"; return 1; }
  floor_keys=$(sed -n 's/^floor key_class=\([a-z]*\) records=[0-9]*$/\1/p' "$1" | sort | tr '\n' ' ')
  floor_points=$(sed -n 's/^floor pointer_class=\([a-z]*\) records=[0-9]* folded=[0-9]*$/\1/p' "$1" | sort | tr '\n' ' ')
  [ "$floor_keys" = 'editing enter navigation other printable shortcut tab ' ] && [ "$floor_points" = 'click drag move scroll ' ] && return 0
  printf 'the floor report holds the key classes [%s] and the pointer classes [%s], and C13.9 has one line for each of the seven and the four\n' "${floor_keys% }" "${floor_points% }"
  return 1
}

title_marker_defect() {
  floor_shape_defect "$1" || return 1
  marker_found=$(floor_value "$1" marker=found app)
  marker_word=$(marker_word_of "$1")
  [ -n "$marker_found" ] || { printf 'floor marker=%s for the planted marker, so the reader saw no text\n' "$marker_word"; return 1; }
  marker_title=$(floor_value "$1" marker=found title)
  marker_column=$(floor_value "$1" marker=found column)
  [ "$marker_found" -gt 0 ] && [ "${marker_title:-0}" -gt 0 ] || { printf 'the planted marker is found with app=%s title=%s, and both must be above 0\n' "$marker_found" "${marker_title:-absent}"; return 1; }
  [ "$marker_column" = 0 ] && return 0
  printf 'the marker is in a database column: column=%s\n' "${marker_column:-absent}"
  return 1
}

predicate_title_marker_found() {
  title_marker_defect "$1" > /dev/null
}

typed_marker_defect() {
  floor_shape_defect "$1" || return 1
  marker_line=$(grep -m 1 '^floor marker=' "$1")
  [ "$marker_line" = 'floor marker=absent app=0 title=0 site=0 column=0' ] && return 0
  printf 'the typed marker is in the store: %s\n' "${marker_line:-no marker line}"
  return 1
}

predicate_typed_marker_absent() {
  typed_marker_defect "$1" > /dev/null
}

raw_scan_defect() {
  raw_found=$(tree_scan_defect "the data directory" "$2" "$1") && return 0
  printf 'the raw scan, outside the encrypted payloads: %s\n' "$raw_found"
  return 1
}

predicate_raw_scan_clean() {
  raw_scan_defect "$@" > /dev/null
}

excluded_defect() {
  floor_shape_defect "$1" || return 1
  excluded_line=$(grep -m 1 '^floor marker=' "$1")
  [ "$excluded_line" = 'floor marker=absent app=0 title=0 site=0 column=0' ] && return 0
  printf 'a record carries the app id of the statically excluded app\n'
  return 1
}

predicate_no_excluded_record() {
  excluded_defect "$1" > /dev/null
}

floor_steps() {
  build_for_measure || return 1
  service_prepare || return 1
  privacy_no_capture_line
  title_marker=$(gate_marker)
  prefill_run prefill 100 --marker "$title_marker" --span-days 1 || { gate_defect="$gate_defect; not tried: --floor-report"; return 1; }
  floor_run floor --marker "$title_marker" || return 1
  found=$(title_marker_defect "$svc_dir/floor.out") || { gate_defect=$found; return 1; }
  found=$(raw_scan_defect "$svc_data" "$title_marker") || { gate_defect=$found; return 1; }
  floor_app=$(floor_value "$svc_dir/floor.out" marker=found app); floor_title=$(floor_value "$svc_dir/floor.out" marker=found title)
  floor_result="floor marker=found app=$floor_app title=$floor_title column=0, undecodable=0 forbidden_fields=0, 7 key_class and 4 pointer_class lines, raw scan clean"
  if [ -n "${GATE_EXCLUDED_APP:-}" ]; then
    floor_run excluded --marker "$GATE_EXCLUDED_APP" || return 1
    found=$(excluded_defect "$svc_dir/excluded.out") || { gate_defect=$found; return 1; }
    floor_result="$floor_result, no record of the excluded app"
  fi
}

measure() {
  floor_result=''
  if floor_steps; then ok=yes; else ok=no; fi
  stop_children
  where="binary=${capture_bin#"$GATE_ROOT"/}"
  [ "$ok" = yes ] || { emit_fail "$name" "$gate_defect; $where"; return 1; }
  if [ "$gate_input" = none ]; then
    emit_no_input "$name" "input=none, so the typed half did not run: $floor_result; $where"
    return 3
  fi
  privacy_rows_line
  emit_fail "$name" "the typed half is not built: it needs a text window whose title carries a second marker and the session log of what was sent; $floor_result; $where"
  return 1
}

against_the_standin() {
  service_standin "$1" "$2"
  measure
}

says() {
  says_text=$1; shift
  service_standin "$1" "$2"
  says_out=$(measure 2>/dev/null)
  case $says_out in *"$says_text"*) return 0 ;; esac
  return 1
}

floor_fixture() {
  {
    printf 'floor records=100 undecodable=%s forbidden_fields=%s\n' "$2" "$3"
    for kind in screen_frame keystroke pointer foreground_app window_title system_audio microphone camera; do printf 'floor kind=%s records=20\n' "$kind"; done
    for class in printable editing navigation enter tab shortcut other; do
      [ "$class" = "$4" ] || printf 'floor key_class=%s records=3\n' "$class"
    done
    for class in click scroll drag move; do printf 'floor pointer_class=%s records=5 folded=0\n' "$class"; done
    printf 'floor titles=20 max_title_bytes=40 sites=0\n%s\n' "$5"
  } > "$gate_scratch/$1"
  printf '%s\n' "$gate_scratch/$1"
}

no_capture_line_first() {
  service_standin "$1" "$2"
  first_out=$(measure 2>/dev/null)
  first_line=$(printf '%s\n' "$first_out" | sed -n 1p)
  [ "$first_line" = "note privacy rows: the unattended half of this check starts no capture source and adds no row" ]
}

cases() {
  scratch_root
  found_line='floor marker=found app=100 title=20 site=0 column=0'
  absent_line='floor marker=absent app=0 title=0 site=0 column=0'
  good=$(floor_fixture good 0 0 none "$found_line")
  t title-marker-found PASS predicate_title_marker_found "$good"
  fixture=$(floor_fixture title-absent 0 0 none "$absent_line")
  t title-marker-absent FAIL predicate_title_marker_found "$fixture"
  fixture=$(floor_fixture forbidden 0 3 none "$found_line")
  t forbidden-fields-3 FAIL predicate_title_marker_found "$fixture"
  fixture=$(floor_fixture undecodable 1 0 none "$found_line")
  t undecodable-1 FAIL predicate_title_marker_found "$fixture"
  fixture=$(floor_fixture class-missing 0 0 tab "$found_line")
  t class-line-missing FAIL predicate_title_marker_found "$fixture"
  fixture=$(floor_fixture column 0 0 none 'floor marker=found app=100 title=20 site=0 column=1')
  t marker-in-column FAIL predicate_title_marker_found "$fixture"
  fixture=$(floor_fixture no-title 0 0 none 'floor marker=found app=100 title=0 site=0 column=0')
  t title-marker-without-a-title FAIL predicate_title_marker_found "$fixture"
  fixture=$(floor_fixture typed-absent 0 0 none "$absent_line")
  t typed-marker-absent PASS predicate_typed_marker_absent "$fixture"
  fixture=$(floor_fixture typed-found 0 0 none 'floor marker=found app=0 title=1 site=0 column=0')
  t typed-marker-found FAIL predicate_typed_marker_absent "$fixture"
  fixture=$(floor_fixture typed-absent-with-a-title 0 0 none 'floor marker=absent app=0 title=1 site=0 column=0')
  t typed-marker-absent-word-with-a-title-count FAIL predicate_typed_marker_absent "$fixture"
  fixture=$(floor_fixture class-twice 0 0 tab "$found_line")
  printf 'floor key_class=printable records=3\n' >> "$fixture"
  t class-line-twice-and-one-missing FAIL predicate_title_marker_found "$fixture"
  mkdir -p "$gate_scratch/raw-clean/store" "$gate_scratch/raw-hit/store"
  head -c 256 /dev/urandom > "$gate_scratch/raw-clean/store/store.db"
  printf 'title r1gateTITLE-9 in plain text\n' > "$gate_scratch/raw-hit/store/store.db-wal"
  t title-marker-not-in-raw-scan PASS predicate_raw_scan_clean "$gate_scratch/raw-clean" r1gateTITLE-9
  t title-marker-in-raw-scan FAIL predicate_raw_scan_clean "$gate_scratch/raw-hit" r1gateTITLE-9
  mkdir -p "$gate_scratch/raw-name/store" "$gate_scratch/raw-utf16/store" "$gate_scratch/raw-link/store"
  : > "$gate_scratch/raw-name/store/r1gateTITLE-9.txt"
  perl -e 'print join("", map { $_ . "\0" } split //, "r1gateTITLE-9")' > "$gate_scratch/raw-utf16/store/note"
  printf 'title r1gateTITLE-9 in plain text\n' > "$gate_scratch/raw-plain"
  ln -s "$gate_scratch/raw-plain" "$gate_scratch/raw-link/store/link"
  t title-marker-in-a-name FAIL predicate_raw_scan_clean "$gate_scratch/raw-name" r1gateTITLE-9
  t title-marker-in-utf-16 FAIL predicate_raw_scan_clean "$gate_scratch/raw-utf16" r1gateTITLE-9
  t symbolic-link-in-the-raw-scan FAIL predicate_raw_scan_clean "$gate_scratch/raw-link" r1gateTITLE-9
  fixture=$(floor_fixture ninth-kind 0 0 none "$found_line")
  awk '/^floor kind=camera / { print; print "floor kind=run_mark records=1"; next } { print }' "$fixture" > "$fixture.nine" && mv "$fixture.nine" "$fixture"
  t floor-ninth-kind FAIL predicate_title_marker_found "$fixture"
  fixture=$(floor_fixture kinds-out-of-order 0 0 none "$found_line")
  awk '/^floor kind=screen_frame / { held = $0; next } /^floor kind=keystroke / { print; print held; next } { print }' "$fixture" > "$fixture.order" && mv "$fixture.order" "$fixture"
  t floor-kinds-out-of-order FAIL predicate_title_marker_found "$fixture"
  fixture=$(floor_fixture excluded-absent 0 0 none "$absent_line")
  t no-excluded-app-record PASS predicate_no_excluded_record "$fixture"
  fixture=$(floor_fixture excluded-found 0 0 none 'floor marker=found app=3 title=0 site=0 column=0')
  t excluded-app-record FAIL predicate_no_excluded_record "$fixture"
  t standin-unattended-half NO-INPUT against_the_standin good none
  t standin-plaintext-title-in-the-store FAIL against_the_standin raw-leak none
  t standin-main-names-the-mode PASS says 'FAIL content-floor (no --prefill-store mode' main none
  t unattended-half-says-it-starts-no-capture-source PASS no_capture_line_first main none
}

check_main "$@"
