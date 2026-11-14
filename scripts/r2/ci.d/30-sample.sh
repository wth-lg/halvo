#!/bin/sh
# jobs: windows
# rule: from the files of HEAD, every input file that the check reads has the mode 100644 in HEAD's tree; halvo-sample build of the golden script writes its committed files byte for byte and check-labels passes on them under both rules; compose and split build the split scripts again from the rates file and the site files, byte for byte; each development body is built twice from its committed scripts, once into its place of scripts/r2/body.sh and once into the check's own directory from every entry of its scripts directory, both print one manifest hash, that of its committed manifest, both hold exactly the files and hashes that the manifest names, and check-labels passes on the placed body under both rules; no tracked file changes.
set -u
name=30-sample
if [ ! -f "${CI_D_REPO:-.}/scripts/r2/body.sh" ]; then
  printf 'FAIL %s (scripts/r2/body.sh cannot be read)\n' "$name"
  exit 1
fi
# shellcheck source=scripts/r2/body.sh
. "${CI_D_REPO:-.}/scripts/r2/body.sh"
keys='cargo-build modes golden-build golden-files golden-generated golden-recorded split-compose split-split split-seen split-unseen gen-dev-build-1 gen-dev-manifest-1 gen-dev-build-2 gen-dev-manifest-2 gen-dev-committed gen-dev-files gen-dev-labels gen-unseen-build-1 gen-unseen-manifest-1 gen-unseen-build-2 gen-unseen-manifest-2 gen-unseen-committed gen-unseen-files gen-unseen-labels'

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
      cargo-build | golden-build | golden-generated | golden-recorded | split-compose | split-split | gen-*-build-1 | gen-*-build-2 | gen-*-labels)
        [ "$value" = 0 ] || fail "$key exited $value"
        ;;
      modes)
        [ "$value" = same ] || fail "$key: input files whose mode in HEAD is not 100644: ${value#differ }"
        ;;
      split-seen | split-unseen)
        if [ "$value" != same ]; then
          n=${value#differ }
          n=${n%% *}
          printf '%s\n' "$n" | grep -Eqx '[0-9]+' || n=unknown
          fail "$key: the rebuilt split scripts differ from HEAD: $n"
        fi
        ;;
      golden-files)
        if [ "$value" != same ]; then
          why=$(differing "$value") || why="files that differ from HEAD: unknown"
          fail "$key: $why"
        fi
        ;;
      gen-*-files)
        if [ "$value" != same ]; then
          why=$(r2_body_line "$value") || why="files that differ: unknown"
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

modes_of() {
  awk '{ n++; if ($1 != "100644") d++ } END { if (!n) print "empty"; else if (d) print "differ " d; else print "same" }' "$1"
}

