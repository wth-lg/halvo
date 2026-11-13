#!/bin/sh
# jobs: windows
# rule: from the files of HEAD, halvo-sample build of the golden script writes its committed files byte for byte, check-labels passes on them under both rules and on each committed development body, and two builds of each development body from its committed scripts print one manifest hash, that of its committed manifest, and write its committed files; no tracked file changes.
set -u
name=30-sample
keys='cargo-build golden-build golden-files golden-generated golden-recorded gen-dev-build-1 gen-dev-manifest-1 gen-dev-build-2 gen-dev-manifest-2 gen-dev-committed gen-dev-files gen-dev-labels gen-unseen-build-1 gen-unseen-manifest-1 gen-unseen-build-2 gen-unseen-manifest-2 gen-unseen-committed gen-unseen-files gen-unseen-labels'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

self_fail() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

value_of() {
  sed -n "s/^$1 //p" "$2"
}

# The decision step: reads the results file, one line `<key> <value>` for each command that ran, and
# prints FAIL for the first line that does not hold. With `final` every key of the run must stand there.
judge() {
  cp "$1" "$1.seen" || fail "the results cannot be read"
  twice=$(awk '{ if (seen[$1]++) { print $1; exit } }' "$1") || fail "the results cannot be read"
  [ -z "$twice" ] || fail "the result $twice stands twice"
  while IFS=' ' read -r key value; do
    case $key in
      cargo-build | golden-build | golden-generated | golden-recorded | gen-*-build-1 | gen-*-build-2 | gen-*-labels)
        [ "$value" = 0 ] || fail "$key exited $value"
        ;;
      golden-files | gen-*-files)
        if [ "$value" != same ]; then
          why=$(differing "$value") || why="files that differ from HEAD: unknown"
          fail "$key: $why"
        fi
        ;;
      gen-*-manifest-1)
        printf '%s\n' "$value" | grep -Eqx '[0-9a-f]{64}' || fail "$key: no manifest line"
        ;;
      gen-*-manifest-2)
        first=$(value_of "${key%-2}-1" "$1.seen") || fail "the results cannot be read"
        [ "$value" = "$first" ] || fail "$key: the two builds print different manifest hashes"
        ;;
      gen-*-committed)
        first=$(value_of "${key%-committed}-manifest-1" "$1.seen") || fail "the results cannot be read"
        [ "$value" = "$first" ] || fail "$key: the build prints another hash than that of the committed manifest"
        ;;
      tracked-*)
        [ "$value" = clean ] || fail "a tracked file differs from HEAD after ${key#tracked-}"
        ;;
      *) fail "unknown result $key" ;;
    esac
  done <"$1"
  if [ "${2:-}" = final ]; then
    for key in $keys; do
      grep -q "^$key " "$1" || fail "no result for $key"
    done
  fi
  return 0
}

differing() {
  rest=${1#differ}
  rest=${rest# }
  n=${rest%% *}
  first=${rest#"$n"}
  first=${first# }
  printf '%s\n' "$n" | grep -Eqx '[0-9]+' || n=unknown
  if printf '%s\n' "$first" | grep -Eqx 'streams/[a-z0-9-]{1,48}/(day-[0-9]{3}|labels)\.jsonl|manifest\.toml|script-manifest\.jsonl|layers/steps\.jsonl'; then
    printf 'files that differ from HEAD: %s, the first %s\n' "$n" "$first"
  else
    printf 'files that differ from HEAD: %s\n' "$n"
  fi
}

record() {
  printf '%s %s\n' "$1" "$2" >>"$results" || fail "the results cannot be written"
  status=$(git status --porcelain --untracked-files=no) || fail "git status failed"
  if [ -n "$status" ]; then
    printf 'tracked-%s changed\n' "$1" >>"$results" || fail "the results cannot be written"
  fi
  judge "$results"
}

sample() {
  "$bin" "$@"
}

sha() {
  if command -v sha256sum >/dev/null 2>&1; then
    sum=$(sha256sum <"$1") || return 1
  else
    sum=$(shasum -a 256 <"$1") || return 1
  fi
  printf '%s\n' "${sum%% *}"
}

# Prints `same` when the two directories hold the same files with the same bytes, `differ` otherwise.
same_tree() {
  diff -r -q "$1" "$2" >"$work/diff.txt" 2>&1
  case $? in
    0) echo same ;;
    1)
      n=$(grep -c . "$work/diff.txt") || return 1
      IFS= read -r line <"$work/diff.txt" || return 1
      case $line in
        "Files $1/"*) rel=${line#"Files $1/"}; rel=${rel%% and *} ;;
        "Only in $1: "* | "Only in $2: "*) rel=${line#*: } ;;
        "Only in $1/"*) rel=${line#"Only in $1/"}; rel="${rel%%: *}/${line#*: }" ;;
        "Only in $2/"*) rel=${line#"Only in $2/"}; rel="${rel%%: *}/${line#*: }" ;;
        *) rel= ;;
      esac
      echo "differ $n $rel"
      ;;
    *) return 1 ;;
  esac
}

