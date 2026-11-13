#!/bin/sh
# jobs: macos
# rule: At HEAD, backend/gate/license-allowlist.json holds the owner's seven licences and no other, backend/gate/license-exceptions.json names 28 packages each by its exact name with its versions (29, the count of the owner's item D28), licence, chain and reason, every package that the pinned pnpm lists from backend/pnpm-lock.yaml is one that the gate's reader reads, check 85 (sh backend/gate/checks/85-licenses.sh) exits 0 with its PASS line and exceptions=29, and after it no tracked file differs from HEAD and the two lists and the lock are HEAD's bytes.
set -u
name=25-r7-backend-licenses
self=scripts/r7/ci.d/25-r7-backend-licenses.sh
d28=29
owner_licences='Apache-2.0 BSD-2-Clause BSD-3-Clause ISC MIT Unicode-3.0 Zlib'
allowlist=backend/gate/license-allowlist.json
exceptions=backend/gate/license-exceptions.json
lock=backend/pnpm-lock.yaml
fixtures=backend/gate/fixtures/license-lists
work=

cleanup() {
  [ -z "$work" ] || rm -rf "$work"
}
trap cleanup EXIT
trap 'exit 2' INT TERM HUP

usage() {
  cat <<'EOF'
usage: sh scripts/r7/ci.d/25-r7-backend-licenses.sh [--self-test | --help]

The rule. At HEAD, backend/gate/license-allowlist.json holds the owner's seven
licences (MIT, Apache-2.0, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, Unicode-3.0)
and no other. backend/gate/license-exceptions.json names each package outside
them by its exact name and its exact versions, with its licence, the chain that
brings it and the reason that it is neither linked nor shipped: 28 entries, 29
named versions. Every package that the pinned pnpm lists from
backend/pnpm-lock.yaml (pnpm list --lockfile-only) is one that the gate's
reader reads. Check 85 exits 0 and prints exceptions=29. After it, no tracked
file differs from HEAD, and the two lists and the lock are HEAD's bytes.

It runs on a committed tree: it reads the lists and the lock from HEAD and
fails when the work tree differs from HEAD.

When the count changes, three things change together: the exceptions file,
d28 in this script and the owner's item D28.

A limit, named and not fixed: the licence gate trusts the licence field of each
package's manifest, and of the registry's answer for a package that the
platform does not install; it reads no licence text. At 69342faa the licence of
23 of the 29 named packages was read from the registry's statement, and one
installed package, @img/sharp-libvips-darwin-arm64, carries no licence file.
Two forms pass by design: white space around a licence name (" MIT" is MIT),
and a licence given as {"type": ...}, which is read as its type.

It removes the one directory that its own mktemp call made, on every way out.
EOF
}

read_lists() {
  lists='allowlist=unread'
  read_files=
  for file in "$allowlist" "$exceptions" backend/package.json backend/pnpm-workspace.yaml "$lock"; do
    mkdir -p "$2/head/${file%/*}" || return 2
    git -C "$1" show "HEAD:$file" >"$2/head/$file" 2>/dev/null || {
      cause="$file is not in HEAD"
      return 1
    }
    read_files="$read_files $file"
  done
  lists=$(node - "$2/head/$allowlist" "$2/head/$exceptions" "$owner_licences" <<'EOF'
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
const text = (value) => typeof value === "string" && value.trim() !== "";
const named =
  Array.isArray(entries) &&
  entries.every(
    (entry) =>
      entry !== null &&
      ["package", "license", "chain", "reason"].every((field) => text(entry[field])) &&
      !entry.package.includes("*") &&
      Array.isArray(entry.versions) &&
      entry.versions.length > 0 &&
      entry.versions.every(text),
  );
const versions = named ? entries.reduce((sum, entry) => sum + entry.versions.length, 0) : "unread";
console.log(`allowlist=${owners ? "owner" : "other"} entries=${Array.isArray(entries) ? entries.length : "unread"} versions=${versions} named=${named}`);
process.exit(!owners ? 3 : !named ? 4 : entries.length === 0 ? 6 : 0);
EOF
)
  case $? in
    0) ;;
    3)
      cause="$allowlist at HEAD is not the owner's list ($owner_licences)"
      return 1
      ;;
    4)
      cause="an entry of $exceptions at HEAD names no exact package without * with its versions, or lacks its license, chain or reason"
      return 1
      ;;
    6)
      cause="$exceptions at HEAD holds no entry"
      return 1
      ;;
    *)
      lists='allowlist=unread'
      cause="node could not read the lists"
      return 1
      ;;
  esac
  named=$(printf '%s\n' "$lists" | sed -n 's/.* versions=\([0-9][0-9]*\) .*/\1/p')
  if [ "$named" != "$d28" ]; then
    cause="$exceptions at HEAD names ${named:-no} versions, not the $d28 of the owner's item D28"
    return 1
  fi
  return 0
}

