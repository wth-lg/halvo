#!/bin/sh
# Mutation test for the store and deletion sub-checks: each mutation breaks one
# property the gate claims to measure, and the matching sub-check must FAIL on
# it. Exits 0 iff every mutation is caught and the unmutated code passes.
#
# Mutates the working tree and restores it with git, so the two mutated files
# must be clean when it starts. Safe to run twice.
set -u
DIR=$(cd "$(dirname "$0")" && pwd)
# shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
. "$DIR/lib.sh"
cd "$GATE_ROOT" || exit 1

DB=crates/halvo-capture/src/store/db.rs
EXPIRY=crates/halvo-capture/src/store/expiry.rs
if [ -n "$(git status --porcelain -- "$DB" "$EXPIRY")" ]; then
  printf 'mutation-test: %s and %s must be clean\n' "$DB" "$EXPIRY" >&2
  exit 2
fi
restore() { git checkout -q -- "$DB" "$EXPIRY"; }
trap restore EXIT

rc=0
# mutate <name> <file> <perl substitution> <sub-check>: apply, expect FAIL.
mutate() {
  name=$1; file=$2; expr=$3; check=$4
  restore
  perl -0pi -e "$expr" "$file"
  if git diff --quiet -- "$file"; then
    printf 'FAIL mutation:%s (the patch did not apply)\n' "$name"; rc=1; return
  fi
  out=$(sh "$DIR/check-$check.sh" --measure 2>&1)
  case "$out" in
    FAIL*) printf 'PASS mutation:%s (gate said: %s)\n' "$name" "$out" ;;
    *) printf 'FAIL mutation:%s (gate said: %s)\n' "$name" "$out"; rc=1 ;;
  esac
}

# shellcheck disable=SC2016
mutate delete-all-destroys-nothing "$EXPIRY" \
  's/(fn destroy_store_key\(&mut self\) -> StoreResult<\(\)> \{\n)/$1        if self.dir().is_dir() {\n            return Ok(());\n        }\n/' \
  deletion
mutate shredded-key-survives "$EXPIRY" \
  's/UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = \?1/UPDATE segment_keys SET wrapped_key = wrapped_key WHERE segment_id = ?1/' \
  deletion
mutate checkpoint-not-verified "$DB" \
  's/PRAGMA wal_checkpoint\(TRUNCATE\)/PRAGMA wal_checkpoint(PASSIVE)/; s/if checkpoint\.busy != 0 \|\| checkpoint\.wal_bytes != 0 \{/if checkpoint.busy < 0 {/' \
  deletion
# shellcheck disable=SC2016
mutate segment-roundtrip-broken "$DB" \
  's/        segment::read_segment_file\(&self\.dir, &seg_key, segment_id, out\)\n    \}/        drop(seg_key);\n        let bytes = std::fs::read(segment::segment_path(&self.dir, segment_id)).map_err(io_error)?;\n        out.write_all(&bytes).map_err(io_error)?;\n        Ok(bytes.len() as u64)\n    }/' \
  store

restore
for check in store deletion; do
  out=$(sh "$DIR/check-$check.sh" --measure 2>&1)
  case "$out" in
    PASS*) printf 'PASS unmutated:%s\n' "$check" ;;
    *) printf 'FAIL unmutated:%s (gate said: %s)\n' "$check" "$out"; rc=1 ;;
  esac
done
exit $rc
