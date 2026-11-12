#!/bin/sh
# jobs: macos
# rule: The backend's licence gate, check 85 (sh backend/gate/checks/85-licenses.sh), exits 0 with the last line PASS licenses: packages=<n> exceptions=29 bundled=<n> cached=<n>, where 29 is the count of the owner's item D28, the packages outside the owner's licences that backend/gate/license-exceptions.json names each with its licence, chain and reason, while backend/gate/license-allowlist.json at HEAD holds the owner's seven licences (MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0) and no other, and no tracked file differs from HEAD after the check.
set -u
name=25-r7-backend-licenses
d28=29
owner_licences='Apache-2.0 BSD-2-Clause BSD-3-Clause ISC MIT Unicode-3.0 Zlib'
allowlist=backend/gate/license-allowlist.json
exceptions=backend/gate/license-exceptions.json
fixtures=backend/gate/fixtures/license-lists

read_lists() {
  git -C "$1" show "HEAD:$allowlist" >"$2/allowlist.json" 2>/dev/null || {
    lists='allowlist=unread'
    cause="$allowlist is not in HEAD"
    return 1
  }
  git -C "$1" show "HEAD:$exceptions" >"$2/exceptions.json" 2>/dev/null || {
    lists='allowlist=unread'
    cause="$exceptions is not in HEAD"
    return 1
  }
  lists=$(node - "$2/allowlist.json" "$2/exceptions.json" "$owner_licences" <<'EOF'
const fs = require("fs");
const [allowFile, exceptFile, owner] = process.argv.slice(2);
const read = (file) => {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return undefined;
  }
};
const allow = read(allowFile);
const entries = read(exceptFile);
const want = owner.split(" ");
const owners = Array.isArray(allow) && allow.length === want.length && want.every((id) => allow.includes(id));
const fields = ["package", "license", "chain", "reason"];
const named =
  Array.isArray(entries) &&
  entries.length > 0 &&
  entries.every((entry) => entry !== null && fields.every((field) => typeof entry[field] === "string" && entry[field].trim() !== ""));
console.log(`allowlist=${owners ? "owner" : "other"} entries=${Array.isArray(entries) ? entries.length : "unread"} named=${named}`);
process.exit(!owners ? 3 : !named ? 4 : 0);
EOF
)
  case $? in
    0) return 0 ;;
    3) cause="$allowlist at HEAD is not the owner's list ($owner_licences)" ;;
    4) cause="an entry of $exceptions at HEAD lacks its package, license, chain or reason" ;;
    *)
      lists='allowlist=unread'
      cause="node could not read the lists"
      ;;
  esac
  return 1
}

check_in() {
  at=$1
  out=$2
  state=$3
  cause=
  read_lists "$at" "$state"
  mkdir -p "$state/gate-state" || return 2
  (cd "$at" && HALVO_ROOT="$at/backend" GATE_STATE="$state/gate-state" sh backend/gate/checks/85-licenses.sh) >"$out/check85.txt" 2>&1
  code=$?
  line=$(sed -n '$p' "$out/check85.txt")
  excepted=$(printf '%s\n' "$line" | sed -n 's/^PASS licenses: packages=[0-9][0-9]* exceptions=\([0-9][0-9]*\) bundled=[0-9][0-9]* cached=[0-9][0-9]*$/\1/p')
  if git -C "$at" diff --quiet HEAD --; then
    changed=0
  else
    changed=1
  fi
  counts="$lists check85-exit=$code exceptions=${excepted:-none} tracked-changed=$changed"
  [ "$changed" = 0 ] || cause="a tracked file differs from HEAD after check 85"
  [ -n "$cause" ] || [ "$code" = 0 ] || cause="check 85 exited $code"
  [ -n "$cause" ] || [ -n "$excepted" ] || cause="the last line of check 85 is no PASS line"
  [ -n "$cause" ] || [ "$excepted" = "$d28" ] || cause="check 85 counts $excepted packages outside the owner's list, not the $d28 of the owner's item D28"
  [ -z "$cause" ]
}

