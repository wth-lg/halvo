#!/bin/sh
# jobs: macos windows
# rule: At HEAD, check 55's memory self-check (node backend/test-kit/fake-meter/rerun/check.mjs --memory-self-check) and its parent-gone self-check (--parent-gone-self-check: a worker of the check ends within PARENT_GONE_SECONDS of its parent) each exit 0 with their PASS line last, every bad fixture of backend/gate/fixtures/ applies to the files of HEAD (each edit's text found exactly once, in its order), and no tracked file differs from HEAD after them.
# lane: alone
set -u
name=27-r7-metering-self
check=test-kit/fake-meter/rerun/check.mjs

apply_program=$(
  cat <<'EOF'
import { spawnSync } from "node:child_process";
import { posix } from "node:path";
const [repo, rev] = process.argv.slice(-2);
const git = (args, input) => spawnSync("git", ["-C", repo, ...args], { input, maxBuffer: 1 << 28 });
const listed = git(["ls-tree", "-r", "--name-only", rev, "--", "backend/gate/fixtures/"]);
if (listed.status !== 0) {
  console.log(`setup: git lists no tree backend/gate/fixtures/ at ${rev}`);
  process.exit(2);
}
function blobs(paths) {
  const found = new Map();
  if (paths.length === 0) return found;
  const run = git(["cat-file", "--batch"], `${paths.map((path) => `${rev}:${path}`).join("\n")}\n`);
  if (run.status !== 0) return null;
  const out = run.stdout;
  let at = 0;
  for (const path of paths) {
    const end = out.indexOf(10, at);
    const head = out.subarray(at, end).toString("utf8");
    at = end + 1;
    if (head.endsWith(" missing")) continue;
    const size = Number(head.split(" ")[2]);
    found.set(path, out.subarray(at, at + size).toString("utf8"));
    at += size + 1;
  }
  return found;
}
const names = listed.stdout.toString("utf8").split("\n").filter((path) => /\/bad-[^/]*\.json$/.test(path));
const fixtures = blobs(names);
if (fixtures === null) {
  console.log("setup: git cat-file cannot read the fixtures");
  process.exit(2);
}
const parsed = [];
const problems = [];
for (const path of names) {
  try {
    const edits = JSON.parse(fixtures.get(path) ?? "").edits;
    if (Array.isArray(edits)) parsed.push({ path, edits });
  } catch {
    problems.push(`${path}: not JSON`);
  }
}
const targets = [...new Set(parsed.flatMap(({ edits }) => edits.map((edit) => posix.normalize(posix.join("backend", String(edit.file))))))];
const files = blobs(targets);
if (files === null) {
  console.log("setup: git cat-file cannot read the files that the fixtures edit");
  process.exit(2);
}
let edits = 0;
let applied = 0;
for (const { path, edits: list } of parsed) {
  const texts = new Map();
  let done = 0;
  for (const edit of list) {
    edits += 1;
    const file = posix.normalize(posix.join("backend", String(edit.file)));
    const text = texts.get(file) ?? files.get(file);
    if (text === undefined) {
      problems.push(`${path}: edit ${done + 1}: HEAD has no file ${file}`);
      break;
    }
    const at = text.indexOf(edit.find);
    if (typeof edit.find !== "string" || at < 0 || text.indexOf(edit.find, at + 1) >= 0) {
      problems.push(`${path}: edit ${done + 1}: its text is not in ${file} exactly once`);
      break;
    }
    texts.set(file, `${text.slice(0, at)}${edit.replace}${text.slice(at + edit.find.length)}`);
    done += 1;
  }
  applied += done === list.length ? 1 : 0;
  console.log(`fixture ${path} edits=${list.length} applied=${done}`);
}
for (const problem of problems) console.log(`problem: ${problem}`);
console.log(`apply: bad-fixtures=${names.length} with-edits=${parsed.length} edits=${edits} applied=${applied} problems=${problems.length}`);
process.exit(problems.length === 0 ? 0 : 1);
EOF
)

shim_program=$(
  cat <<'EOF'
import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
const [copy, ...packages] = process.argv.slice(1);
for (const name of packages) {
  const url = import.meta.resolve(name);
  const namespace = await import(url);
  const dir = join(copy, "node_modules", name);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, "package.json"), `${JSON.stringify({ name, type: "module", exports: "./index.js" })}\n`);
  writeFileSync(join(dir, "index.js"), `export * from ${JSON.stringify(url)};\n${"default" in namespace ? `export { default } from ${JSON.stringify(url)};\n` : ""}`);
}
EOF
)

first_cause() {
  [ -n "$cause" ] || cause=$1
}

part() {
  part_label=$1
  part_out=$3
  (cd "$2" && shift 3 && node "$check" "$@") >"$part_out" 2>&1
  part_code=$?
  part_last=$(awk 'END { print }' "$part_out")
  case $part_code:$part_last in
    "0:PASS metering-$part_label: "*)
      printf '%s\n' "$part_last"
      return 0
      ;;
  esac
  printf '%s\n' "${part_last:-no line}"
  first_cause "the $part_label self-check of check 55 exited $part_code with the last line \"$part_last\""
  return 1
}

