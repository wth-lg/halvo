#!/bin/sh
# jobs: macos windows
# rule: No tracked Rust test outside crates/halvo-test-dir names a scratch directory from the process id or the clock alone, read lexically in the forms that --help lists: in test code (a tests/ directory, tests.rs, testing.rs, a module or a file under a cfg with test), a line that makes a directory by create_dir_all, a DirBuilder whose recursive() takes anything but false, a plain DirBuilder create or create_dir (also each under another name by use ... as), also inside a loop that retries it, is a place when a process id or clock read (also under another name by use ... as, in a variable bound from one, or through a test function that returns one) stands beside a path or string build on one line, or such a read and a format! or write! stand in the lines above it in its function (16 at most), and each file's count of places equals its row in known(), whose rows, places and sorted path and count pairs equal the pins beside it: the list may shrink with its pins and may not grow (U2.5).
# lane: free
set -u
LC_ALL=C
export LC_ALL
name=15-r3-test-dirs
nl='
'
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

known() {
  cat <<'EOF'
account/crates/halvo-account/tests/environment.rs 1 account run
account/crates/halvo-account/tests/support/mod.rs 1 account run
account/crates/halvo-secrets/src/mac/tests.rs 1 account run
account/crates/halvo-secrets/tests/ancestors.rs 1 account run
account/crates/halvo-secrets/tests/failed_reads.rs 1 account run
account/crates/halvo-secrets/tests/file_store.rs 1 account run
account/crates/halvo-secrets/tests/hosted.rs 1 account run
account/crates/halvo-secrets/tests/walk.rs 1 account run
account/crates/halvo-secrets/tests/wipe.rs 1 account run
crates/halvo-agent-client/src/marks.rs 1 agent-setup run, U4.3
crates/halvo-cap/src/lock.rs 1 workflow run
crates/halvo-cap/tests/model_lock.rs 1 workflow run
crates/halvo-capture/tests/source_rules.rs 1 capture run, store-4
crates/halvo-coord/tests/fixtures_same.rs 1 interface run
crates/halvo-derived/src/tests.rs 1 workflow run
crates/halvo-derived/tests/common/mod.rs 1 workflow run
crates/halvo-route-gate/tests/cli_commands.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-route-gate/tests/import_golden.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-route-gate/tests/import_rules.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-route-gate/tests/import_write.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-route-gate/tests/out_file.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-route-gate/tests/product_cli.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-route-gate/tests/product_recorded_cli.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-route-gate/tests/sequences_same.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-route-wire/tests/gateway_check.rs 1 routing run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-runner/tests/support/mod.rs 1 agent-setup run, U4.1
crates/halvo-sample/tests/common/mod.rs 1 workflow run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
crates/halvo-web/src/child.rs 1 agent-setup run, U2.6
crates/halvo-web/src/guard.rs 2 agent-setup run, U2.6
src-tauri/src/tests.rs 1 capture run, app-3a
src-tauri/tests/page_guard.rs 1 capture run; exclusive create retried on a pid-and-counter name; kept as it stood on 2026-11-15
tools/r3-trials/profile/src/bin/profile-trial.rs 1 agent-setup run, P0 trials
tools/r3-trials/profile/src/lib.rs 4 agent-setup run, P0 trials
tools/r3-trials/sheet/tests/reader.rs 1 agent-setup run, P0 trials
tools/r3-trials/web/src/bin/web-trial.rs 1 agent-setup run, P0 trials
tools/r3-trials/web/src/cdp.rs 4 agent-setup run, P0 trials
tools/r3-trials/web/src/cdp/session.rs 2 agent-setup run, P0 trials
tools/r3-trials/web/src/checks.rs 1 agent-setup run, P0 trials
tools/r3-trials/web/src/start.rs 2 agent-setup run, P0 trials
tools/r3-trials/web/tests/org.rs 4 agent-setup run, P0 trials
tools/r3-trials/web/tests/signals.rs 2 agent-setup run, P0 trials
tools/r4-trials/app/tests/pages.rs 1 interface run
EOF
}

pinned_rows=42
pinned_places=55
pinned_paths=2166838485