measure() {
  top=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$top/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  counts=unmeasured
  if check_in "$top" "$artifacts" "$work"; then
    printf '%s\n' "$counts"
    printf 'PASS %s\n' "$name"
    exit 0
  fi
  printf '%s\n' "$counts"
  printf 'FAIL %s (%s)\n' "$name" "${cause:-check 85 could not run}"
  exit 1
}

verdict() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$2" "$1" "$3"
  if [ "$3" != "$1" ]; then
    wrong="$wrong $2"
  elif [ "$1" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

lists_fixture() {
  file=$1
  case_name=$(basename "$file" .json)
  case $case_name in
    good*) want=PASS ;;
    *) want=FAIL ;;
  esac
  expected=$(node - "$top/$fixtures/good.json" "$file" "$top/$exceptions" "$scratch/$case_name.json" <<'EOF'
const fs = require("fs");
const [goodFile, file, exceptFile, outFile] = process.argv.slice(2);
const good = JSON.parse(fs.readFileSync(goodFile, "utf8"));
const one = JSON.parse(fs.readFileSync(file, "utf8"));
const packages = file === goodFile ? good.packages : [...good.packages, ...one.packages];
const input = { packages, bundled: good.bundled ?? [], exceptions: JSON.parse(fs.readFileSync(exceptFile, "utf8")) };
fs.writeFileSync(outFile, JSON.stringify(input));
console.log(one.expect);
EOF
)
  if [ -z "$expected" ]; then
    verdict "$want" "$case_name" unreadable
    return
  fi
  (cd "$top/backend" && node gate/lib/licenses.mjs --fixture "$scratch/$case_name.json") >"$scratch/$case_name.out" 2>&1
  code=$?
  last=$(sed -n '$p' "$scratch/$case_name.out")
  case $code:$last in
    "0:$expected") got=PASS ;;
    "1:$expected") got=FAIL ;;
    *) got="exit-$code-with-another-line" ;;
  esac
  verdict "$want" "$case_name" "$got"
}

stub_repo() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/gate/checks" "$dir/state" "$dir/out" "$dir/work" || exit 2
  cat >"$dir/backend/gate/checks/85-licenses.sh" <<'EOF'
#!/bin/sh
[ -n "${HALVO_ROOT:-}" ] || { echo "HALVO_ROOT is not set"; exit 0; }
cat "$STUB_STATE/lines"
[ ! -f "$STUB_STATE/write" ] || printf '\n' >>"$HALVO_ROOT/gate/license-exceptions.json"
exit "$(cat "$STUB_STATE/exit")"
EOF
  printf '["Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$dir/$allowlist"
  printf '[{"package": "a-tool", "license": "MPL-2.0", "chain": "a-dev-tool@1.0.0 > a-tool@1.0.0", "reason": "A test tool."}]\n' >"$dir/$exceptions"
  printf 'PASS licenses: packages=231 exceptions=29 bundled=1 cached=121\n' >"$dir/state/lines"
  printf '0\n' >"$dir/state/exit"
}

commit_stub() {
  git -C "$1" init -q || exit 2
  git -C "$1" add -A backend || exit 2
  git -C "$1" -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m "stub" >/dev/null || exit 2
}

stub_case() {
  want=$1
  case_name=$2
  dir=$scratch/$case_name
  (
    STUB_STATE=$dir/state
    export STUB_STATE
    check_in "$dir" "$dir/out" "$dir/work"
  ) >/dev/null 2>&1
  code=$?
  case $code in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=exit-$code ;;
  esac
  verdict "$want" "$case_name" "$got"
}