measure_in() {
  repo=$1
  backend=$2
  out=$3
  parts=$4
  cause=
  (cd "$repo" && node --input-type=module -e "$apply_program" "$repo" HEAD) >"$out/apply.txt" 2>&1
  apply=$?
  sed -n -e 's/^\(apply: .*\)$/\1/p' -e 's/^\(problem: .*\)$/\1/p' -e 's/^\(setup: .*\)$/\1/p' "$out/apply.txt"
  case $apply in
    0) ;;
    1) first_cause "a bad fixture does not apply to HEAD; the list is apply.txt among the artifacts" ;;
    *) first_cause "the apply step could not read HEAD" ;;
  esac
  case " $parts " in
    *" memory "*) part memory "$backend" "$out/memory.txt" --memory-self-check ;;
  esac
  case " $parts " in
    *" parent-gone "*) part parent-gone "$backend" "$out/parent-gone.txt" --parent-gone-self-check ;;
  esac
  case " $parts " in
    *" stamp "*)
      (cd "$repo" && sh scripts/r7/stamp-gate-records.sh --self-test) >"$out/stamp.txt" 2>&1
      stamp_code=$?
      stamp_last=$(awk 'END { print }' "$out/stamp.txt")
      printf '%s\n' "${stamp_last:-no line}"
      case $stamp_code:$stamp_last in
        "0:PASS self-test:stamp-gate-records "*) ;;
        *) first_cause "the self-test of scripts/r7/stamp-gate-records.sh exited $stamp_code with the last line \"$stamp_last\"" ;;
      esac
      ;;
  esac
  changed=$(git -C "$repo" status --porcelain --untracked-files=no | awk 'END { print NR }')
  [ "$changed" = 0 ] || first_cause "$changed tracked files differ from HEAD after the checks"
  printf 'tracked-files-changed=%s\n' "$changed"
  [ -z "$cause" ]
}

measure() {
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || exit 2
  before=$(git -C "$repo" status --porcelain --untracked-files=no | awk 'END { print NR }')
  if [ "$before" != 0 ]; then
    printf 'FAIL %s (%s tracked files differ from HEAD before the checks; commit them first)\n' "$name" "$before"
    exit 1
  fi
  if measure_in "$repo" "$repo/backend" "$artifacts" "memory parent-gone"; then
    printf 'PASS %s\n' "$name"
  else
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
}

copy_of_head() {
  copy=$scratch/$1
  mkdir -p "$copy" || exit 2
  git -C "$repo" archive -o "$scratch/$1.tar" HEAD backend || exit 2
  tar -xf "$scratch/$1.tar" -C "$copy" || exit 2
  (cd "$repo/backend" && node --input-type=module -e "$shim_program" "$copy/backend" fast-check zod) || exit 2
}

edit_copy() {
  file=$scratch/$1/backend/$2
  node -e '
    const fs = require("node:fs");
    const [file, find, replace] = process.argv.slice(1);
    const text = fs.readFileSync(file, "utf8");
    const at = text.indexOf(find);
    if (at < 0 || text.indexOf(find, at + 1) >= 0) process.exit(3);
    fs.writeFileSync(file, text.slice(0, at) + replace + text.slice(at + find.length));
  ' "$file" "$3" "$4" || {
    printf 'FAIL self-test:%s (the edit of the case %s does not apply to %s at HEAD)\n' "$name" "$1" "$2"
    exit 1
  }
}

expect() {
  want=$1
  case_name=$2
  case_repo=$3
  case_backend=$4
  parts=$5
  out=$scratch/$case_name.out
  mkdir -p "$out" || exit 2
  if measure_in "$case_repo" "$case_backend" "$out" "$parts" >"$out/lines.txt" 2>&1; then got=PASS; else got=FAIL; fi
  printf 'case %s/%s want=%s got=%s%s\n' "$name" "$case_name" "$want" "$got" "${cause:+ ($cause)}"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

fixture_repo() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/gate/fixtures/metering" "$dir/backend/src" || exit 2
  git -C "$dir" init -q || exit 2
  printf 'const one = 1;\nconst two = 2;\nconst two_again = 2;\n' >"$dir/backend/src/x.ts"
  printf '{ "line": ["FAIL metering: "], "edits": [{ "file": "src/x.ts", "find": "%s", "replace": "const one = 3;" }] }\n' "$2" >"$dir/backend/gate/fixtures/metering/bad-one.json"
  printf '{ "line": ["PASS metering: "] }\n' >"$dir/backend/gate/fixtures/metering/good-one.json"
  printf '%s\n' "$3" >"$dir/backend/gate/fixtures/metering/bad-two.json"
  git -C "$dir" add -A . || exit 2
  git -C "$dir" -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m 'A backend with gate fixtures' >/dev/null || exit 2
}

