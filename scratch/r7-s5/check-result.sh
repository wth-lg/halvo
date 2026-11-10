#!/bin/sh
# Exits 0 when RESULT.md holds a MET or MISSED verdict for every rule line, 1 otherwise.
set -eu
result="$(dirname "$0")/RESULT.md"
status=0
for line in "S5 RULE macos-15" "S5 RULE windows-2025" "S5 RULE both runners" "S6 RULE macos-15" "S6 RULE windows-2025"; do
  if grep -Eq "^$line: (MET|MISSED)( |$)" "$result"; then
    echo "ok       $(grep -E "^$line: " "$result")"
  else
    echo "pending  $line"
    status=1
  fi
done
exit "$status"