self_test() {
  top=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  if [ ! -f "$top/$fixtures/good.json" ]; then
    printf 'FAIL self-test:%s (%s/good.json is missing)\n' "$name" "$fixtures"
    exit 1
  fi
  for file in "$top/$fixtures"/*.json; do
    lists_fixture "$file"
  done

  stub_repo good
  commit_stub "$scratch/good"
  stub_case PASS good

  stub_repo count-of-the-old-lists
  commit_stub "$scratch/count-of-the-old-lists"
  printf 'PASS licenses: packages=231 exceptions=26 bundled=1 cached=121\n' >"$scratch/count-of-the-old-lists/state/lines"
  stub_case FAIL count-of-the-old-lists

  stub_repo count-with-a-digit-more
  commit_stub "$scratch/count-with-a-digit-more"
  printf 'PASS licenses: packages=231 exceptions=290 bundled=1 cached=121\n' >"$scratch/count-with-a-digit-more/state/lines"
  stub_case FAIL count-with-a-digit-more

  stub_repo pass-line-and-exit-1
  commit_stub "$scratch/pass-line-and-exit-1"
  printf '1\n' >"$scratch/pass-line-and-exit-1/state/exit"
  stub_case FAIL pass-line-and-exit-1

  stub_repo fail-line-and-exit-0
  commit_stub "$scratch/fail-line-and-exit-0"
  printf 'FAIL licenses: 1 problems; first: a@1.0.0 has the license 0BSD, which is neither on the allowlist nor in the exceptions\n' >"$scratch/fail-line-and-exit-0/state/lines"
  stub_case FAIL fail-line-and-exit-0

  stub_repo pass-line-not-last
  commit_stub "$scratch/pass-line-not-last"
  printf 'PASS licenses: packages=231 exceptions=29 bundled=1 cached=121\nanother line\n' >"$scratch/pass-line-not-last/state/lines"
  stub_case FAIL pass-line-not-last

  stub_repo allowlist-with-0bsd
  printf '["0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$scratch/allowlist-with-0bsd/$allowlist"
  commit_stub "$scratch/allowlist-with-0bsd"
  stub_case FAIL allowlist-with-0bsd

  stub_repo allowlist-without-unicode
  printf '["Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Zlib"]\n' >"$scratch/allowlist-without-unicode/$allowlist"
  commit_stub "$scratch/allowlist-without-unicode"
  stub_case FAIL allowlist-without-unicode

  stub_repo allowlist-of-seven-with-cc0-for-zlib
  printf '["Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "CC0-1.0", "ISC", "MIT", "Unicode-3.0"]\n' >"$scratch/allowlist-of-seven-with-cc0-for-zlib/$allowlist"
  commit_stub "$scratch/allowlist-of-seven-with-cc0-for-zlib"
  stub_case FAIL allowlist-of-seven-with-cc0-for-zlib

  stub_repo entry-without-chain
  printf '[{"package": "a-tool", "license": "MPL-2.0", "reason": "A test tool."}]\n' >"$scratch/entry-without-chain/$exceptions"
  commit_stub "$scratch/entry-without-chain"
  stub_case FAIL entry-without-chain

  stub_repo entry-with-a-blank-reason
  printf '[{"package": "a-tool", "license": "MPL-2.0", "chain": "a-dev-tool@1.0.0 > a-tool@1.0.0", "reason": " "}]\n' >"$scratch/entry-with-a-blank-reason/$exceptions"
  commit_stub "$scratch/entry-with-a-blank-reason"
  stub_case FAIL entry-with-a-blank-reason

  stub_repo check-writes-a-tracked-file
  commit_stub "$scratch/check-writes-a-tracked-file"
  : >"$scratch/check-writes-a-tracked-file/state/write"
  stub_case FAIL check-writes-a-tracked-file

  stub_repo allowlist-of-the-work-tree-differs-from-head
  commit_stub "$scratch/allowlist-of-the-work-tree-differs-from-head"
  printf '["0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$scratch/allowlist-of-the-work-tree-differs-from-head/$allowlist"
  stub_case FAIL allowlist-of-the-work-tree-differs-from-head

  stub_repo lists-not-in-head
  mv "$scratch/lists-not-in-head/$allowlist" "$scratch/lists-not-in-head/state/allowlist.json" || exit 2
  commit_stub "$scratch/lists-not-in-head"
  stub_case FAIL lists-not-in-head

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
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