find_pnpm() {
  pnpm_path=
  on_path=$(command -v pnpm 2>/dev/null) || on_path=
  for candidate in "$on_path" "${HOME:-}/.local/bin/pnpm" "${HOME:-}/.local/pnpm"; do
    [ -n "$candidate" ] && [ -x "$candidate" ] || continue
    version=$("$candidate" --version 2>/dev/null) || continue
    if [ "$version" = "$1" ]; then
      pnpm_path=$candidate
      return 0
    fi
  done
  return 1
}

listing_in() {
  listing='listed=unread read=unread'
  pinned=$(sed -n 's/.*"packageManager": "pnpm@\([^+"]*\).*/\1/p' "$4")
  if ! find_pnpm "${pinned:-none}"; then
    cause="no pnpm ${pinned:-of a version that backend/package.json pins} on the path or under the user's .local, as the prerequisites leave it"
    return 1
  fi
  (cd "$1" && "$pnpm_path" list --lockfile-only --json --depth Infinity) >"$3/listing.json" 2>"$3/listing.err"
  code=$?
  if [ "$code" != 0 ]; then
    cause="pnpm list --lockfile-only exited $code"
    return 1
  fi
  listing=$(node --input-type=module - "$3/listing.json" "$1/pnpm-lock.yaml" "$2" "$3/missing.txt" <<'EOF'
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
const [listingFile, lockFile, readerFile, missingFile] = process.argv.slice(2);
const listed = new Set();
try {
  const walk = (deps) => {
    for (const [name, dep] of Object.entries(deps ?? {})) {
      if (typeof dep?.version === "string") listed.add(`${name}@${dep.version}`);
      walk(dep?.dependencies);
      walk(dep?.optionalDependencies);
    }
  };
  for (const project of JSON.parse(readFileSync(listingFile, "utf8"))) {
    walk(project.dependencies);
    walk(project.devDependencies);
    walk(project.optionalDependencies);
  }
} catch {
  console.log("listed=unreadable read=unread");
  process.exit(6);
}
if (listed.size === 0) {
  console.log("listed=0 read=unread");
  process.exit(6);
}
const { packagesOfEveryLockfileDocument } = await import(pathToFileURL(readerFile).href);
let read;
try {
  read = new Set(packagesOfEveryLockfileDocument(readFileSync(lockFile, "utf8")).map((entry) => entry.id));
} catch (error) {
  writeFileSync(missingFile, error.message);
  console.log(`listed=${listed.size} read=refused`);
  process.exit(7);
}
const missing = [...listed].filter((id) => !read.has(id)).sort();
writeFileSync(missingFile, missing.join(", "));
console.log(`listed=${listed.size} read=${read.size}`);
process.exit(missing.length > 0 ? 5 : 0);
EOF
)
  case $? in
    0) return 0 ;;
    5) cause="pnpm lists packages of the lock that the gate's reader does not read: $(cat "$3/missing.txt")" ;;
    6) cause="the listing of pnpm is empty or cannot be read" ;;
    7) cause="the gate's reader refuses the lock: $(cat "$3/missing.txt")" ;;
    *)
      listing='listed=unread read=unread'
      cause="node could not compare the listing with the gate's reader"
      ;;
  esac
  return 1
}