measure() {
  [ -n "${CI_D_REPO:-}" ] || fail "CI_D_REPO is not set"
  [ -n "${CI_D_ARTIFACTS:-}" ] || fail "CI_D_ARTIFACTS is not set"
  cd "$CI_D_REPO" || fail "the repository directory cannot be entered"
  if ! command -v cargo >/dev/null 2>&1; then
    [ "${CI:-}" = true ] && fail "cargo is not installed"
    printf 'SKIP %s (cargo is not installed)\n' "$name"
    exit 77
  fi
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  results=$work/results.txt
  : >"$results" || fail "the results cannot be written"
  git archive --output="$work/head.tar" HEAD -- fixtures/r2/golden fixtures/r2/scripts/seen fixtures/r2/scripts/unseen fixtures/r2/dev ||
    fail "git archive of the inputs at HEAD failed"
  mkdir "$work/head" || fail "mkdir failed"
  tar -xf "$work/head.tar" -C "$work/head" || fail "the inputs of HEAD cannot be unpacked"
  head=$work/head/fixtures/r2
  g=$head/golden
  cargo build --locked -q -p halvo-sample
  record cargo-build "$?"
  case ${CARGO_TARGET_DIR:-} in
    '') bin=$CI_D_REPO/target/debug/halvo-sample ;;
    /* | [A-Za-z]:*) bin=$CARGO_TARGET_DIR/debug/halvo-sample ;;
    *) bin=$CI_D_REPO/$CARGO_TARGET_DIR/debug/halvo-sample ;;
  esac
  [ -f "$bin.exe" ] && bin=$bin.exe
  sample build --seed 1 --out "$work/golden" "$g/script.jsonl" "$g/marked.jsonl" >"$CI_D_ARTIFACTS/golden-build.txt"
  record golden-build "$?"
  mkdir "$work/committed" || fail "mkdir failed"
  cp -R "$g/streams" "$g/script-manifest.jsonl" "$work/committed/" || fail "the golden files cannot be copied"
  files=$(same_tree "$work/golden" "$work/committed") || fail "the golden files cannot be compared"
  record golden-files "$files"
  sample check-labels "$g" >"$CI_D_ARTIFACTS/golden-generated.txt"
  record golden-generated "$?"
  sample check-labels --rule recorded "$g" >"$CI_D_ARTIFACTS/golden-recorded.txt"
  record golden-recorded "$?"
  for pair in gen-dev:seen gen-unseen:unseen; do
    tier=${pair%%:*}
    side=${pair#*:}
    seed=$(sed -n 's/^seed = //p' "$head/dev/$side/manifest.toml") || fail "the seed of $tier cannot be read"
    for n in 1 2; do
      (cd "$head/scripts/$side" && sample build --tier "$tier" --seed "$seed" --out "$work/$tier-$n" ./*.jsonl) >"$CI_D_ARTIFACTS/$tier-build-$n.txt"
      record "$tier-build-$n" "$?"
      line=$(sed -n 's/^manifest //p' "$CI_D_ARTIFACTS/$tier-build-$n.txt") || fail "the output of build cannot be read"
      record "$tier-manifest-$n" "$line"
    done
    committed=$(sha "$head/dev/$side/manifest.toml") || fail "the committed manifest of $tier cannot be hashed"
    record "$tier-committed" "$committed"
    files=$(same_tree "$work/$tier-1" "$head/dev/$side") || fail "the files of $tier cannot be compared"
    record "$tier-files" "$files"
    sample check-labels "$head/dev/$side" >"$CI_D_ARTIFACTS/$tier-labels.txt"
    record "$tier-labels" "$?"
    grep -E '^(counts|rates|masks) ' "$CI_D_ARTIFACTS/$tier-build-1.txt" || fail "the output of build cannot be read"
    grep -E '^checked ' "$CI_D_ARTIFACTS/$tier-labels.txt" || fail "the output of check-labels cannot be read"
  done
  judge "$results" final
  printf 'PASS %s\n' "$name"
}

passing_results() {
  cat <<'EOF'
cargo-build 0
golden-build 0
golden-files same
golden-generated 0
golden-recorded 0
gen-dev-build-1 0
gen-dev-manifest-1 1111111111111111111111111111111111111111111111111111111111111111
gen-dev-build-2 0
gen-dev-manifest-2 1111111111111111111111111111111111111111111111111111111111111111
gen-dev-committed 1111111111111111111111111111111111111111111111111111111111111111
gen-dev-files same
gen-dev-labels 0
gen-unseen-build-1 0
gen-unseen-manifest-1 2222222222222222222222222222222222222222222222222222222222222222
gen-unseen-build-2 0
gen-unseen-manifest-2 2222222222222222222222222222222222222222222222222222222222222222
gen-unseen-committed 2222222222222222222222222222222222222222222222222222222222222222
gen-unseen-files same
gen-unseen-labels 0
EOF
}

check() {
  sed "$3" "$work/passing.txt" >"$work/$1.txt" || self_fail "the fixture $1 cannot be written"
  if [ -n "${4:-}" ]; then
    printf '%s\n' "$4" >>"$work/$1.txt" || self_fail "the fixture $1 cannot be written"
  fi
  out=$( (judge "$work/$1.txt" final) 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  if [ -n "${5:-}" ] && [ "$last" != "$5" ]; then
    code=line
  fi
  case $code:$last in
    "0:") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$got" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || self_fail "mktemp failed"
  passing_results >"$work/passing.txt" || self_fail "the fixture cannot be written"
  rejected=0
  wrong=
  check all-hold PASS 's/^//'
  check cargo-build-fails FAIL 's/^cargo-build 0$/cargo-build 101/'
  check golden-build-refused FAIL 's/^golden-build 0$/golden-build 2/'
  check golden-files-differ FAIL 's/^golden-files same$/golden-files differ/'
  check golden-generated-fails FAIL 's/^golden-generated 0$/golden-generated 1/'
  check golden-recorded-fails FAIL 's/^golden-recorded 0$/golden-recorded 1/'
  check dev-build-1-refused FAIL 's/^gen-dev-build-1 0$/gen-dev-build-1 2/'
  check dev-build-2-refused FAIL 's/^gen-dev-build-2 0$/gen-dev-build-2 2/'
  check dev-no-manifest-line FAIL 's/^gen-dev-manifest-1 .*$/gen-dev-manifest-1 /'
  check dev-two-hashes FAIL 's/^gen-dev-manifest-2 1/gen-dev-manifest-2 3/'
  check dev-not-the-committed-hash FAIL 's/^gen-dev-committed 1/gen-dev-committed 4/'
  check dev-files-differ FAIL 's/^gen-dev-files same$/gen-dev-files differ/'
  check dev-labels-fail FAIL 's/^gen-dev-labels 0$/gen-dev-labels 1/'
  check unseen-two-hashes FAIL 's/^gen-unseen-manifest-2 2/gen-unseen-manifest-2 5/'
  check unseen-not-the-committed-hash FAIL 's/^gen-unseen-committed 2/gen-unseen-committed 6/'
  check unseen-files-differ FAIL 's/^gen-unseen-files same$/gen-unseen-files differ/'
  check unseen-labels-fail FAIL 's/^gen-unseen-labels 0$/gen-unseen-labels 1/'
  check tracked-file-changed FAIL 's/^//' 'tracked-gen-dev-build-1 changed'
  check unknown-result FAIL 's/^//' 'gen-dev-other 0'
  check unseen-labels-missing FAIL '/^gen-unseen-labels /d'
  check golden-files-missing FAIL '/^golden-files /d'
  check golden-build-twice FAIL 's/^//' 'golden-build 0'
  check golden-files-twice FAIL 's/^//' 'golden-files same'
  check labels-twice FAIL 's/^//' 'gen-dev-labels 0'
  check golden-files-name-the-first FAIL 's|^golden-files same$|golden-files differ 2 streams/golden/day-003.jsonl|' '' \
    "FAIL $name (golden-files: files that differ from HEAD: 2, the first streams/golden/day-003.jsonl)"
  check dev-files-name-a-file-of-the-body FAIL 's|^gen-dev-files same$|gen-dev-files differ 1 manifest.toml|' '' \
    "FAIL $name (gen-dev-files: files that differ from HEAD: 1, the first manifest.toml)"
  check unseen-files-count-alone-for-another-path FAIL 's|^gen-unseen-files same$|gen-unseen-files differ 4 notes/x.txt|' '' \
    "FAIL $name (gen-unseen-files: files that differ from HEAD: 4)"
  check dev-files-count-alone-for-a-short-day FAIL 's|^gen-dev-files same$|gen-dev-files differ 3 streams/gen-dev-s001/day-1.jsonl|' '' \
    "FAIL $name (gen-dev-files: files that differ from HEAD: 3)"
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (unknown argument)\n' "$name"
    exit 64
    ;;
esac
