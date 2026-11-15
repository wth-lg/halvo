#!/bin/sh
# jobs: macos windows
# rule: No tracked Rust test outside crates/halvo-test-dir names a scratch directory from the process id or the clock alone: in test code (a tests/ directory, tests.rs, testing.rs, a module under a cfg with test), a line that makes a directory by create_dir_all, a recursive DirBuilder or create_dir with no retry on a standing name, with a process id or clock read (also through a test function that returns one) beside a path or string build on one line, or with such a read and a format! or write! in the lines above it in its function (16 at most), is a place, and each file's count of places equals its count in the known list of this script, which names the places of other units and runs until they take halvo-test-dir (U2.5).
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
crates/halvo-checkpoint/tests/identity.rs 1 agent-setup run, U2.4
crates/halvo-derived/src/tests.rs 1 workflow run
crates/halvo-derived/tests/common/mod.rs 1 workflow run
crates/halvo-runner/tests/support/mod.rs 1 agent-setup run, U4.1
crates/halvo-sheet/tests/common/mod.rs 1 agent-setup run, U2.4
crates/halvo-web/src/child.rs 1 agent-setup run, U2.6
crates/halvo-web/src/guard.rs 2 agent-setup run, U2.6
src-tauri/src/tests.rs 1 capture run, app-3a
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

program='
function code(s) {
  sub(/\/\/.*/, "", s)
  return s
}
function cfgtest(s) {
  return s ~ /#\[cfg\(/ && s ~ /[(, ]test[),]/ && s !~ /not\([^)]*test/
}
function source(s,   i) {
  if (s ~ /process::id\(|(^|[^A-Za-z0-9_.:])id\(\)|getpid\(|GetCurrentProcessId|SystemTime|UNIX_EPOCH|Instant::now|(nanos|micros|millis)\(\)|Utc::now|Local::now/) return 1
  for (i = 1; i <= nfn; i++)
    if (index(s, fname[i] "(") && s ~ ("(^|[^A-Za-z0-9_])" fname[i] "\\(")) return 1
  return 0
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
function scan(path, mode,   file, dir, leaf, base, line, c, t, n, intest, pending, fnstart, fnname, lo, j, s, f, m, l, count, r) {
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
  while ((r = (getline line < file)) > 0) {
    n++
    c = code(line)
    if (pending && c ~ /[^ \t\r]/ && c !~ /^[ \t]*#/) {
      if (c ~ /^[ \t]*(pub(\([a-z]+\))? )?mod [A-Za-z_0-9]+[ \t]*\{/) intest = 1
      else if (mode == 1 && c ~ /^[ \t]*(pub(\([a-z]+\))? )?mod [A-Za-z_0-9]+[ \t]*;/) addmod(c, base)
      pending = 0
    }
    if (cfgtest(c)) {
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
    src[n] = source(c)
    fmt[n] = (c ~ /(format|write)!\(/)
    nam[n] = (src[n] && c ~ /\.join\(|to_string\(\)|push_str\(|\.push\(/)
    lp[n] = ((" " c " ") ~ /[^A-Za-z0-9_](loop|for|while)[^A-Za-z0-9_]/ || c ~ /\.find(_map)?\(/)
    if (intest && c ~ /create_dir_all\(|recursive\(true\)|create_dir\(/) {
      lo = n - 16
      if (fnstart > lo) lo = fnstart
      if (lo < 1) lo = 1
      s = 0
      f = 0
      m = 0
      l = 0
      for (j = lo; j <= n; j++) {
        s += src[j]
        f += fmt[j]
        m += nam[j]
        l += lp[j]
      }
      if (c ~ /create_dir\(/ && l && c ~ /is_ok\(\)|match |Ok\(/) continue
      if (m || (s && f)) {
        count++
        print path ":" n > placelist
      }
    }
  }
  if (r < 0) {
    printf "test-dirs unread=%s\n", path
    faults++
  }
  close(file)
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
  }
  if (r < 0) {
    printf "test-dirs known-list=unread\n"
    faults++
  }
  close(knownlist)
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
      printf "test-dirs unknown=%s places=%d\n", p, got[p]
      faults++
    } else if (got[p] != want[p]) {
      printf "test-dirs count=%s places=%d known=%d\n", p, got[p], want[p]
      faults++
    }
  }
  for (i = 1; i <= nk; i++) {
    p = order[i]
    if (!(p in got)) {
      printf "test-dirs gone=%s known=%d\n", p, want[p]
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

measure() {
  repo=$1
  list=$2
  out=$3
  git -C "$repo" -c core.quotePath=false ls-files -- '*.rs' >"$out/files" || fail git
  : >"$out/places.txt" || fail setup
  awk -v root="$repo" -v skip=crates/halvo-test-dir/ -v placelist="$out/places.txt" \
    -v knownlist="$list" -v filelist="$out/files" "$program"
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
  measure "$repo" "$scratch/known" "$artifacts"
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

case_of() {
  out=$(mkdir -p "$work/$1.out" && measure "$work/$1" "$work/$1.known" "$work/$1.out" 2>&1)
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
  case_of maker-loop PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo base-and-dir-on-one-line
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    for n in 0.. {\n        let dir = base.join(format!("x-{}-{n}", std::process::id()));\n        if fs::create_dir_all(base).is_ok() && fs::create_dir(&dir).is_ok() {\n            return dir;\n        }\n    }\n}\n'
  case_of base-and-dir-on-one-line PASS 'test-dirs files=1 places=0 known=0 faults=0'

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
  case_of maker-match-in-a-loop PASS 'test-dirs files=1 places=0 known=0 faults=0'

  repo create-dir-tested-without-a-loop
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    let dir = base.join(format!("x-{}", std::process::id()));\n    if fs::create_dir(&dir).is_ok() {\n        return dir;\n    }\n    panic!("taken")\n}\n'
  case_of create-dir-tested-without-a-loop FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo maker-result-tested-below-is-a-place
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    for n in 0u32.. {\n        let dir = base.join(format!("x-{}-{n}", std::process::id()));\n        let made = fs::create_dir(&dir);\n        if made.as_ref().err().map(|e| e.kind()) == Some(ErrorKind::AlreadyExists) {\n            continue;\n        }\n        made.unwrap();\n        return dir;\n    }\n    unreachable!()\n}\n'
  case_of maker-result-tested-below-is-a-place FAIL 'test-dirs unknown=crates/a/tests/t.rs places=1'

  repo maker-find
  put crates/a/tests/t.rs 'fn scratch() -> PathBuf {\n    (0..1_000)\n        .map(|n| base.join(format!("x-{}-{n}", std::process::id())))\n        .find(|dir| std::fs::create_dir(dir).is_ok())\n        .expect("a new directory")\n}\n'
  case_of maker-find PASS 'test-dirs files=1 places=0 known=0 faults=0'

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
  case_of maker-if-let-ok PASS 'test-dirs files=1 places=0 known=0 faults=0'

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
  *) fail usage ;;
esac