stand_in_check() {
  dir=$scratch/$1
  mkdir -p "$dir/backend/test-kit/fake-meter/rerun" || exit 2
  printf 'import { appendFileSync } from "node:fs";\nif (process.argv.includes("--memory-self-check")) {\n  appendFileSync("src/x.ts", "const changed = 1;\\n");\n  console.log("PASS metering-memory: stand-in");\n}\n' >"$dir/backend/$check"
}

self_test() {
  { git var GIT_AUTHOR_IDENT && git var GIT_COMMITTER_IDENT; } >/dev/null 2>&1 || {
    printf 'FAIL self-test:%s (the self-test and the self-test of scripts/r7/stamp-gate-records.sh commit in throwaway repositories, and git has no name and email for a commit here. Configure a git user, or set the variables GIT_AUTHOR_NAME, GIT_AUTHOR_EMAIL, GIT_COMMITTER_NAME and GIT_COMMITTER_EMAIL.)\n' "$name"
    exit 1
  }
  repo=${CI_D_REPO:-$(git rev-parse --show-toplevel)} || exit 2
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  copy_of_head head-copy
  expect PASS head-copy "$repo" "$scratch/head-copy/backend" "memory parent-gone stamp"

  copy_of_head unmended-statement-cache
  edit_copy unmended-statement-cache test-kit/fake-meter/rerun/worker.mjs '  prepared.delete(this);
' ''
  expect FAIL unmended-statement-cache "$repo" "$scratch/unmended-statement-cache/backend" memory

  copy_of_head no-watchdog
  edit_copy no-watchdog test-kit/fake-meter/rerun/worker.mjs 'new Worker(WATCHDOG, { eval: true, workerData: { parent: process.ppid, pollMs: PARENT_POLL_MS } }).unref();' ''
  if command -v cygpath >/dev/null 2>&1; then
    expect PASS no-watchdog-the-system-ends-the-child-on-windows "$repo" "$scratch/no-watchdog/backend" parent-gone
  else
    expect FAIL no-watchdog "$repo" "$scratch/no-watchdog/backend" parent-gone
  fi

  fixture_repo applies 'const one = 1;' '{ "line": ["FAIL metering: "], "edits": [{ "file": "src/x.ts", "find": "const one = 1;\nconst two = 2;", "replace": "const one = 1;\nconst two = 5;" }, { "file": "src/x.ts", "find": "const two = 5;", "replace": "const two = 6;" }] }'
  expect PASS applies "$scratch/applies" "$scratch/applies/backend" ""

  fixture_repo text-not-at-head 'const three = 3;' '{ "edits": [] }'
  expect FAIL text-not-at-head "$scratch/text-not-at-head" "$scratch/text-not-at-head/backend" ""

  fixture_repo text-twice-at-head 'const two' '{ "edits": [] }'
  expect FAIL text-twice-at-head "$scratch/text-twice-at-head" "$scratch/text-twice-at-head/backend" ""

  fixture_repo second-edit-reads-the-first 'const one = 1;' '{ "edits": [{ "file": "src/x.ts", "find": "const one = 1;", "replace": "const one = 4;" }, { "file": "src/x.ts", "find": "const one = 1;", "replace": "const one = 5;" }] }'
  expect FAIL second-edit-reads-the-first "$scratch/second-edit-reads-the-first" "$scratch/second-edit-reads-the-first/backend" ""

  fixture_repo file-not-at-head 'const one = 1;' '{ "edits": [{ "file": "src/y.ts", "find": "x", "replace": "y" }] }'
  expect FAIL file-not-at-head "$scratch/file-not-at-head" "$scratch/file-not-at-head/backend" ""

  fixture_repo stamp-self-test-fails 'const one = 1;' '{ "edits": [] }'
  mkdir -p "$scratch/stamp-self-test-fails/scripts/r7" || exit 2
  printf 'printf "FAIL self-test:stamp-gate-records (wrong verdict in: memory-self-check-fails)\\n"\nexit 1\n' >"$scratch/stamp-self-test-fails/scripts/r7/stamp-gate-records.sh"
  git -C "$scratch/stamp-self-test-fails" add -A . || exit 2
  git -C "$scratch/stamp-self-test-fails" -c commit.gpgsign=false -c core.hooksPath=/dev/null commit -q -m 'A stamp script whose self-test fails' >/dev/null || exit 2
  expect FAIL stamp-self-test-fails "$scratch/stamp-self-test-fails" "$scratch/stamp-self-test-fails/backend" stamp

  fixture_repo check-writes-a-tracked-file 'const one = 1;' '{ "edits": [] }'
  stand_in_check check-writes-a-tracked-file
  expect FAIL check-writes-a-tracked-file "$scratch/check-writes-a-tracked-file" "$scratch/check-writes-a-tracked-file/backend" memory

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) printf 'FAIL %s (usage)\n' "$name"; exit 1 ;;
esac