program='
function code(s) {
  sub(/\/\/.*/, "", s)
  return s
}
function word(s, w) {
  return (" " s " ") ~ ("[^A-Za-z0-9_]" w "[^A-Za-z0-9_]")
}
function call(s, w) {
  return index(s, w "(") && s ~ ("(^|[^A-Za-z0-9_])" w "\\(")
}
function cfgtest(s) {
  return s ~ /#!?\[cfg\(/ && s ~ /[(, ]test[),]/ && s !~ /not\([^)]*test/
}
function read(s,   i) {
  if (s ~ /process::id\(|(^|[^A-Za-z0-9_.:])id\(\)|getpid\(|GetCurrentProcessId|SystemTime|UNIX_EPOCH|Instant::now|(nanos|micros|millis)\(\)|Utc::now|Local::now/) return 1
  for (i = 1; i <= nfn; i++)
    if (call(s, fname[i])) return 1
  for (i = 1; i <= nsa; i++)
    if (call(s, salias[i])) return 1
  return 0
}
function source(s,   i) {
  if (read(s)) return 1
  for (i = 1; i <= nvar; i++)
    if (index(s, var[i]) && word(s, var[i])) return 1
  return 0
}
function accepts(s,   i) {
  if (s ~ /create_dir_all\(/) return 1
  if (s ~ /\.recursive\(/ && s !~ /\.recursive\(false\)/) return 1
  for (i = 1; i <= naa; i++)
    if (call(s, aal[i])) return 1
  return 0
}
function refuses(s,   i) {
  if (s ~ /create_dir\(/) return 1
  for (i = 1; i <= nca; i++)
    if (call(s, cal[i])) return 1
  return 0
}
function uses(s,   t, a, b) {
  t = s
  while (match(t, /[A-Za-z_0-9]+[ \t]+as[ \t]+[A-Za-z_0-9]+/)) {
    a = substr(t, RSTART, RLENGTH)
    t = substr(t, RSTART + RLENGTH)
    b = a
    sub(/[ \t]+as[ \t].*/, "", a)
    sub(/.*[ \t]as[ \t]+/, "", b)
    if (a == "create_dir_all") aal[++naa] = b
    else if (a == "create_dir") cal[++nca] = b
    else if (a == "id" || a == "getpid" || a == "GetCurrentProcessId") salias[++nsa] = b
  }
}
function addfn(name,   i) {
  for (i = 1; i <= nfn; i++)
    if (fname[i] == name) return
  fname[++nfn] = name
}
function addmod(t, base,   name, prefix) {
  name = t
  sub(/.*mod[ \t]+/, "", name)
  sub(/[^A-Za-z0-9_].*/, "", name)
  prefix = (base == "") ? "" : base "/"
  modfile[prefix name ".rs"] = 1
  modfile[prefix name "/mod.rs"] = 1
}
function scan(path, mode,   file, dir, leaf, base, line, c, t, v, n, k, intest, pending, fnstart, fnname, lo, j, s, f, m, d, w, count, r) {
  file = root "/" path
  leaf = path
  sub(/.*\//, "", leaf)
  dir = ""
  if (index(path, "/")) {
    dir = path
    sub(/\/[^\/]*\.rs/, "", dir)
  }
  if (leaf == "lib.rs" || leaf == "main.rs" || leaf == "mod.rs") base = dir
  else base = ((dir == "") ? "" : dir "/") substr(leaf, 1, length(leaf) - 3)
  intest = (path ~ /(^|\/)tests\// || leaf == "tests.rs" || leaf == "testing.rs" || (path in modfile))
  n = 0
  pending = 0
  fnstart = 0
  fnname = ""
  count = 0
  nsa = 0
  naa = 0
  nca = 0
  nvar = 0
  while ((r = (getline line < file)) > 0) {
    n++
    c = code(line)
    if (c ~ /(^|[ \t])use[ \t]/ && c ~ /[ \t]as[ \t]/) uses(c)
    if (c ~ /#!\[cfg\(/ && cfgtest(c)) intest = 1
    if (pending && c ~ /[^ \t\r]/ && c !~ /^[ \t]*#/) {
      if (c ~ /^[ \t]*(pub(\([a-z]+\))? )?mod [A-Za-z_0-9]+[ \t]*\{/) intest = 1
      else if (mode == 1 && c ~ /^[ \t]*(pub(\([a-z]+\))? )?mod [A-Za-z_0-9]+[ \t]*;/) addmod(c, base)
      pending = 0
    }
    if (c ~ /#\[cfg\(/ && cfgtest(c)) {
      t = c
      sub(/.*\][ \t]*/, "", t)
      if (t ~ /^(pub(\([a-z]+\))? )?mod [A-Za-z_0-9]+[ \t]*\{/) intest = 1
      else if (t ~ /^(pub(\([a-z]+\))? )?mod [A-Za-z_0-9]+[ \t]*;/) {
        if (mode == 1) addmod(t, base)
      } else pending = 1
    }
    if (c ~ /(^|[^A-Za-z0-9_])fn [A-Za-z_]/) {
      fnstart = n
      fnname = ""
      nvar = 0
      if (mode == 1 && intest && (c " ") ~ /->[ \t]*(String|OsString|u128|u64|u32|usize|i64|i128)[ \t{]/) {
        fnname = c
        sub(/.*fn[ \t]+/, "", fnname)
        sub(/[^A-Za-z0-9_].*/, "", fnname)
      }
    }
    if (mode == 1) {
      if (fnname != "" && n - fnstart <= 16 && source(c)) {
        addfn(fnname)
        fnname = ""
      }
      continue
    }
    cc[n] = c
    it[n] = intest
    fs[n] = fnstart
    src[n] = source(c)
    fmt[n] = (c ~ /(format|write)!\(/)
    nam[n] = (src[n] && c ~ /\.join\(|to_string\(\)|push_str\(|\.push\(/)
    db[n] = (c ~ /DirBuilder/)
    rc[n] = (c ~ /\.recursive\(/ && c !~ /\.recursive\(false\)/)
    acc[n] = accepts(c)
    ref[n] = refuses(c)
    if (read(c) && c !~ /\.join\(|(format|write)!\(/ && match(c, /let[ \t]+(mut[ \t]+)?[A-Za-z_][A-Za-z_0-9]*/)) {
      v = substr(c, RSTART, RLENGTH)
      sub(/let[ \t]+(mut[ \t]+)?/, "", v)
      var[++nvar] = v
    }
  }
  if (r < 0) {
    printf "test-dirs unread=%s\n", path
    faults++
  }
  close(file)
  if (mode == 1) return 0
  for (k = 1; k <= n; k++) {
    if (!it[k]) continue
    lo = k - 16
    if (fs[k] > lo) lo = fs[k]
    if (lo < 1) lo = 1
    s = 0
    f = 0
    m = 0
    d = 0
    w = 0
    for (j = lo; j <= k; j++) {
      s += src[j]
      f += fmt[j]
      m += nam[j]
      d += db[j]
      w += rc[j]
    }
    if (!acc[k] && !ref[k] && !(d && !w && cc[k] ~ /\.create\(/)) continue
    if (m || (s && f)) {
      count++
      print path ":" k > placelist
    }
  }
  return count
}
BEGIN {
  k = 0
  while ((r = (getline line < knownlist)) > 0) {
    k++
    if (split(line, w, " ") == 0) continue
    if (w[1] in want || w[2] !~ /^[1-9]/ || w[2] ~ /[^0-9]/) {
      printf "test-dirs known-line=%d\n", k
      faults++
      continue
    }
    want[w[1]] = w[2]
    order[++nk] = w[1]
    kp += w[2]
  }
  if (r < 0) {
    printf "test-dirs known-list=unread\n"
    faults++
  }
  close(knownlist)
  if (nk > pin_rows) {
    printf "test-dirs pin=rows known=%d pinned=%d: the known list may not grow; a new row needs a ruling, else take halvo_test_dir::make at the new place\n", nk, pin_rows
    faults++
  } else if (nk < pin_rows) {
    printf "test-dirs pin=rows known=%d pinned=%d: set pinned_rows=%d and pinned_paths=%s beside known() in scripts/r3/ci.d/15-r3-test-dirs.sh, in the commit that mends the place\n", nk, pin_rows, nk, paths
    faults++
  }
  if (kp > pin_places) {
    printf "test-dirs pin=places known=%d pinned=%d: the known list may not grow; a higher count needs a ruling, else take halvo_test_dir::make at the new place\n", kp, pin_places
    faults++
  } else if (kp < pin_places) {
    printf "test-dirs pin=places known=%d pinned=%d: set pinned_places=%d and pinned_paths=%s beside known() in scripts/r3/ci.d/15-r3-test-dirs.sh, in the commit that mends the place\n", kp, pin_places, kp, paths
    faults++
  }
  if (nk == pin_rows && kp == pin_places && paths != pin_paths) {
    printf "test-dirs pin=paths: known() names a file or a count that pinned_paths does not allow; a new row, a raised count or a count moved to another row needs a ruling (after a mend that lowered pinned_rows or pinned_places, pinned_paths=%s)\n", paths
    faults++
  }
  while ((r = (getline path < filelist)) > 0) {
    sub(/\r/, "", path)
    if (substr(path, 1, 1) == "\"") {
      printf "test-dirs name=quoted\n"
      faults++
      continue
    }
    if (substr(path, 1, length(skip)) == skip) continue
    list[++files] = path
  }
  if (r < 0) {
    printf "test-dirs file-list=unread\n"
    faults++
  }
  close(filelist)
  for (i = 1; i <= files; i++) scan(list[i], 1)
  for (i = 1; i <= files; i++) {
    path = list[i]
    c = scan(path, 2)
    if (c) {
      got[path] = c
      seen[++ns] = path
      total += c
    }
  }
  for (i = 1; i <= ns; i++) {
    p = seen[i]
    if (!(p in want)) {
      printf "test-dirs unknown=%s places=%d: take halvo_test_dir::make there; the known list in scripts/r3/ci.d/15-r3-test-dirs.sh may not grow\n", p, got[p]
      faults++
    } else if (got[p] > want[p]) {
      printf "test-dirs count=%s places=%d known=%d: take halvo_test_dir::make at the new place; the known list in scripts/r3/ci.d/15-r3-test-dirs.sh may not grow\n", p, got[p], want[p]
      faults++
    } else if (got[p] < want[p]) {
      printf "test-dirs count=%s places=%d known=%d: set its row in known() to %d and lower pinned_places and pinned_paths beside it in scripts/r3/ci.d/15-r3-test-dirs.sh\n", p, got[p], want[p], got[p]
      faults++
    }
  }
  for (i = 1; i <= nk; i++) {
    p = order[i]
    if (!(p in got)) {
      printf "test-dirs gone=%s known=%d: take its row out of known() and lower pinned_rows, pinned_places and pinned_paths beside it in scripts/r3/ci.d/15-r3-test-dirs.sh\n", p, want[p]
      faults++
    }
  }
  printf "test-dirs files=%d places=%d known=%d faults=%d\n", files, total, nk, faults
  exit faults ? 1 : 0
}
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

paths_of() {
  awk 'NF { print $1, $2 }' "$1" | sort | cksum | awk '{ print $1 }'
}

measure() {
  repo=$1
  list=$2
  out=$3
  read -r pin_rows pin_places pin_paths <"$4" || fail pins
  paths=$(paths_of "$list") || fail setup
  git -C "$repo" -c core.quotePath=false ls-files -- '*.rs' >"$out/files" || fail git
  : >"$out/places.txt" || fail setup
  awk -v root="$repo" -v skip=crates/halvo-test-dir/ -v placelist="$out/places.txt" \
    -v knownlist="$list" -v filelist="$out/files" -v pin_rows="$pin_rows" \
    -v pin_places="$pin_places" -v pin_paths="$pin_paths" -v paths="$paths" "$program"
  case $? in
    0) printf 'PASS %s\n' "$name" ;;
    1) fail places ;;
    *) fail awk ;;
  esac
}

main() {
  repo=${CI_D_REPO:-.}
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail setup
  artifacts=${CI_D_ARTIFACTS:-$scratch}
  known >"$scratch/known" || fail setup
  printf '%s %s %s\n' "$pinned_rows" "$pinned_places" "$pinned_paths" >"$scratch/pins" || fail setup
  measure "$repo" "$scratch/known" "$artifacts" "$scratch/pins"
}

help() {
  cat <<'EOF'
usage: sh scripts/r3/ci.d/15-r3-test-dirs.sh [--self-test | --help]

The check is lexical: it reads lines of tracked .rs files, not Rust's syntax.

Test code that it reads:
  a file under a directory named tests, a file named tests.rs or testing.rs, a module
  whose cfg names test (#[cfg(test)], #[cfg(any(unix, test))], #[cfg(all(test, unix))])
  inline or as a file (mod x; on the next line or the same line), and a file that opens
  with #![cfg(test)]. A cfg with not(test) opens no test code.

Calls that make a directory:
  create_dir_all, a DirBuilder whose recursive() takes anything but false, a plain
  DirBuilder create, create_dir, and each of them under another name by use ... as.
  A loop that retries a create_dir changes nothing: the check does not read loops.

Reads of the process id or the clock:
  process::id(), id(), getpid(), GetCurrentProcessId, SystemTime, UNIX_EPOCH,
  Instant::now, nanos(), micros(), millis(), Utc::now, Local::now; each of the first
  three under another name by use ... as; a variable that a let binds from such a read
  in the same function; and a test function that returns a String, an OsString or an
  integer and holds such a read within 16 lines of its first line.

A place: a call that makes a directory, in test code, when a read stands on one line with
.join(, to_string(), push_str( or .push(, or when a read and a format! or write! stand in
the lines above it in its function, at most 16 lines up.

Forms that it does not read (each can share a folder and pass):
  a name built in another function that returns a PathBuf, a Path or a &str, and a
  caller that makes the folder; a name handed to a child process that makes the folder;
  a read more than 16 lines above the call in its function; a variable bound from
  another variable, a struct field or a closure; a SystemTime or an Instant under
  another name by use ... as; a macro other than format! and write!; a module that
  #[path = ...] or cfg_attr places. After an inline test module opens, the rest of its
  file is read as test code.

The known list, known() in this script, holds each file of another unit with its count of
places. A file's count must equal its row: a new place takes halvo_test_dir::make.
Beside it, pinned_rows, pinned_places and pinned_paths (a cksum of the sorted pairs of
path and count) pin the list: it may shrink with its pins and may not grow, and a count
may not move from one row to another without a ruling. The known list is closed: it
shrinks only, and new code takes its folder from halvo-test-dir. A raised pin passes,
because the check cannot know a ruling: the review of the diff is the gate.
EOF
}

broken() {
  printf 'FAIL self-test:%s (fixture %s)\n' "$name" "$1"
  exit 1
}

put() {
  mkdir -p "$work/$label/${1%/*}" || broken "$label: mkdir"
  printf '%b' "$2" >"$work/$label/$1" || broken "$label: write"
  git -C "$work/$label" add -- "$1" || broken "$label: add"
}

repo() {
  label=$1
  git init -q "$work/$label" >/dev/null 2>&1 || broken "$label: git init"
  : >"$work/$label.known" || broken "$label: known"
}

pin_file() {
  label=$1
  rows=$2
  places=$3
  shift 3
  : >"$work/$label.pinned-paths" || broken "$label: pinned paths"
  while [ $# -ge 2 ]; do
    printf '%s %s pinned\n' "$1" "$2" >>"$work/$label.pinned-paths" || broken "$label: pinned paths"
    shift 2
  done
  digest=$(paths_of "$work/$label.pinned-paths") || broken "$label: digest"
  printf '%s %s %s\n' "$rows" "$places" "$digest" >"$work/$label.pins" || broken "$label: pins"
}

pins_of() {
  awk 'NF { r++; p += $2 } END { printf "%d %d ", r, p }' "$1" || return 1
  paths_of "$1"
}

case_of() {
  [ -f "$work/$1.pins" ] || pins_of "$work/$1.known" >"$work/$1.pins" || broken "$1: pins"
  out=$(mkdir -p "$work/$1.out" && measure "$work/$1" "$work/$1.known" "$work/$1.out" "$work/$1.pins" 2>&1)
  verdict "$1" "$2" "$3"
}

verdict() {
  code=$?
  last=${out##*"$nl"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$3"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
    printf '%s\n' "$out" >"$artifacts/self-test-$1.txt"
  elif [ "$2" != PASS ]; then
    rejected=$((rejected + 1))
  fi
}

place='fn scratch(name: &str) -> PathBuf {\n    let dir = base.join(format!("{name}-{}", std::process::id()));\n    std::fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || broken mktemp
  artifacts=${CI_D_ARTIFACTS:-$work}
  wrong=
  rejected=0

  repo pid-in-tests
  put crates/a/tests/common/mod.rs "$place"
  case_of pid-in-tests FAIL 'test-dirs unknown=crates/a/tests/common/mod.rs places=1'

  repo clock-in-testing
  put crates/a/src/testing.rs 'fn scratch(name: &str) -> PathBuf {\n    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();\n    let dir = base.join(format!("{name}-{nanos}"));\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of clock-in-testing FAIL 'test-dirs unknown=crates/a/src/testing.rs places=1'

  for source in 'process::id()' 'libc::getpid()' 'GetCurrentProcessId()' 'Instant::now()' 'nanos()' 'd.as_micros()' 't.as_millis()' 'Utc::now()' 'Local::now()' 'UNIX_EPOCH'; do
    label=source-$(printf '%s' "$source" | tr -c 'a-zA-Z' '-')
    repo "$label"
    put crates/a/tests/t.rs "fn scratch() -> PathBuf {\n    let v = $source;\n    let dir = base.join(format!(\"x-{v}\"));\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n"
    case_of "$label" FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'
  done

  repo write-macro
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let mut s = String::new();\n    write!(s, "x-{}", std::process::id()).unwrap();\n    fs::create_dir_all(base.join(&s)).unwrap();\n    base.join(s)\n}\n'
  case_of write-macro FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo dir-builder
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    DirBuilder::new()\n        .recursive(true)\n        .create(&dir)\n        .unwrap();\n    dir\n}\n'
  case_of dir-builder FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo inline-test-module
  put crates/a/src/lib.rs "pub fn f() {}\n\n#[cfg(test)]\nmod tests {\n    use super::*;\n\n$place}\n"
  case_of inline-test-module FAIL 'test-dirs unknown=crates/a/src/lib.rs places=1'

  repo inline-module-after-attribute
  put crates/a/src/lib.rs "pub fn f() {}\n\n#[cfg(test)]\n// the tests\n#[allow(dead_code)]\npub(crate) mod tests {\n$place}\n"
  case_of inline-module-after-attribute FAIL 'test-dirs unknown=crates/a/src/lib.rs places=1'

  repo cfg-and-mod-on-one-line
  put crates/a/src/lib.rs "#[cfg(test)] mod tests {\n$place}\n"
  case_of cfg-and-mod-on-one-line FAIL 'test-dirs unknown=crates/a/src/lib.rs places=1'

  repo product-code
  put crates/a/src/lib.rs "$place\n#[cfg(test)]\nmod testing;\n\n$place"
  case_of product-code PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo cfg-test-on-an-item
  put crates/a/src/lib.rs "#[cfg(test)]\nfn helper() {}\n\n$place"
  case_of cfg-test-on-an-item PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo maker-loop
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    loop {\n        let dir = base.join(format!("x-{}-{}", std::process::id(), NEXT.fetch_add(1, SeqCst)));\n        if std::fs::create_dir(&dir).is_ok() {\n            return dir;\n        }\n    }\n}\n'
  case_of maker-loop FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo base-and-dir-on-one-line
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    for n in 0.. {\n        let dir = base.join(format!("x-{}-{n}", std::process::id()));\n        if fs::create_dir_all(base).is_ok() && fs::create_dir(&dir).is_ok() {\n            return dir;\n        }\n    }\n}\n'
  case_of base-and-dir-on-one-line FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo the-maker-itself
  put crates/halvo-test-dir/src/lib.rs "#[cfg(test)]\nmod tests {\n$place}\n"
  put crates/halvo-test-dir/tests/t.rs "$place"
  case_of the-maker-itself PASS 'test-dirs files=0 places=0 known=0 faults=0'

  repo a-crate-named-like-the-maker
  put crates/halvo-test-dir-two/tests/t.rs "$place"
  case_of a-crate-named-like-the-maker FAIL 'test-dirs unknown=crates/halvo-test-dir-two/tests/t.rs places=1'

  repo no-name-format
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let own = std::process::id();\n    assert!(own > 0);\n    fs::create_dir_all(base.join("data")).unwrap();\n    base.join("data")\n}\n'
  case_of no-name-format PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo source-in-the-function-before
  put crates/a/tests/t.rs 'fn pid() -> String {\n    format!("{}", std::process::id())\n}\n\nfn scratch() -> PathBuf {\n    let dir = base.join(format!("x"));\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of source-in-the-function-before PASS 'test-dirs files=1 places=0 known=0 faults=0'

  filler=
  i=0
  while [ "$i" -lt 15 ]; do
    filler="$filler    let _ = $i;\n"
    i=$((i + 1))
  done
  repo sixteen-lines-above
  put crates/a/tests/t.rs "fn scratch() -> PathBuf {\n    let dir = base.join(format!(\"x-{}\", std::process::id()));\n$filler    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n"
  case_of sixteen-lines-above FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo seventeen-lines-above
  put crates/a/tests/t.rs "fn scratch() -> PathBuf {\n    let dir = base.join(format!(\"x-{}\", std::process::id()));\n$filler    let _ = 15;\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n"
  case_of seventeen-lines-above PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo pid-to-string-in-a-join
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let dir = std::env::temp_dir().join(std::process::id().to_string());\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of pid-to-string-in-a-join FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo bare-id-after-use
  put crates/a/tests/t.rs 'use std::process::id;\n\nfn scratch(name: &str) -> PathBuf {\n    let dir = base.join(format!("{name}-{}", id()));\n    create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of bare-id-after-use FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo cfg-all-test-unix
  put crates/a/src/lib.rs "pub fn f() {}\n\n#[cfg(all(test, unix))]\nmod tests {\n$place}\n"
  case_of cfg-all-test-unix FAIL 'test-dirs unknown=crates/a/src/lib.rs places=1'

  repo cfg-not-test
  put crates/a/src/lib.rs "#[cfg(not(test))]\nmod real {\n$place}\n"
  case_of cfg-not-test PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo fn-in-a-comment
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    // this fn makes the directory once\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of fn-in-a-comment FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo test-file-module
  put crates/a/src/lib.rs '#[cfg(test)]\nmod support;\n\npub fn f() {}\n'
  put crates/a/src/support.rs "$place"
  case_of test-file-module FAIL 'test-dirs unknown=crates/a/src/support.rs places=1'

  repo test-file-module-of-a-module
  put crates/a/src/lib.rs 'mod sheet;\n'
  put crates/a/src/sheet.rs '#[cfg(test)]\nmod support;\n'
  put crates/a/src/sheet/support/mod.rs "$place"
  case_of test-file-module-of-a-module FAIL 'test-dirs unknown=crates/a/src/sheet/support/mod.rs places=1'

  repo product-file-module
  put crates/a/src/lib.rs 'mod support;\n'
  put crates/a/src/support.rs "$place"
  case_of product-file-module PASS 'test-dirs files=2 places=0 known=0 faults=0'

  repo name-from-a-helper
  put crates/a/tests/t.rs 'fn stamp() -> String {\n    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();\n    format!("x-{nanos}")\n}\n\nfn scratch() -> PathBuf {\n    let dir = base.join(stamp());\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of name-from-a-helper FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo both-calls-on-one-line
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    fs::create_dir_all(&dir).and_then(|()| fs::create_dir(dir.join("in"))).ok();\n    dir\n}\n'
  case_of both-calls-on-one-line FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo create-dir-error-dropped
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    let _ = fs::create_dir(&dir);\n    dir\n}\n'
  case_of create-dir-error-dropped FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo create-dir-retry-in-a-loop-without-a-test
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    loop {\n        let dir = base.join(format!("x-{}", std::process::id()));\n        fs::create_dir(&dir).unwrap();\n        return dir;\n    }\n}\n'
  case_of create-dir-retry-in-a-loop-without-a-test FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo maker-match-in-a-loop
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    for n in 0u32.. {\n        let dir = base.join(format!("x-{}-{n}", std::process::id()));\n        match fs::create_dir(&dir) {\n            Ok(()) => return dir,\n            Err(e) if e.kind() == ErrorKind::AlreadyExists => {}\n            Err(e) => panic!("{e}"),\n        }\n    }\n    unreachable!()\n}\n'
  case_of maker-match-in-a-loop FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo create-dir-tested-without-a-loop
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    if fs::create_dir(&dir).is_ok() {\n        return dir;\n    }\n    panic!("taken")\n}\n'
  case_of create-dir-tested-without-a-loop FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo maker-result-tested-below-is-a-place
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    for n in 0u32.. {\n        let dir = base.join(format!("x-{}-{n}", std::process::id()));\n        let made = fs::create_dir(&dir);\n        if made.as_ref().err().map(|e| e.kind()) == Some(ErrorKind::AlreadyExists) {\n            continue;\n        }\n        made.unwrap();\n        return dir;\n    }\n    unreachable!()\n}\n'
  case_of maker-result-tested-below-is-a-place FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo maker-find
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    (0..1_000)\n        .map(|n| base.join(format!("x-{}-{n}", std::process::id())))\n        .find(|dir| std::fs::create_dir(dir).is_ok())\n        .expect("a new directory")\n}\n'
  case_of maker-find FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo tested-create-dir-in-an-unrelated-loop
  put crates/a/tests/t.rs 'fn scratch(name: &str) -> PathBuf {\n    let mut last = PathBuf::new();\n    for case in ["one", "two"] {\n        let dir = base.join(format!("{name}-{case}-{}", std::process::id()));\n        if fs::create_dir(&dir).is_ok() {\n            println!("made");\n        }\n        last = dir;\n    }\n    last\n}\n'
  case_of tested-create-dir-in-an-unrelated-loop FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo loop-word-in-a-string
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let d = base.join(format!("x-{}", std::process::id()));\n    let note = "made for the test";\n    assert!(fs::create_dir(&d).is_ok() || d.is_dir(), "{note}");\n    d\n}\n'
  case_of loop-word-in-a-string FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo create-dir-all-and-create-dir-in-a-match
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    loop {\n        let dir = base.join(format!("x-{}", std::process::id()));\n        match fs::create_dir_all(&dir).and(fs::create_dir(dir.join("in"))) {\n            Ok(()) => return dir,\n            Err(_) => {}\n        }\n    }\n}\n'
  case_of create-dir-all-and-create-dir-in-a-match FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo find-without-is-ok
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    (0..3)\n        .map(|n| base.join(format!("x-{}-{n}", std::process::id())))\n        .find(|d| fs::create_dir(d).map(|()| true).unwrap_or(true))\n        .expect("a folder")\n}\n'
  case_of find-without-is-ok FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo return-on-the-line-of-the-test
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    loop {\n        let dir = base.join(format!("x-{}", std::process::id()));\n        if fs::create_dir(&dir).is_ok() { return dir; }\n    }\n}\n'
  case_of return-on-the-line-of-the-test FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo ok-arm-with-a-block
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    loop {\n        let dir = base.join(format!("x-{}", std::process::id()));\n        match fs::create_dir(&dir) {\n            Ok(()) => {\n                return dir;\n            }\n            Err(_) => {}\n        }\n    }\n}\n'
  case_of ok-arm-with-a-block FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo unconditional-break
  put crates/a/tests/t.rs 'fn scratch(name: &str) -> PathBuf {\n    let mut dir = PathBuf::new();\n    loop {\n        dir = base.join(format!("{name}-{}", std::process::id()));\n        if fs::create_dir(&dir).is_ok() {\n            println!("made {}", dir.display());\n        }\n        break;\n    }\n    dir\n}\n'
  case_of unconditional-break FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo break-on-exists
  put crates/a/tests/t.rs 'fn scratch(name: &str) -> PathBuf {\n    let mut dir = PathBuf::new();\n    for n in 0..3 {\n        dir = base.join(format!("{name}-{}-{n}", std::process::id()));\n        if fs::create_dir(&dir).is_ok() || dir.exists() {\n            break;\n        }\n    }\n    dir\n}\n'
  case_of break-on-exists FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo find-or-exists
  put crates/a/tests/t.rs 'fn scratch(name: &str) -> PathBuf {\n    (0..3)\n        .map(|n| base.join(format!("{name}-{}-{n}", std::process::id())))\n        .find(|d| fs::create_dir(d).is_ok() || d.exists())\n        .expect("a folder")\n}\n'
  case_of find-or-exists FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo tested-result-joined-by-and
  put crates/a/tests/t.rs 'fn scratch(name: &str, keep: bool) -> PathBuf {\n    loop {\n        let dir = base.join(format!("{name}-{}", std::process::id()));\n        if fs::create_dir(&dir).is_ok() && keep {\n            return dir;\n        }\n    }\n}\n'
  case_of tested-result-joined-by-and FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo loop-word-in-a-string-beside-a-return
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let d = base.join(format!("x-{}", std::process::id()));\n    let note = "made for the test";\n    if fs::create_dir(&d).is_ok() {\n        return d;\n    }\n    panic!("{note}")\n}\n'
  case_of loop-word-in-a-string-beside-a-return FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo return-word-in-a-string
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let mut last = PathBuf::new();\n    for case in ["one", "two"] {\n        let dir = base.join(format!("x-{case}-{}", std::process::id()));\n        if fs::create_dir(&dir).is_ok() {\n            println!("return later");\n        }\n        last = dir;\n    }\n    last\n}\n'
  case_of return-word-in-a-string FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo variable-from-a-variable
  put crates/a/tests/t.rs 'fn aside() {\n    let first = u64::from(std::process::id()) << 32;\n    let moved = removing_name(first);\n    fs::write(folder.join(&moved), b"mine").unwrap();\n    fs::create_dir_all(scratch.join("data")).unwrap();\n}\n'
  case_of variable-from-a-variable PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo create-dir-all-retried-in-a-loop
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    loop {\n        let d = base.join(format!("x-{}", std::process::id()));\n        if fs::create_dir_all(&d).is_ok() {\n            return d;\n        }\n    }\n}\n'
  case_of create-dir-all-retried-in-a-loop FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo maker-loop-break
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let mut n = 0u32;\n    loop {\n        let dir = base.join(format!("x-{}-{n}", std::process::id()));\n        if fs::create_dir(&dir).is_ok() {\n            break dir;\n        }\n        n += 1;\n    }\n}\n'
  case_of maker-loop-break FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo unknown-line-names-the-fix
  put crates/a/tests/t.rs "$place"
  case_of unknown-line-names-the-fix FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1: take halvo_test_dir::make there; the known list in scripts/r3/ci.d/15-r3-test-dirs.sh may not grow'

  repo count-up-line-names-the-fix
  put crates/a/tests/t.rs "$place\n$place"
  printf 'crates/a/tests/t.rs 1 a run\n' >"$work/count-up-line-names-the-fix.known" || broken known
  case_of count-up-line-names-the-fix FAIL 'test-dirs count=crates/a/tests/t.rs places=2 known=1: take halvo_test_dir::make at the new place; the known list in scripts/r3/ci.d/15-r3-test-dirs.sh may not grow'

  repo count-down-line-names-the-fix
  put crates/a/tests/t.rs "$place"
  printf 'crates/a/tests/t.rs 2 a run\n' >"$work/count-down-line-names-the-fix.known" || broken known
  case_of count-down-line-names-the-fix FAIL 'test-dirs count=crates/a/tests/t.rs places=1 known=2: set its row in known() to 1 and lower pinned_places and pinned_paths beside it in scripts/r3/ci.d/15-r3-test-dirs.sh'

  repo gone-line-names-the-fix
  put crates/a/tests/t.rs 'fn f() {}\n'
  printf 'crates/a/tests/t.rs 1 a run\n' >"$work/gone-line-names-the-fix.known" || broken known
  case_of gone-line-names-the-fix FAIL 'test-dirs gone=crates/a/tests/t.rs known=1: take its row out of known() and lower pinned_rows, pinned_places and pinned_paths beside it in scripts/r3/ci.d/15-r3-test-dirs.sh'

  repo row-added-with-its-place
  put crates/a/tests/t.rs "$place"
  printf 'crates/a/tests/t.rs 1 a run\n' >"$work/row-added-with-its-place.known" || broken known
  pin_file row-added-with-its-place 0 0
  case_of row-added-with-its-place FAIL 'test-dirs pin=rows known=1 pinned=0'

  repo count-raised-with-a-second-place
  put crates/a/tests/t.rs "$place\n$place"
  printf 'crates/a/tests/t.rs 2 a run\n' >"$work/count-raised-with-a-second-place.known" || broken known
  pin_file count-raised-with-a-second-place 1 1 crates/a/tests/t.rs 1
  case_of count-raised-with-a-second-place FAIL 'test-dirs pin=places known=2 pinned=1'

  repo mend-with-row-and-pins-lowered
  put crates/a/tests/t.rs 'fn f() {}\n'
  pin_file mend-with-row-and-pins-lowered 0 0
  case_of mend-with-row-and-pins-lowered PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo mend-with-the-pins-left
  put crates/a/tests/t.rs 'fn f() {}\n'
  pin_file mend-with-the-pins-left 1 1 crates/a/tests/t.rs 1
  case_of mend-with-the-pins-left FAIL 'test-dirs pin=rows known=0 pinned=1'

  repo row-swapped-under-the-pins
  put crates/a/tests/u.rs "$place"
  printf 'crates/a/tests/u.rs 1 a run\n' >"$work/row-swapped-under-the-pins.known" || broken known
  pin_file row-swapped-under-the-pins 1 1 crates/a/tests/t.rs 1
  case_of row-swapped-under-the-pins FAIL 'test-dirs pin=paths'

  repo count-moved-between-rows
  put crates/a/tests/a.rs "$place"
  put crates/a/tests/b.rs "$place\n$place"
  printf 'crates/a/tests/a.rs 1 a run\ncrates/a/tests/b.rs 2 a run\n' >"$work/count-moved-between-rows.known" || broken known
  pin_file count-moved-between-rows 2 3 crates/a/tests/a.rs 2 crates/a/tests/b.rs 1
  case_of count-moved-between-rows FAIL 'test-dirs pin=paths'

  repo mend-with-the-places-pin-left
  put crates/a/tests/t.rs "$place"
  printf 'crates/a/tests/t.rs 1 a run\n' >"$work/mend-with-the-places-pin-left.known" || broken known
  pin_file mend-with-the-places-pin-left 1 2 crates/a/tests/t.rs 2
  case_of mend-with-the-places-pin-left FAIL 'test-dirs pin=places known=1 pinned=2'

  repo pins-raised-with-the-row-left
  put crates/a/tests/t.rs "$place\n$place"
  printf 'crates/a/tests/t.rs 1 a run\n' >"$work/pins-raised-with-the-row-left.known" || broken known
  pin_file pins-raised-with-the-row-left 1 2 crates/a/tests/t.rs 2
  case_of pins-raised-with-the-row-left FAIL 'test-dirs count=crates/a/tests/t.rs places=2 known=1'

  repo mend-with-the-three-pins-moved
  put crates/a/tests/a.rs "$place"
  printf 'crates/a/tests/a.rs 1 a run\n' >"$work/mend-with-the-three-pins-moved.known" || broken known
  pin_file mend-with-the-three-pins-moved 1 1 crates/a/tests/a.rs 1
  case_of mend-with-the-three-pins-moved PASS 'test-dirs files=1 places=1 known=1 faults=0'

  repo pid-under-another-name
  put crates/a/tests/t.rs 'use std::process::id as pid;\n\nfn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", pid()));\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of pid-under-another-name FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo pid-in-a-variable-joined
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let pid = std::process::id();\n    let dir = base.join(pid.to_string());\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of pid-in-a-variable-joined FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo number-in-a-variable-joined
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let pid = 7u32;\n    let dir = base.join(pid.to_string());\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of number-in-a-variable-joined PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo dir-builder-not-recursive
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    DirBuilder::new().create(&dir).ok();\n    dir\n}\n'
  case_of dir-builder-not-recursive FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo inner-cfg-test-file
  put crates/a/src/lib.rs 'mod support;\n'
  put crates/a/src/support.rs "#![cfg(test)]\n\n$place"
  case_of inner-cfg-test-file FAIL 'test-dirs unknown=crates/a/src/support.rs places=1'

  repo inner-cfg-not-test-file
  put crates/a/src/lib.rs 'mod support;\n'
  put crates/a/src/support.rs "#![cfg(not(test))]\n\n$place"
  case_of inner-cfg-not-test-file PASS 'test-dirs files=2 places=0 known=0 faults=0'

  repo create-dir-all-under-another-name
  put crates/a/tests/t.rs 'use std::fs::create_dir_all as mk;\n\nfn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    mk(&dir).unwrap();\n    dir\n}\n'
  case_of create-dir-all-under-another-name FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo recursive-from-a-variable
  put crates/a/tests/t.rs 'fn scratch(rec: bool) -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    DirBuilder::new().recursive(rec).create(&dir).unwrap();\n    dir\n}\n'
  case_of recursive-from-a-variable FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo tests-rs-declared-with-no-cfg
  put crates/a/src/lib.rs 'mod tests;\n'
  put crates/a/src/tests.rs "$place"
  case_of tests-rs-declared-with-no-cfg FAIL 'test-dirs unknown=crates/a/src/tests.rs places=1'

  repo tests-rs-control
  put crates/a/src/lib.rs 'mod testz;\n'
  put crates/a/src/testz.rs "$place"
  case_of tests-rs-control PASS 'test-dirs files=2 places=0 known=0 faults=0'

  repo system-time-printed
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let t = SystemTime::now();\n    let dir = base.join(format!("x-{t:?}"));\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of system-time-printed FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo system-time-control
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let t = SystemDate::now();\n    let dir = base.join(format!("x-{t:?}"));\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  case_of system-time-control PASS 'test-dirs files=1 places=0 known=0 faults=0'

  stamp='fn stamp() -> String {\n    format!("{:?}", SystemTime::now())\n}\n\n'
  repo push-str-build
  put crates/a/tests/t.rs "${stamp}fn scratch() -> PathBuf {\n    let mut s = String::from(\"x-\");\n    s.push_str(&stamp());\n    let dir = base.join(&s);\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n"
  case_of push-str-build FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo push-str-control
  put crates/a/tests/t.rs "${stamp}fn scratch() -> PathBuf {\n    let mut s = String::from(\"x-\");\n    s.pusx_str(&stamp());\n    let dir = base.join(&s);\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n"
  case_of push-str-control PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo push-build
  put crates/a/tests/t.rs "${stamp}fn scratch() -> PathBuf {\n    let mut p = base.to_path_buf();\n    p.push(stamp());\n    fs::create_dir_all(&p).unwrap();\n    p\n}\n"
  case_of push-build FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo push-control
  put crates/a/tests/t.rs "${stamp}fn scratch() -> PathBuf {\n    let mut p = base.to_path_buf();\n    p.pusx(stamp());\n    fs::create_dir_all(&p).unwrap();\n    p\n}\n"
  case_of push-control PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo maker-if-let-ok
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    for n in 0u32.. {\n        let dir = base.join(format!("x-{}-{n}", std::process::id()));\n        if let Ok(()) = fs::create_dir(&dir) {\n            return dir;\n        }\n    }\n    unreachable!()\n}\n'
  case_of maker-if-let-ok FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo maker-if-let-control
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    for n in 0u32.. {\n        let dir = base.join(format!("x-{}-{n}", std::process::id()));\n        if let Okay(()) = fs::create_dir(&dir) {\n            return dir;\n        }\n    }\n    unreachable!()\n}\n'
  case_of maker-if-let-control FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo cfg-test-after-a-space
  put crates/a/src/lib.rs "#[cfg(any(unix, test))]\nmod tests {\n$place}\n"
  case_of cfg-test-after-a-space FAIL 'test-dirs unknown=crates/a/src/lib.rs places=1'

  repo cfg-test-after-a-comma
  put crates/a/src/lib.rs "#[cfg(any(unix,test))]\nmod tests {\n$place}\n"
  case_of cfg-test-after-a-comma FAIL 'test-dirs unknown=crates/a/src/lib.rs places=1'

  repo cfg-after-a-comma-control
  put crates/a/src/lib.rs "#[cfg(any(unix,tesx))]\nmod tests {\n$place}\n"
  case_of cfg-after-a-comma-control PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo one-line-cfg-file-module
  put crates/a/src/lib.rs '#[cfg(test)] mod support;\n'
  put crates/a/src/support.rs "$place"
  case_of one-line-cfg-file-module FAIL 'test-dirs unknown=crates/a/src/support.rs places=1'

  repo one-line-cfg-file-module-control
  put crates/a/src/lib.rs '#[cfg(tesx)] mod support;\n'
  put crates/a/src/support.rs "$place"
  case_of one-line-cfg-file-module-control PASS 'test-dirs files=2 places=0 known=0 faults=0'

  helper_filler=
  i=0
  while [ "$i" -lt 15 ]; do
    helper_filler="$helper_filler    let _ = $i;\n"
    i=$((i + 1))
  done
  use_stamp='fn scratch() -> PathBuf {\n    let dir = base.join(stamp().to_string());\n    fs::create_dir_all(&dir).unwrap();\n    dir\n}\n'
  repo helper-source-sixteen-lines-down
  put crates/a/tests/t.rs "fn stamp() -> u64 {\n${helper_filler}    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()\n}\n\n$use_stamp"
  case_of helper-source-sixteen-lines-down FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo helper-source-seventeen-lines-down
  put crates/a/tests/t.rs "fn stamp() -> u64 {\n${helper_filler}    let _ = 15;\n    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs()\n}\n\n$use_stamp"
  case_of helper-source-seventeen-lines-down PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo two-places-known
  put crates/a/tests/t.rs "$place\n$place"
  printf 'crates/a/tests/t.rs 2 a run\n' >"$work/two-places-known.known" || broken known
  case_of two-places-known PASS 'test-dirs files=1 places=2 known=1 faults=0'

  repo a-place-more-than-known
  put crates/a/tests/t.rs "$place\n$place"
  printf 'crates/a/tests/t.rs 1 a run\n' >"$work/a-place-more-than-known.known" || broken known
  case_of a-place-more-than-known FAIL 'test-dirs count=crates/a/tests/t.rs places=2 known=1'

  repo a-place-less-than-known
  put crates/a/tests/t.rs "$place"
  printf 'crates/a/tests/t.rs 2 a run\n' >"$work/a-place-less-than-known.known" || broken known
  case_of a-place-less-than-known FAIL 'test-dirs count=crates/a/tests/t.rs places=1 known=2'

  repo a-known-place-moved
  put crates/a/tests/t.rs 'fn f() {}\n'
  printf 'crates/a/tests/t.rs 1 a run\n' >"$work/a-known-place-moved.known" || broken known
  case_of a-known-place-moved FAIL 'test-dirs gone=crates/a/tests/t.rs known=1'

  repo a-known-line-twice
  put crates/a/tests/t.rs "$place"
  printf 'crates/a/tests/t.rs 1 a run\ncrates/a/tests/t.rs 1 a run\n' >"$work/a-known-line-twice.known" || broken known
  case_of a-known-line-twice FAIL 'test-dirs known-line=2'

  repo a-known-count-of-zero
  put crates/a/tests/t.rs 'fn f() {}\n'
  printf 'crates/a/tests/t.rs 0 a run\n' >"$work/a-known-count-of-zero.known" || broken known
  case_of a-known-count-of-zero FAIL 'test-dirs known-line=1'

  repo untracked
  put crates/a/tests/t.rs 'fn f() {}\n'
  printf '%b' "$place" >"$work/untracked/crates/a/tests/u.rs" || broken untracked
  case_of untracked PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo not-rust
  put crates/a/tests/t.txt "$place"
  case_of not-rust PASS 'test-dirs files=0 places=0 known=0 faults=0'

  repo crlf
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\r\n    let dir = base.join(format!("x-{}", std::process::id()));\r\n    fs::create_dir_all(&dir).unwrap();\r\n    dir\r\n}\r\n'
  case_of crlf FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo quoted-name
  git -C "$work/quoted-name" config core.protectNTFS false || broken "quoted-name: config"
  blob=$(git -C "$work/quoted-name" hash-object -w --stdin </dev/null) || broken "quoted-name: blob"
  (cd "$work/quoted-name" && MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' git update-index --add --cacheinfo "100644,$blob,crates/a/tests/t\"q.rs") || broken "quoted-name: update-index"
  case_of quoted-name FAIL 'test-dirs name=quoted'

  repo the-pinned-list
  put crates/a/tests/t.rs 'fn f() {}\n'
  mkdir -p "$work/the-pinned-list.out" || broken "the-pinned-list: mkdir"
  out=$(CI_D_REPO=$work/the-pinned-list CI_D_ARTIFACTS=$work/the-pinned-list.out sh "$self" 2>&1)
  verdict the-pinned-list FAIL 'test-dirs gone=src-tauri/src/tests.rs known=1'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $#:${1:-} in
  0:) main ;;
  1:--self-test) self_test ;;
  1:--help) help ;;
  *) fail usage ;;
esac