check_in() {
  at=$1
  out=$2
  state=$3
  cause=
  read_files=
  listing='listed=unread read=unread'
  if read_lists "$at" "$state"; then
    listing_in "$state/head/backend" "$at/backend/gate/lib/lockfile.mjs" "$state" "$state/head/backend/package.json"
  fi
  mkdir -p "$state/gate-state" || return 2
  (cd "$at" && HALVO_ROOT="$at/backend" GATE_STATE="$state/gate-state" sh backend/gate/checks/85-licenses.sh) >"$out/check85.txt" 2>&1
  code=$?
  line=$(sed -n '$p' "$out/check85.txt")
  excepted=$(printf '%s\n' "$line" | sed -n 's/^PASS licenses: packages=[0-9][0-9]* exceptions=\([0-9][0-9]*\) bundled=[0-9][0-9]* cached=[0-9][0-9]*$/\1/p')
  changed=0
  git -C "$at" diff --quiet HEAD -- || changed=1
  differ=0
  for file in "$allowlist" "$exceptions" "$lock"; do
    case "$read_files " in
      *" $file "*) cmp -s "$state/head/$file" "$at/$file" || differ=1 ;;
    esac
  done
  counts="$lists $listing check85-exit=$code exceptions=${excepted:-none} tracked-changed=$changed lists-differ=$differ"
  [ "$differ" = 0 ] || cause="the work tree's two lists or lock differ from HEAD's bytes after check 85"
  [ "$changed" = 0 ] || cause="a tracked file differs from HEAD after check 85"
  [ -n "$cause" ] || [ "$code" = 0 ] || cause="check 85 exited $code"
  [ -n "$cause" ] || [ "$excepted" = "$d28" ] || cause="the last line of check 85 is not its PASS line with ${excepted:-no} count but $d28 packages outside the owner's list, the count of the owner's item D28"
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
  expected=$(node --input-type=module - "$top/$fixtures/good.json" "$file" "$top/$exceptions" "$work/$case_name.json" "$top/backend/gate/lib/lockfile.mjs" <<'EOF'
import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
const [goodFile, file, exceptFile, outFile, readerFile] = process.argv.slice(2);
const { licenseOf } = await import(pathToFileURL(readerFile).href);
const good = JSON.parse(readFileSync(goodFile, "utf8"));
const one = JSON.parse(readFileSync(file, "utf8"));
const gone = new Set(one.replaces ?? []);
const planted = (one.packages ?? []).map((entry) => (entry.manifest ? { id: entry.id, license: licenseOf(entry.manifest) ?? "NONE", cached: entry.cached } : entry));
const packages = file === goodFile ? good.packages : [...good.packages.filter((entry) => !gone.has(entry.id)), ...planted];
const listed = JSON.parse(readFileSync(exceptFile, "utf8"));
const exceptions = [...(one.exceptions ?? listed), ...(one.extraExceptions ?? [])];
writeFileSync(outFile, JSON.stringify({ packages, bundled: good.bundled ?? [], exceptions }));
console.log(one.expect);
EOF
)
  ran=$((ran + 1))
  if [ -z "$expected" ]; then
    verdict "$want" "$case_name" unreadable
    return
  fi
  (cd "$top/backend" && node gate/lib/licenses.mjs --fixture "$work/$case_name.json") >"$work/$case_name.out" 2>&1
  code=$?
  last=$(sed -n '$p' "$work/$case_name.out")
  case $code:$last in
    "0:$expected") got=PASS ;;
    "1:$expected") got=FAIL ;;
    *) got="exit-$code-with-another-line" ;;
  esac
  verdict "$want" "$case_name" "$got"
}

reader_case() {
  want=$1
  case_name=$2
  kind=$3
  mkdir -p "$work/$case_name/.tools" || exit 2
  printf '%s' "$4" >"$work/$case_name/input" || exit 2
  node --input-type=module - "$kind" "$work/$case_name" "$top/backend/gate/lib/lockfile.mjs" >"$work/$case_name/out" 2>&1 <<'EOF'
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
const [kind, dir, readerFile] = process.argv.slice(2);
const { cachedLicenses, licenseOf, packagesOfEveryLockfileDocument } = await import(pathToFileURL(readerFile).href);
const input = readFileSync(join(dir, "input"), "utf8");
if (kind === "lock") {
  let read;
  try {
    read = packagesOfEveryLockfileDocument(input);
  } catch (error) {
    console.log(`refused: ${error.message}`);
    process.exit(1);
  }
  console.log(`read ${read.map((entry) => entry.id).join(" ")}`);
  process.exit(read.length === 2 ? 0 : 3);
}
writeFileSync(join(dir, ".tools/licenses.json"), input);
const packages = [{ id: "cached@1.0.0", integrity: "sha512-cached" }];
const { licenses, untied } = cachedLicenses(dir, packages);
console.log(`cached ${[...licenses.keys()].join(" ")} untied ${untied.join(" ")} list ${licenseOf({ licenses: [{ type: "MIT" }, { type: "ISC" }] })}`);
process.exit(untied.length > 0 ? 1 : licenses.size === 1 ? 0 : 3);
EOF
  case $? in
    0) got=PASS ;;
    1) got=FAIL ;;
    *) got=other ;;
  esac
  verdict "$want" "$case_name" "$got"
}

