stub() {
  name=$1
  unit=$2
  argument=$3
  if [ "$argument" = --self-test ]; then
    printf 'self-test %s: stub, nothing to prove until %s builds the check\n' "$name" "$unit"
    exit 0
  fi
  printf 'FAIL %s: stub, not built yet (%s)\n' "$name" "$unit"
  exit 1
}
