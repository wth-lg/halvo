#!/bin/sh
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
. "$DIR/lib.sh"
name=store

good="store rows=2000 secure_delete=1 temp_store=2 fts_secure_delete=1 cipher_provider=libtomcrypt segment_roundtrip=ok unwrap=ok unwrap_ms=3 prompt=none"

with() { printf '%s\n' "$good" | sed "$1"; }

marker_defect_is() {
  want=$1; shift
  defect=$(marker_defect "$@")
  [ "$defect" = "$want" ]
}

defects_are() {
  defects=$(store_report_defects "$1")
  [ "$defects" = "$2" ]
}

link_line() {
  link_found=$(marker_defect "$1" "$2" "$gate_scratch/clean/store.db") || { emit_fail "$name" "$link_found"; return 1; }
  emit_pass "$name"
}

cases() {
  scratch_root
  marker=HALVOMARKERselftest
  mkdir -p "$gate_scratch/clean" "$gate_scratch/leaky" "$gate_scratch/empty" "$gate_scratch/no-key" "$gate_scratch/no-db" "$gate_scratch/unreadable"
  printf 'wrapped-key\n' > "$gate_scratch/no-db/store.key"
  cp "$gate_scratch/no-db/store.key" "$gate_scratch/unreadable/store.key"
  printf 'bytes-that-are-ciphertext\n' > "$gate_scratch/unreadable/store.db"
  printf 'bytes that the scan cannot read\n' > "$gate_scratch/unreadable/seg0001"
  chmod 000 "$gate_scratch/unreadable/seg0001"
  for dir in clean leaky; do
    printf 'bytes-that-are-ciphertext\n' > "$gate_scratch/$dir/store.db"
    printf 'wrapped-key\n' > "$gate_scratch/$dir/store.key"
  done
  printf 'bytes-that-are-ciphertext\n' > "$gate_scratch/no-key/store.db"
  printf 'bytes-that-are-ciphertext\n' > "$gate_scratch/clean/seg0001"
  printf 'here is the %s in plaintext\n' "$marker" > "$gate_scratch/leaky/seg0001"
  printf 'store rows=2000\n' > "$gate_scratch/stdout"
  : > "$gate_scratch/stderr"
  printf 'debug: wrote %s\n' "$marker" > "$gate_scratch/stderr-with-marker"
  t clean-dir PASS predicate_marker_absent "$gate_scratch/clean" "$marker"
  t leaky-dir FAIL predicate_marker_absent "$gate_scratch/leaky" "$marker"
  t marker-dir-missing FAIL predicate_marker_absent "$gate_scratch/no-such-dir" "$marker"
  t marker-dir-empty FAIL predicate_marker_absent "$gate_scratch/empty" "$marker"
  t no-store-key FAIL predicate_marker_absent "$gate_scratch/no-key" "$marker"
  t no-store-db FAIL predicate_marker_absent "$gate_scratch/no-db" "$marker"
  mkdir -p "$gate_scratch/linked" && cp "$gate_scratch/clean/store.db" "$gate_scratch/clean/store.key" "$gate_scratch/linked/" && printf '%s\n' "$marker" > "$gate_scratch/outside-the-store" && ln -s "$gate_scratch/outside-the-store" "$gate_scratch/linked/note"
  t symbolic-link-below-the-store FAIL predicate_marker_absent "$gate_scratch/linked" "$marker"
  t symbolic-link-is-named FAIL fails_saying 'a symbolic link below the store directory' link_line "$gate_scratch/linked" "$marker"
  t unreadable-file-in-dir FAIL predicate_marker_absent "$gate_scratch/unreadable" "$marker"
  t marker-empty FAIL predicate_marker_absent "$gate_scratch/clean" ""
  t clean-capture PASS predicate_marker_not_printed "$marker" "$gate_scratch/stdout" "$gate_scratch/stderr"
  t stderr-holds-marker FAIL predicate_marker_not_printed "$marker" "$gate_scratch/stdout" "$gate_scratch/stderr-with-marker"
  t stdout-holds-marker FAIL predicate_marker_not_printed "$marker" "$gate_scratch/stderr-with-marker" "$gate_scratch/stderr"
  t capture-file-missing FAIL predicate_marker_not_printed "$marker" "$gate_scratch/stdout" "$gate_scratch/no-such-file"
  t no-capture-file-named FAIL predicate_marker_not_printed "$marker"
  t clean-run-has-no-defect PASS marker_defect "$gate_scratch/clean" "$marker" "$gate_scratch/stdout" "$gate_scratch/stderr"
  t leak-line-names-the-directory PASS marker_defect_is "leak: the marker is in the store directory" "$gate_scratch/leaky" "$marker" "$gate_scratch/stdout" "$gate_scratch/stderr"
  t leak-line-names-the-output PASS marker_defect_is "leak: the marker is in what the binary printed" "$gate_scratch/clean" "$marker" "$gate_scratch/stdout" "$gate_scratch/stderr-with-marker"
  not_run="marker scan did not run on the store directory: it needs a marker, store.db, store.key and files that grep can read"
  t unscanned-directory-is-no-leak PASS marker_defect_is "$not_run" "$gate_scratch/empty" "$marker" "$gate_scratch/stdout" "$gate_scratch/stderr"
  t empty-marker-is-no-leak PASS marker_defect_is "$not_run" "$gate_scratch/clean" "" "$gate_scratch/stdout" "$gate_scratch/stderr"
  t empty-marker-on-empty-capture FAIL predicate_marker_not_printed "" "$gate_scratch/stderr"
  standin_build || { emit_fail "self-test:$name" "cc could not build helper/load-standin.c"; return 1; }
  t standin-store PASS against_the_standin ok none
  t standin-marker-in-the-directory FAIL against_the_standin leak-in-dir none
  t standin-marker-on-stderr FAIL against_the_standin leak-on-stderr none
  t standin-rows-0 FAIL against_the_standin rows-0 none
  t standin-binary-fails FAIL against_the_standin exit-1 none
  t standin-marker-on-stdout FAIL against_the_standin leak-on-stdout none
  t standin-no-store-files FAIL against_the_standin no-store-files none
  t standin-report-then-exit-1 FAIL against_the_standin report-then-exit-1 none
  t standin-verify-past-its-deadline FAIL fails_saying 'verify-store ran past its deadline of 1s and was ended' past_the_deadline
  t term-ends-the-verify-child PASS signal_leaves_no_child sh TERM
  t hup-ends-the-verify-child PASS signal_leaves_no_child sh HUP
  if command -v dash > /dev/null 2>&1; then
    t term-ends-the-verify-child-under-dash PASS signal_leaves_no_child dash TERM
  fi
  for pair in rows=2000 secure_delete=1 temp_store=2 cipher_provider=libtomcrypt segment_roundtrip=ok; do
    line=$(with "s/ $pair//")
    t "${pair%%=*}-absent" FAIL predicate_store_report "$line"
  done
  t good-line PASS predicate_store_report "$good"
  t line-of-store-4 PASS predicate_store_report "store events=2000 records=2000 secure_delete=1 temp_store=2 cipher_provider=libtomcrypt segment_roundtrip=ok unwrap=ok unwrap_ms=3 prompt=none"
  line=$(with 's/ prompt=none//'); t defects-name-the-key PASS defects_are "$line" "prompt "
  line=$(with 's/rows=2000/rows=0/'); t rows-0 FAIL predicate_store_report "$line"
  line=$(with 's/rows=2000/events=0/'); t events-0 FAIL predicate_store_report "$line"
  line=$(with 's/rows=2000 //'); t event-count-missing FAIL predicate_store_report "$line"
  line=$(with 's/rows=2000/rows=2000 events=2000/'); t both-names-of-one-figure FAIL predicate_store_report "$line"
  line=$(with 's/rows=2000/events=2000 records=1999/'); t records-1999-when-present FAIL predicate_store_report "$line"
  line=$(with 's/temp_store=2/temp_store=1/'); t temp-store-1 FAIL predicate_store_report "$line"
  line=$(with 's/fts_secure_delete=1/fts_secure_delete=0/'); t fts-0-when-present FAIL predicate_store_report "$line"
  line=$(with 's/segment_roundtrip=ok/segment_roundtrip=fail/'); t roundtrip-fail FAIL predicate_store_report "$line"
  line=$(with 's/cipher_provider=libtomcrypt/cipher_provider=openssl/'); t provider-openssl FAIL predicate_store_report "$line"
  line=$(with 's/secure_delete=1/secure_delete=0/'); t secure-delete-0 FAIL predicate_store_report "$line"
  line=$(with 's/ unwrap=ok//'); t unwrap-missing FAIL predicate_store_report "$line"
  line=$(with 's/unwrap=ok/unwrap=fail/'); t unwrap-fail FAIL predicate_store_report "$line"
  line=$(with 's/prompt=none/prompt=shown/'); t prompt-shown FAIL predicate_store_report "$line"
  line=$(with 's/ prompt=none//'); t field-missing FAIL predicate_store_report "$line"
  t two-fields-only FAIL predicate_store_report "store unwrap=ok prompt=none"
  t line-printed-twice FAIL predicate_store_report "$good $good"
  line=$(printf '%s\nrecords=1999' "$good"); t records-1999-on-a-line-of-its-own FAIL predicate_store_report "$line"
  t records-on-a-line-of-its-own-is-named PASS defects_are "$line" "records "
  no_fts=$(with 's/ fts_secure_delete=1//')
  line=$(printf '%s\nfts_secure_delete=0' "$no_fts"); t fts-0-on-a-line-of-its-own FAIL predicate_store_report "$line"
  line=$(printf '%s\nevents=0' "$good"); t events-0-on-a-second-line FAIL predicate_store_report "$line"
  line=$(printf '%s\nxrecords=1999' "$good"); t key-inside-another-word PASS predicate_store_report "$line"
  line=$(printf '%s\n\trecords=1999' "$good"); t records-1999-after-a-tab FAIL predicate_store_report "$line"
  line=$(printf '%s\n\rrecords=1999' "$good"); t records-1999-after-a-carriage-return FAIL predicate_store_report "$line"
  no_prompt=$(with 's/ prompt=none//')
  line=$(printf '%s\tprompt=none' "$no_prompt"); t prompt-none-after-a-tab-is-read PASS predicate_store_report "$line"
}