stub_repo() {
  dir=$work/$1
  mkdir -p "$dir/backend/gate/checks" "$dir/backend/gate/lib" "$dir/bin" "$dir/state" "$dir/out" "$dir/work" "$dir/tmp" || exit 2
  cat >"$dir/backend/gate/checks/85-licenses.sh" <<'EOF'
#!/bin/sh
[ -n "${HALVO_ROOT:-}" ] || { echo "HALVO_ROOT is not set"; exit 0; }
cat "$STUB_STATE/lines"
[ ! -f "$STUB_STATE/write" ] || printf '\n' >>"$HALVO_ROOT/$(cat "$STUB_STATE/write")"
exit "$(cat "$STUB_STATE/exit")"
EOF
  cat >"$dir/bin/pnpm" <<'EOF'
#!/bin/sh
case ${1:-} in
  --version) cat "$STUB_STATE/pnpm-version" ;;
  list)
    cat "$STUB_STATE/listing.json"
    exit "$(cat "$STUB_STATE/listing.exit")"
    ;;
  *) exit 9 ;;
esac
EOF
  chmod +x "$dir/bin/pnpm" || exit 2
  cp "$top/backend/gate/lib/lockfile.mjs" "$dir/backend/gate/lib/lockfile.mjs" || exit 2
  printf '["Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$dir/$allowlist"
  versions='"1.0.0"'
  n=1
  while [ "$n" -lt "$d28" ]; do
    versions="$versions, \"1.0.$n\""
    n=$((n + 1))
  done
  printf '[{"package": "a-tool", "versions": [%s], "license": "MPL-2.0", "chain": "a-dev-tool@1.0.0 > a-tool@1.0.0", "reason": "A test tool."}]\n' "$versions" >"$dir/$exceptions"
  printf '{\n  "packageManager": "pnpm@99.0.0+sha512.%s"\n}\n' 00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000 >"$dir/backend/package.json"
  printf 'packages:\n  - .\n' >"$dir/backend/pnpm-workspace.yaml"
  printf "lockfileVersion: '9.0'\n\npackages:\n\n  a-tool@1.0.0:\n    resolution: {integrity: sha512-a}\n\nsnapshots:\n\n  a-tool@1.0.0: {}\n" >"$dir/$lock"
  printf 'a tracked file outside backend/gate\n' >"$dir/notes.txt"
  printf 'PASS licenses: packages=231 exceptions=29 bundled=1 cached=121\n' >"$dir/state/lines"
  printf '0\n' >"$dir/state/exit"
  printf '99.0.0\n' >"$dir/state/pnpm-version"
  printf '[{"name": "stub", "devDependencies": {"a-tool": {"version": "1.0.0"}}}]\n' >"$dir/state/listing.json"
  printf '0\n' >"$dir/state/listing.exit"
}

commit_stub() {
  git -C "$1" init -q || exit 2
  git -C "$1" add -A backend notes.txt || exit 2
  git -C "$1" -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m "stub" >/dev/null || exit 2
}

stub_case() {
  want=$1
  case_name=$2
  expect_cause=$3
  dir=$work/$case_name
  saved_path=$PATH
  PATH=$dir/bin:$PATH
  STUB_STATE=$dir/state
  export STUB_STATE
  check_in "$dir" "$dir/out" "$dir/work" >/dev/null 2>&1
  code=$?
  PATH=$saved_path
  found=$cause
  case $code:$found in
    0:) got=PASS ;;
    1:*"$expect_cause"*) got=FAIL ;;
    1:*) got=FAIL-for-another-cause ;;
    *) got=exit-$code ;;
  esac
  verdict "$want" "$case_name" "$got"
}

