#!/bin/sh
# Verifier 133, point 1 of S-86 E8: what the log and the journal hold at the last close after a removal.
# Run at the root of the checkout. It adds the probe module, runs it, and takes it out again.
set +e -u
py=python3
if [ "${RUNNER_OS:-}" = Windows ]; then py=python; fi
tmp=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
if [ "${RUNNER_OS:-}" = Windows ]; then tmp=$(cygpath -u "$tmp"); fi
work="$tmp/v133-work-$$"
mkdir -p "$work"
out="$tmp/v133-log.out"
work_native=$work
if [ "${RUNNER_OS:-}" = Windows ]; then work_native=$(cygpath -m "$work"); fi
$py -I scratch-v133/edit.py probe || exit 1
V133_WORK="$work_native" cargo test --locked -p halvo-sqlcipher --lib page_gap::v133::v133_ -- --nocapture --test-threads=1 >"$out" 2>&1
code=$?
tr -d '\r' <"$out" | grep -E 'V133 |^test result|panicked|^error' | grep -v 'V133 which'
lines=$(tr -d '\r' <"$out" | grep -c 'V133 .*closed')
git checkout -- crates/halvo-sqlcipher/tests/page_gap/mod.rs
mv crates/halvo-sqlcipher/tests/page_gap/v133.rs "$tmp/v133.rs.taken-out.$$"
printf 'V133 log probe: cargo test exit %s, lines of a closed case %s, status lines after the restore %s\n' "$code" "$lines" "$(git status --porcelain | wc -l | tr -d ' ')"
if [ "$code" -eq 0 ] && [ "$lines" -ge 11 ]; then exit 0; fi
tr -d '\r' <"$out" | tail -n 30
exit 1