past_the_deadline() {
  export GATE_OVERRIDE_child_deadline_extra_s=1
  against_the_standin hang none
}

against_the_standin() {
  standin_as_the_binary "$1" "$2"
  measure
}

measure() {
  build_for_measure || { fail_defect; return 1; }
  scratch_root
  store=$gate_scratch/${capture_label}store; out=$gate_scratch/${capture_label}stdout; err=$gate_scratch/${capture_label}stderr
  mkdir "$store"
  marker="HALVO_GATE_MARKER_$$"
  export HALVO_STORE_DIR="$store" HALVO_GATE_MARKER="$marker"
  if watched "$capture_bin" --verify-store --store-dir "$store" --marker "$marker" > "$out" 2> "$err"; then
    if ! defect=$(marker_defect "$store" "$marker" "$out" "$err"); then
      emit_fail "$name" "$defect"; return 1
    fi
    report=$(cat "$out")
    defects=$(store_report_defects "$report")
    if [ -z "$defects" ]; then emit_pass "$name"; return 0; fi
    emit_fail "$name" "verify-store measured: ${defects% }"; return 1
  fi
  [ -z "$watched_defect" ] || { emit_fail "$name" "verify-store $watched_defect"; return 1; }
  first=$(head -1 "$err" 2>/dev/null)
  emit_fail "$name" "verify-store unavailable: $first"; return 1
}

check_main "$@"