with_entries() {
  (
    cd "$1" || exit 2
    shift
    for f in ./* ./.[!.]* ./..?*; do
      if [ -e "$f" ] || [ -L "$f" ]; then
        set -- "$@" "$f"
      fi
    done
    "$@"
  )
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
  inputs='fixtures/r2/golden fixtures/r2/scripts fixtures/r2/dev/seen/manifest.toml fixtures/r2/dev/unseen/manifest.toml tools/fixture-server/sites'
  # shellcheck disable=SC2086 # the list of input paths is split on purpose
  git archive --output="$work/head.tar" HEAD -- $inputs ||
    fail "git archive of the inputs at HEAD failed"
  mkdir "$work/head" || fail "mkdir failed"
  tar -xf "$work/head.tar" -C "$work/head" || fail "the inputs of HEAD cannot be unpacked"
  head=$work/head/fixtures/r2
  g=$head/golden
  cargo build --locked -q -p halvo-sample
  record cargo-build "$?"
  # shellcheck disable=SC2086 # the list of input paths is split on purpose
  git ls-tree -r HEAD -- $inputs >"$work/modes.txt" || fail "git ls-tree of the inputs at HEAD failed"
  modes=$(modes_of "$work/modes.txt") || fail "the modes of the inputs cannot be read"
  record modes "$modes"
  bin=$(r2_body_bin "$CI_D_REPO") || fail "the build directory cannot be read"
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
  started=$(date +%s) || fail "date failed"
  split_seed=$(sed -n 's/^seed = //p' "$head/scripts/rates.toml") || fail "the rates file cannot be read"
  with_entries "$work/head/tools/fixture-server/sites" sample compose --rates "$head/scripts/rates.toml" --out "$work/composed" >"$work/compose.txt" 2>&1
  record split-compose "$?"
  with_entries "$work/composed" sample split --sides seen,unseen --seed "$split_seed" --out "$work/split" >"$work/split.txt" 2>&1
  record split-split "$?"
  for side in seen unseen; do
    files=$(same_tree "$work/split/$side" "$head/scripts/$side") || fail "the rebuilt split scripts cannot be compared"
    record "split-$side" "$files"
  done
  ended=$(date +%s) || fail "date failed"
  printf 'split scripts rebuilt and compared in %s s\n' "$((ended - started))"
  for pair in gen-dev:seen gen-unseen:unseen; do
    tier=${pair%%:*}
    side=${pair#*:}
    committed_manifest=$head/dev/$side/manifest.toml
    seed=$(sed -n 's/^seed = //p' "$committed_manifest") || fail "the seed of $tier cannot be read"
    body=$(r2_body_dir "$CI_D_REPO" "$tier") || fail "the place of the body of $tier cannot be read"
    r2_body_build "$CI_D_REPO" "$tier" "$bin" "$head/scripts/$side" "$committed_manifest" "$CI_D_ARTIFACTS/$tier-build-1.txt" >"$work/$tier-body.txt"
    code=$?
    cat "$work/$tier-body.txt" || fail "the body line of $tier cannot be read"
    record "$tier-build-1" "$code"
    with_entries "$head/scripts/$side" sample build --tier "$tier" --seed "$seed" --out "$work/$tier-2" >"$CI_D_ARTIFACTS/$tier-build-2.txt"
    record "$tier-build-2" "$?"
    for n in 1 2; do
      line=$(sed -n 's/^manifest //p' "$CI_D_ARTIFACTS/$tier-build-$n.txt") || fail "the output of build cannot be read"
      record "$tier-manifest-$n" "$line"
    done
    committed=$(sha "$committed_manifest") || fail "the committed manifest of $tier cannot be hashed"
    record "$tier-committed" "$committed"
    started=$(date +%s) || fail "date failed"
    files=$(r2_body_check "$body" "$committed_manifest") || fail "the body of $tier cannot be checked"
    if [ "$files" = same ]; then
      files=$(r2_body_check "$work/$tier-2" "$committed_manifest") || fail "the second build of $tier cannot be checked"
    fi
    ended=$(date +%s) || fail "date failed"
    printf 'check %s %s s\n' "$tier" "$((ended - started))"
    record "$tier-files" "$files"
    sample check-labels "$body" >"$CI_D_ARTIFACTS/$tier-labels.txt"
    labels=$?
    sample check-labels --rule recorded "$body" >"$CI_D_ARTIFACTS/$tier-labels-recorded.txt"
    recorded=$?
    [ "$labels" = 0 ] && labels=$recorded
    record "$tier-labels" "$labels"
    grep -E '^(counts|rates|masks) ' "$CI_D_ARTIFACTS/$tier-build-1.txt" || fail "the output of build cannot be read"
    grep -E '^checked ' "$CI_D_ARTIFACTS/$tier-labels.txt" "$CI_D_ARTIFACTS/$tier-labels-recorded.txt" || fail "the output of check-labels cannot be read"
  done
  judge "$results" final
  printf 'PASS %s\n' "$name"
}

passing_results() {
  cat <<'EOF'
cargo-build 0
modes same
golden-build 0
golden-files same
golden-generated 0
golden-recorded 0
split-compose 0
split-split 0
split-seen same
split-unseen same
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

body_case() {
  cp -R "$work/body" "$work/case-$1" || self_fail "the body fixture $1 cannot be copied"
  case $1 in
    changed) printf 'four\n' >"$work/case-$1/streams/gen-dev-s001/day-001.jsonl" ;;
    missing) mv "$work/case-$1/streams/gen-dev-s001/labels.jsonl" "$work/moved-$1" ;;
    extra) printf 'five\n' >"$work/case-$1/streams/gen-dev-s001/day-002.jsonl" ;;
    manifest-byte) sed 's/^seed = 7$/seed = 8/' "$work/committed.toml" >"$work/case-$1/manifest.toml" ;;
    *) : ;;
  esac
  state=$(r2_body_check "$work/case-$1" "$work/committed.toml") || state=unread
  if [ "$state" = same ]; then
    got=same
  else
    got=$(r2_body_line "$state") || got=unread
  fi
  if [ "$got" != "$2" ]; then
    wrong="$wrong body-$1"
  elif [ "$got" != same ]; then
    rejected=$((rejected + 1))
  fi
}

body_fixtures() {
  mkdir -p "$work/body/streams/gen-dev-s001" || self_fail "the body fixture cannot be made"
  printf 'one\n' >"$work/body/streams/gen-dev-s001/day-001.jsonl" || self_fail "the body fixture cannot be written"
  printf 'two\n' >"$work/body/streams/gen-dev-s001/labels.jsonl" || self_fail "the body fixture cannot be written"
  printf 'three\n' >"$work/body/script-manifest.jsonl" || self_fail "the body fixture cannot be written"
  printf 'seed = 7\n' >"$work/committed.toml" || self_fail "the body fixture cannot be written"
  for f in script-manifest.jsonl streams/gen-dev-s001/day-001.jsonl streams/gen-dev-s001/labels.jsonl; do
    h=$(sha "$work/body/$f") || self_fail "the body fixture cannot be hashed"
    printf '[[file]]\npath = "%s"\nsha256 = "%s"\n' "$f" "$h" >>"$work/committed.toml" || self_fail "the body fixture cannot be written"
  done
  cp "$work/committed.toml" "$work/body/manifest.toml" || self_fail "the body fixture cannot be written"
  body_case untouched same
  body_case changed 'files that differ: 1, missing: 0, extra: 0, the first streams/gen-dev-s001/day-001.jsonl'
  body_case missing 'files that differ: 0, missing: 1, extra: 0, the first streams/gen-dev-s001/labels.jsonl'
  body_case extra 'files that differ: 0, missing: 0, extra: 1, the first streams/gen-dev-s001/day-002.jsonl'
  body_case manifest-byte 'files that differ: 1, missing: 0, extra: 0, the first manifest.toml'
}

modes_case() {
  printf '%s' "$3" >"$work/modes-$1.txt" || self_fail "the modes fixture $1 cannot be written"
  got=$(modes_of "$work/modes-$1.txt") || got=unread
  if [ "$got" != "$2" ]; then
    wrong="$wrong modes-$1"
  elif [ "$got" != same ]; then
    rejected=$((rejected + 1))
  fi
}

modes_fixtures() {
  h=0123456789abcdef0123456789abcdef01234567
  modes_case plain same "100644 blob $h	fixtures/r2/golden/script.jsonl
100644 blob $h	fixtures/r2/scripts/rates.toml
"
  modes_case executable 'differ 1' "100644 blob $h	fixtures/r2/golden/script.jsonl
100755 blob $h	fixtures/r2/scripts/rates.toml
"
  modes_case link 'differ 2' "120000 blob $h	fixtures/r2/golden/script.jsonl
100755 blob $h	fixtures/r2/scripts/rates.toml
"
  modes_case none empty ''
}

entries_fixture() {
  mkdir -p "$work/entries/sub" || self_fail "the entries fixture cannot be made"
  for f in a.jsonl .b.jsonl ..c.jsonl c.json; do
    printf 'x\n' >"$work/entries/$f" || self_fail "the entries fixture cannot be written"
  done
  with_entries "$work/entries" printf '%s\n' >"$work/entries.txt" || self_fail "the entries cannot be listed"
  got=$(LC_ALL=C sort "$work/entries.txt") || got=unread
  [ "$got" = "./..c.jsonl
./.b.jsonl
./a.jsonl
./c.json
./sub" ] || wrong="$wrong entries"
}

# The stand-in for the binary: a few lines of sh, written at test time, that copy the body fixture to the
# directory after --out.
stand_in() {
  # shellcheck disable=SC2016 # the text is the stand-in's own code, expanded when the stand-in runs
  printf '%s\n' '#!/bin/sh' 'while [ $# -gt 0 ]; do' '  case $1 in --out) out=$2; shift ;; esac' '  shift' 'done' \
    "mkdir -p \"\$out\" && cp -R '$work/body/.' \"\$out\"" >"$work/stand-in" || self_fail "the stand-in cannot be written"
  chmod +x "$work/stand-in" || self_fail "the stand-in cannot be made runnable"
  mkdir -p "$work/scripts" || self_fail "the scripts of the stand-in cannot be made"
  printf '{}\n' >"$work/scripts/one.jsonl" || self_fail "the scripts of the stand-in cannot be written"
}

# Calls r2_body_ensure, or the step given second, on the tier gen-dev, with the build directory set for that
# call alone.
ensure_call() {
  (
    CARGO_TARGET_DIR=$1
    export CARGO_TARGET_DIR
    "${2:-r2_body_ensure}" "$work" gen-dev "$work/stand-in" "$work/scripts" "$work/committed.toml" "$work/ensure.log"
  ) 2>&1
}

ensure_case() {
  target=$work/target-$1
  mkdir -p "$target/r2" || self_fail "the build directory of $1 cannot be made"
  cp -R "$work/body" "$target/r2/gen-dev" || self_fail "the body of $1 cannot be placed"
  case $1 in
    stale) printf 'four\n' >"$target/r2/gen-dev/streams/gen-dev-s001/day-001.jsonl" ;;
    *) : ;;
  esac
  out=$(ensure_call "$target")
  case $out in
    'body gen-dev reused') got=reused ;;
    'body gen-dev built '*' s') got=built ;;
    *) got=other ;;
  esac
  state=$(r2_body_check "$target/r2/gen-dev" "$work/committed.toml") || state=unread
  if [ "$got $state" != "$2 same" ]; then
    wrong="$wrong ensure-$1"
  elif [ "$got" = built ]; then
    rejected=$((rejected + 1))
  fi
}

# A build leaves nothing beside the body in place: after a build over a body that checks same and after each
# of five replacements in one shell, the directory of the bodies holds the one name gen-dev.
placing_fixture() {
  target=$work/target-placing
  out=$(ensure_call "$target")
  case $out in
    'body gen-dev built '*' s') : ;;
    *) wrong="$wrong placing-first" ;;
  esac
  ensure_call "$target" r2_body_build >/dev/null
  names=$(ls -A "$target/r2") || names=unread
  [ "$names" = gen-dev ] || wrong="$wrong placing-over-same"
  i=1
  while [ "$i" -le 5 ]; do
    { printf 'four\n' >"$target/r2/gen-dev/streams/gen-dev-s001/day-001.jsonl"; } 2>/dev/null || :
    out=$(ensure_call "$target")
    case $out in
      'body gen-dev built '*' s') : ;;
      *) wrong="$wrong placing-replacement-$i" ;;
    esac
    names=$(ls -A "$target/r2") || names=unread
    [ "$names" = gen-dev ] || wrong="$wrong placing-names-$i"
    i=$((i + 1))
  done
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
  check modes-differ FAIL 's/^modes same$/modes differ 1/' '' "FAIL $name (modes: input files whose mode in HEAD is not 100644: 1)"
  check modes-empty FAIL 's/^modes same$/modes empty/'
  check modes-missing FAIL '/^modes /d'
  check split-compose-refused FAIL 's/^split-compose 0$/split-compose 2/'
  check split-split-refused FAIL 's/^split-split 0$/split-split 2/'
  check split-seen-differs FAIL 's|^split-seen same$|split-seen differ 3 s001.jsonl|' '' "FAIL $name (split-seen: the rebuilt split scripts differ from HEAD: 3)"
  check split-unseen-differs FAIL 's/^split-unseen same$/split-unseen differ 1/'
  check split-unseen-missing FAIL '/^split-unseen /d'
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
  check dev-files-name-a-file-of-the-body FAIL 's|^gen-dev-files same$|gen-dev-files differ 1 0 0 manifest.toml|' '' \
    "FAIL $name (gen-dev-files: files that differ: 1, missing: 0, extra: 0, the first manifest.toml)"
  check unseen-files-count-alone-for-another-path FAIL 's|^gen-unseen-files same$|gen-unseen-files differ 0 0 4 notes/x.txt|' '' \
    "FAIL $name (gen-unseen-files: files that differ: 0, missing: 0, extra: 4)"
  check dev-files-count-alone-for-a-short-day FAIL 's|^gen-dev-files same$|gen-dev-files differ 0 3 0 streams/gen-dev-s001/day-1.jsonl|' '' \
    "FAIL $name (gen-dev-files: files that differ: 0, missing: 3, extra: 0)"
  body_fixtures
  modes_fixtures
  entries_fixture
  stand_in
  ensure_case whole reused
  ensure_case stale built
  placing_fixture
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