measure_case() {
  want=$1
  case_name=$2
  expect_last=$3
  dir=$work/$case_name
  env STUB_STATE="$dir/state" PATH="$dir/bin:$PATH" CI_D_REPO="$dir" CI_D_ARTIFACTS="$dir/out" TMPDIR="$dir/tmp" \
    sh "$top/$self" >"$dir/measure.out" 2>&1
  code=$?
  last=$(sed -n '$p' "$dir/measure.out")
  left=$(ls -A "$dir/tmp")
  case $code:$last:$left in
    "0:PASS $name:") got=PASS ;;
    "1:FAIL $name ("*"$expect_last"*")":) got=FAIL ;;
    *) got="exit-$code-with-another-line-or-tmpdir-not-empty" ;;
  esac
  verdict "$want" "$case_name" "$got"
}

self_test() {
  top=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  ran=0
  wrong=
  if [ ! -f "$top/$fixtures/good.json" ]; then
    printf 'FAIL self-test:%s (%s/good.json is missing)\n' "$name" "$fixtures"
    exit 1
  fi
  for file in "$top/$fixtures"/*.json; do
    lists_fixture "$file"
  done
  listed_at_head=$(git -C "$top" ls-tree --name-only HEAD "$fixtures/" | grep -c '\.json$')
  if [ "$ran" != "$listed_at_head" ]; then
    verdict PASS fixtures-that-ran-are-the-fixtures-that-head-lists "ran-$ran-of-$listed_at_head"
  fi

  reader_case PASS reader-of-a-lock-of-two-packages lock "$(printf "packages:\n\n  a@1.0.0:\n    resolution: {integrity: sha512-a}\n\n  b@1.0.0:\n    resolution: {integrity: sha512-b}\n")"
  reader_case FAIL reader-of-a-key-with-a-trailing-space lock "$(printf "packages:\n\n  a@1.0.0:\n    resolution: {integrity: sha512-a}\n\n  b@1.0.0: \n    resolution: {integrity: sha512-b}\n\n  c@1.0.0:\n    resolution: {integrity: sha512-c}\n")"
  reader_case FAIL reader-of-an-entry-on-one-line lock "$(printf "packages:\n\n  a@1.0.0:\n    resolution: {integrity: sha512-a}\n\n  b@1.0.0: {resolution: {integrity: sha512-b}}\n\n  c@1.0.0:\n    resolution: {integrity: sha512-c}\n")"
  reader_case FAIL reader-of-a-packages-key-in-another-form lock "$(printf "packages: \n\n  a@1.0.0:\n    resolution: {integrity: sha512-a}\n\n  b@1.0.0:\n    resolution: {integrity: sha512-b}\n")"
  reader_case PASS cache-of-this-reading cache '{"cached@1.0.0": {"license": "MIT", "licenseList": "AND", "integrity": "sha512-cached"}}'
  reader_case FAIL cache-of-another-reading cache '{"cached@1.0.0": {"license": "MIT", "integrity": "sha512-cached"}}'

  real=$work/real-pnpm-on-a-lock-with-a-key-with-a-trailing-space
  mkdir -p "$real" || exit 2
  cause=
  listing_in "$top/$fixtures/lock-key-with-trailing-space" "$top/backend/gate/lib/lockfile.mjs" "$real" "$top/backend/package.json" >/dev/null 2>&1
  case $?:$cause in
    *'"  zod@4.6.7: "'*) got=FAIL ;;
    0:*) got=PASS ;;
    *) got=FAIL-for-another-cause ;;
  esac
  verdict FAIL real-pnpm-on-a-lock-with-a-key-with-a-trailing-space "$got"

  stub_repo good
  commit_stub "$work/good"
  stub_case PASS good ''

  stub_repo count-of-the-old-lists
  commit_stub "$work/count-of-the-old-lists"
  printf 'PASS licenses: packages=231 exceptions=26 bundled=1 cached=121\n' >"$work/count-of-the-old-lists/state/lines"
  stub_case FAIL count-of-the-old-lists 'is not its PASS line'

  stub_repo count-with-a-new-version-of-a-named-package
  commit_stub "$work/count-with-a-new-version-of-a-named-package"
  printf 'PASS licenses: packages=232 exceptions=30 bundled=1 cached=121\n' >"$work/count-with-a-new-version-of-a-named-package/state/lines"
  stub_case FAIL count-with-a-new-version-of-a-named-package 'is not its PASS line'

  stub_repo count-with-a-digit-more
  commit_stub "$work/count-with-a-digit-more"
  printf 'PASS licenses: packages=231 exceptions=290 bundled=1 cached=121\n' >"$work/count-with-a-digit-more/state/lines"
  stub_case FAIL count-with-a-digit-more 'is not its PASS line'

  stub_repo pass-line-and-exit-1
  commit_stub "$work/pass-line-and-exit-1"
  printf '1\n' >"$work/pass-line-and-exit-1/state/exit"
  stub_case FAIL pass-line-and-exit-1 'check 85 exited 1'

  stub_repo fail-line-and-exit-0
  commit_stub "$work/fail-line-and-exit-0"
  printf 'FAIL licenses: 1 problems; first: a@1.0.0 has the license 0BSD, which is neither on the allowlist nor in the exceptions\n' >"$work/fail-line-and-exit-0/state/lines"
  stub_case FAIL fail-line-and-exit-0 'is not its PASS line'

  stub_repo pass-line-not-last
  commit_stub "$work/pass-line-not-last"
  printf 'PASS licenses: packages=231 exceptions=29 bundled=1 cached=121\nanother line\n' >"$work/pass-line-not-last/state/lines"
  stub_case FAIL pass-line-not-last 'is not its PASS line'

  stub_repo pass-line-with-more-after-it
  commit_stub "$work/pass-line-with-more-after-it"
  printf 'PASS licenses: packages=231 exceptions=29 bundled=1 cached=121 exceptions=26\n' >"$work/pass-line-with-more-after-it/state/lines"
  stub_case FAIL pass-line-with-more-after-it 'is not its PASS line'

  stub_repo allowlist-staged-and-differs-from-head
  commit_stub "$work/allowlist-staged-and-differs-from-head"
  printf '["0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$work/allowlist-staged-and-differs-from-head/$allowlist"
  git -C "$work/allowlist-staged-and-differs-from-head" add "$allowlist" || exit 2
  stub_case FAIL allowlist-staged-and-differs-from-head 'a tracked file differs from HEAD'

  stub_repo allowlist-with-0bsd
  printf '["0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$work/allowlist-with-0bsd/$allowlist"
  commit_stub "$work/allowlist-with-0bsd"
  stub_case FAIL allowlist-with-0bsd "is not the owner's list"

  stub_repo allowlist-without-unicode
  printf '["Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Zlib"]\n' >"$work/allowlist-without-unicode/$allowlist"
  commit_stub "$work/allowlist-without-unicode"
  stub_case FAIL allowlist-without-unicode "is not the owner's list"

  stub_repo allowlist-of-seven-with-cc0-for-zlib
  printf '["Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "CC0-1.0", "ISC", "MIT", "Unicode-3.0"]\n' >"$work/allowlist-of-seven-with-cc0-for-zlib/$allowlist"
  commit_stub "$work/allowlist-of-seven-with-cc0-for-zlib"
  stub_case FAIL allowlist-of-seven-with-cc0-for-zlib "is not the owner's list"

  stub_repo entry-without-chain
  sed 's/"chain": "[^"]*", //' "$work/entry-without-chain/$exceptions" >"$work/entry-without-chain/state/exceptions" || exit 2
  cp "$work/entry-without-chain/state/exceptions" "$work/entry-without-chain/$exceptions" || exit 2
  commit_stub "$work/entry-without-chain"
  stub_case FAIL entry-without-chain 'names no exact package'

  stub_repo entry-with-a-blank-reason
  sed 's/"reason": "[^"]*"/"reason": " "/' "$work/entry-with-a-blank-reason/$exceptions" >"$work/entry-with-a-blank-reason/state/exceptions" || exit 2
  cp "$work/entry-with-a-blank-reason/state/exceptions" "$work/entry-with-a-blank-reason/$exceptions" || exit 2
  commit_stub "$work/entry-with-a-blank-reason"
  stub_case FAIL entry-with-a-blank-reason 'names no exact package'

  stub_repo entry-with-a-star
  sed 's/"package": "a-tool"/"package": "a-tool-*"/' "$work/entry-with-a-star/$exceptions" >"$work/entry-with-a-star/state/exceptions" || exit 2
  cp "$work/entry-with-a-star/state/exceptions" "$work/entry-with-a-star/$exceptions" || exit 2
  commit_stub "$work/entry-with-a-star"
  stub_case FAIL entry-with-a-star 'names no exact package'

  stub_repo entry-without-versions
  sed 's/"versions": \[[^]]*\], //' "$work/entry-without-versions/$exceptions" >"$work/entry-without-versions/state/exceptions" || exit 2
  cp "$work/entry-without-versions/state/exceptions" "$work/entry-without-versions/$exceptions" || exit 2
  commit_stub "$work/entry-without-versions"
  stub_case FAIL entry-without-versions 'names no exact package'

  stub_repo entry-with-an-empty-version
  sed 's/"versions": \["1.0.0", /"versions": ["", /' "$work/entry-with-an-empty-version/$exceptions" >"$work/entry-with-an-empty-version/state/exceptions" || exit 2
  cp "$work/entry-with-an-empty-version/state/exceptions" "$work/entry-with-an-empty-version/$exceptions" || exit 2
  commit_stub "$work/entry-with-an-empty-version"
  stub_case FAIL entry-with-an-empty-version 'names no exact package'

  stub_repo versions-one-short-of-d28
  sed 's/"versions": \["1.0.0", /"versions": [/' "$work/versions-one-short-of-d28/$exceptions" >"$work/versions-one-short-of-d28/state/exceptions" || exit 2
  cp "$work/versions-one-short-of-d28/state/exceptions" "$work/versions-one-short-of-d28/$exceptions" || exit 2
  commit_stub "$work/versions-one-short-of-d28"
  stub_case FAIL versions-one-short-of-d28 "versions, not the $d28 of the owner's item D28"

  stub_repo exceptions-an-empty-list
  printf '[]\n' >"$work/exceptions-an-empty-list/$exceptions"
  commit_stub "$work/exceptions-an-empty-list"
  stub_case FAIL exceptions-an-empty-list 'holds no entry'

  stub_repo check-writes-a-tracked-file
  commit_stub "$work/check-writes-a-tracked-file"
  printf 'gate/license-exceptions.json\n' >"$work/check-writes-a-tracked-file/state/write"
  stub_case FAIL check-writes-a-tracked-file 'a tracked file differs from HEAD'

  stub_repo allowlist-of-the-work-tree-differs-from-head
  commit_stub "$work/allowlist-of-the-work-tree-differs-from-head"
  printf '["0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$work/allowlist-of-the-work-tree-differs-from-head/$allowlist"
  stub_case FAIL allowlist-of-the-work-tree-differs-from-head 'a tracked file differs from HEAD'

  stub_repo allowlist-skip-worktree-and-another-list-in-the-work-tree
  commit_stub "$work/allowlist-skip-worktree-and-another-list-in-the-work-tree"
  git -C "$work/allowlist-skip-worktree-and-another-list-in-the-work-tree" update-index --skip-worktree "$allowlist" || exit 2
  printf '["GPL-3.0-only", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$work/allowlist-skip-worktree-and-another-list-in-the-work-tree/$allowlist"
  stub_case FAIL allowlist-skip-worktree-and-another-list-in-the-work-tree "differ from HEAD's bytes"

  stub_repo lists-not-in-head
  mv "$work/lists-not-in-head/$allowlist" "$work/lists-not-in-head/state/allowlist.json" || exit 2
  commit_stub "$work/lists-not-in-head"
  stub_case FAIL lists-not-in-head "$allowlist is not in HEAD"

  stub_repo allowlist-not-in-head-and-untracked-in-the-work-tree
  mv "$work/allowlist-not-in-head-and-untracked-in-the-work-tree/$allowlist" "$work/allowlist-not-in-head-and-untracked-in-the-work-tree/state/allowlist.json" || exit 2
  commit_stub "$work/allowlist-not-in-head-and-untracked-in-the-work-tree"
  cp "$work/allowlist-not-in-head-and-untracked-in-the-work-tree/state/allowlist.json" "$work/allowlist-not-in-head-and-untracked-in-the-work-tree/$allowlist" || exit 2
  stub_case FAIL allowlist-not-in-head-and-untracked-in-the-work-tree "$allowlist is not in HEAD"

  stub_repo listing-names-a-package-that-the-lock-lacks
  commit_stub "$work/listing-names-a-package-that-the-lock-lacks"
  printf '[{"name": "stub", "devDependencies": {"a-tool": {"version": "1.0.0"}, "b-tool": {"version": "2.0.0"}}}]\n' >"$work/listing-names-a-package-that-the-lock-lacks/state/listing.json"
  stub_case FAIL listing-names-a-package-that-the-lock-lacks "the gate's reader does not read: b-tool@2.0.0"

  stub_repo listing-fails
  commit_stub "$work/listing-fails"
  printf '1\n' >"$work/listing-fails/state/listing.exit"
  stub_case FAIL listing-fails 'pnpm list --lockfile-only exited 1'

  stub_repo listing-empty
  commit_stub "$work/listing-empty"
  printf '[{"name": "stub"}]\n' >"$work/listing-empty/state/listing.json"
  stub_case FAIL listing-empty 'empty or cannot be read'

  stub_repo listing-unreadable
  commit_stub "$work/listing-unreadable"
  printf 'not json\n' >"$work/listing-unreadable/state/listing.json"
  stub_case FAIL listing-unreadable 'empty or cannot be read'

  stub_repo lock-that-the-reader-refuses
  printf "lockfileVersion: '9.0'\n\npackages:\n\n  a-tool@1.0.0: \n    resolution: {integrity: sha512-a}\n" >"$work/lock-that-the-reader-refuses/$lock"
  commit_stub "$work/lock-that-the-reader-refuses"
  stub_case FAIL lock-that-the-reader-refuses "the gate's reader refuses the lock"

  stub_repo no-pnpm-of-the-pinned-version
  commit_stub "$work/no-pnpm-of-the-pinned-version"
  printf '98.0.0\n' >"$work/no-pnpm-of-the-pinned-version/state/pnpm-version"
  stub_case FAIL no-pnpm-of-the-pinned-version 'no pnpm 99.0.0'

  stub_repo measure-passes
  commit_stub "$work/measure-passes"
  measure_case PASS measure-passes ''

  stub_repo measure-of-a-failing-check-85
  commit_stub "$work/measure-of-a-failing-check-85"
  printf 'FAIL licenses: 1 problems; first: a@1.0.0 has the license GPL-3.0-only, which is neither on the allowlist nor in the exceptions\n' >"$work/measure-of-a-failing-check-85/state/lines"
  printf '1\n' >"$work/measure-of-a-failing-check-85/state/exit"
  measure_case FAIL measure-of-a-failing-check-85 'check 85 exited 1'

  stub_repo measure-of-0bsd-at-head-while-check-85-passes
  printf '["0BSD", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "MIT", "Unicode-3.0", "Zlib"]\n' >"$work/measure-of-0bsd-at-head-while-check-85-passes/$allowlist"
  commit_stub "$work/measure-of-0bsd-at-head-while-check-85-passes"
  measure_case FAIL measure-of-0bsd-at-head-while-check-85-passes "is not the owner's list"

  stub_repo measure-of-a-check-that-writes-a-tracked-file-outside-the-gate
  commit_stub "$work/measure-of-a-check-that-writes-a-tracked-file-outside-the-gate"
  printf '../notes.txt\n' >"$work/measure-of-a-check-that-writes-a-tracked-file-outside-the-gate/state/write"
  measure_case FAIL measure-of-a-check-that-writes-a-tracked-file-outside-the-gate 'a tracked file differs from HEAD'

  stub_repo measure-of-an-allowlist-not-in-head
  mv "$work/measure-of-an-allowlist-not-in-head/$allowlist" "$work/measure-of-an-allowlist-not-in-head/state/allowlist.json" || exit 2
  commit_stub "$work/measure-of-an-allowlist-not-in-head"
  cp "$work/measure-of-an-allowlist-not-in-head/state/allowlist.json" "$work/measure-of-an-allowlist-not-in-head/$allowlist" || exit 2
  measure_case FAIL measure-of-an-allowlist-not-in-head "$allowlist is not in HEAD"

  if [ -z "${R7_25_INNER:-}" ]; then
    inner=$work/self-test-leaves-its-tmpdir-empty
    mkdir -p "$inner/tmp" || exit 2
    env R7_25_INNER=1 TMPDIR="$inner/tmp" sh "$top/$self" --self-test >"$inner/out" 2>&1
    code=$?
    left=$(ls -A "$inner/tmp")
    case $code:$left in
      0:) got=PASS ;;
      *) got="exit-$code-or-tmpdir-not-empty" ;;
    esac
    verdict PASS self-test-leaves-its-tmpdir-empty "$got"
  fi

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
  exit 0
}

case ${1:-} in
  --self-test) self_test ;;
  --help) usage ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
