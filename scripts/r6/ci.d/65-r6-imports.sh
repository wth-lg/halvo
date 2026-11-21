#!/bin/sh
# jobs: macos windows
# rule: The dev and the release route-gate that the job builds import no pair of a library and a symbol outside this script's closed list for the job's system (nm -u -m on macOS, llvm-readobj --coff-imports on Windows), and a canary that connects and starts a process, built in both profiles and read in the same way, is refused by its connection and its process start.
set -u
LC_ALL=C
export LC_ALL
name=65-r6-imports
macos_connection='_socket _connect'
macos_process='_posix_spawnp _fork _execvp'
windows_connection='WSASocketW connect'
windows_process='CreateProcessW'
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

help() {
  cat <<'EOF'
Usage: sh scripts/r6/ci.d/65-r6-imports.sh [--self-test | --help]

The import read of route-gate (contracts.md T6.1; SG-64, the unit U4.2i of the routing run).
In the job that CI_D_JOB names (the host's system when it is not set), the check builds route-gate
in the dev and in the release profile (cargo build --locked -p halvo-route-gate --bin route-gate)
and reads what each binary imports: nm -u -m on macOS, which names the library of each symbol, and
llvm-readobj --coff-imports on Windows, from the llvm-tools component of the toolchain (in CI the
check adds the component when the runner lacks it). Every pair of a library and a symbol must stand
on the closed list of the job's system, which this script holds; a new import fails the check until
a ruling adds its line.
The canary: a package of one file under the artifacts directory, whose main opens a TCP connection
and starts a process. The check builds it in both profiles with --locked into a target directory of
its own and reads it in the same way; it never runs it. It must be refused: its imports outside the
list must hold each connection word and each process word of the job's system (macOS: _socket
_connect, and _posix_spawnp _fork _execvp; Windows: WSASocketW connect, and CreateProcessW).
The lists are read into the shell before the first cargo command.
Prints:
  imports job=<macos|windows> reader=<nm|llvm-readobj> allowed=<n>
  imports binary=<route-gate|canary> profile=<dev|release> libraries=<n> symbols=<n> outside=<n>
with connection=<refused|through> process=<refused|through> on the canary's lines, and one line
  imports outside binary=route-gate profile=<p> library=<l> symbol=<s>
for each import of route-gate outside the list, at most 20 for a binary (a name with a character
other than a letter, a digit or one of _$.@?#+- is printed as its length), then PASS 65-r6-imports
or FAIL 65-r6-imports (<cause>).
Causes, the first of this list where two fit:
  setup    the repository or a directory cannot be entered or made
  usage    an argument that the check does not know
  host     CI_D_JOB names another system than the host's, or the host is neither macOS nor Git Bash
  build    a cargo build failed, or its JSON names no executable or more than one
  reader   no reader, a reader that exits with a status other than 0, or a listing with no import,
           with a line that the check cannot read, or (Windows) of no COFF file
  imports  route-gate imports a pair outside the closed list
  canary   the canary's imports outside the list lack a connection word or a process word
The standard output and the standard error of every cargo command, of rustup and of the reader go
to the artifacts directory, with the pairs that the check read.
--self-test runs the check itself with stand-ins for cargo, nm, llvm-readobj, rustc, rustup and
uname, on listings made from the closed lists, and counts the fixtures that it rejects.
The limits that stay:
1. It holds route-gate alone: halvo-route as the app links it needs its own hold before the app
   links it.
2. A library that a macOS binary loads and takes no symbol from, and a function that a binary finds
   at run time by its name (dlsym, which the list of macOS does not hold, or GetProcAddress, which
   the Rust standard library imports on Windows), do not show in a listing.
3. A system call that a binary makes without an imported symbol (inline assembly, which needs
   unsafe code; the roots of halvo-route-gate and halvo-route-wire forbid it, a third-party crate
   can hold it).
4. A listed symbol is held by its name and not by its arguments: a listed call that opens a path can
   open a path that the system serves over a network.
5. The lists hold what the toolchain and the system libraries of the hosted runners link; a build
   with another linker, SDK, target or flag can import otherwise, and the check reads the binaries
   of its own job.
6. The check finds cargo, nm and rustc through PATH, and llvm-readobj in the sysroot that rustc
   prints.
EOF
}

macos_list() {
  cat <<'EOF'
libSystem _CCRandomGenerateBytes
libSystem __NSGetArgc
libSystem __NSGetArgv
libSystem __Unwind_Backtrace
libSystem __Unwind_DeleteException
libSystem __Unwind_GetDataRelBase
libSystem __Unwind_GetIP
libSystem __Unwind_GetIPInfo
libSystem __Unwind_GetLanguageSpecificData
libSystem __Unwind_GetRegionStart
libSystem __Unwind_GetTextRelBase
libSystem __Unwind_RaiseException
libSystem __Unwind_Resume
libSystem __Unwind_SetGR
libSystem __Unwind_SetIP
libSystem ___error
libSystem __dyld_get_image_header
libSystem __dyld_get_image_name
libSystem __dyld_get_image_vmaddr_slide
libSystem __dyld_image_count
libSystem __tlv_atexit
libSystem __tlv_bootstrap
libSystem _abort
libSystem _bzero
libSystem _calloc
libSystem _close
libSystem _closedir
libSystem _dirfd
libSystem _dispatch_release
libSystem _dispatch_semaphore_create
libSystem _dispatch_semaphore_signal
libSystem _dispatch_semaphore_wait
libSystem _dup
libSystem _fcntl
libSystem _free
libSystem _fstat
libSystem _getcwd
libSystem _getenv
libSystem _getpid
libSystem _linkat
libSystem _lstat
libSystem _malloc
libSystem _memcmp
libSystem _memcpy
libSystem _memmove
libSystem _memset
libSystem _mmap
libSystem _mprotect
libSystem _munmap
libSystem _open
libSystem _opendir
libSystem _pause
libSystem _posix_memalign
libSystem _pthread_get_stackaddr_np
libSystem _pthread_get_stacksize_np
libSystem _pthread_mutex_destroy
libSystem _pthread_mutex_init
libSystem _pthread_mutex_lock
libSystem _pthread_mutex_trylock
libSystem _pthread_mutex_unlock
libSystem _pthread_mutexattr_destroy
libSystem _pthread_mutexattr_init
libSystem _pthread_mutexattr_settype
libSystem _pthread_self
libSystem _pthread_setname_np
libSystem _pthread_threadid_np
libSystem _read
libSystem _readdir
libSystem _realloc
libSystem _sigaction
libSystem _sigaltstack
libSystem _signal
libSystem _stat
libSystem _strerror_r
libSystem _strlen
libSystem _sysconf
libSystem _unlink
libSystem _write
libSystem _writev
libSystem dyld_stub_binder
EOF
}

windows_list() {
  cat <<'EOF'
KERNEL32.dll GetLastError
EOF
}

canary_main() {
  cat <<'EOF'
fn main() {
    let connected = std::net::TcpStream::connect("127.0.0.1:9").is_ok();
    let started = std::process::Command::new("imports-canary-no-such-program")
        .status()
        .is_ok();
    std::process::exit(i32::from(connected) + i32::from(started));
}
EOF
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel 2>/dev/null) || fail setup
  fi
  cd "$repo" 2>/dev/null || fail setup
}

job_of() {
  host=$(uname -s 2>/dev/null) || host=
  case $host in
    Darwin) host_job=macos ;;
    MINGW* | MSYS* | CYGWIN*) host_job=windows ;;
    *) host_job=none ;;
  esac
  job=${CI_D_JOB:-$host_job}
  case $job in
    macos | windows) ;;
    *) job=other ;;
  esac
  if [ "$job" != "$host_job" ]; then
    printf 'imports job=%s host=%s (the check reads the binaries that its host builds for its job)\n' "$job" "$host_job"
    fail host
  fi
}

write_canary() {
  canary_dir=$artifacts/canary
  mkdir -p "$canary_dir/src" || return 1
  printf '[package]\nname = "imports-canary"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n[workspace]\n' \
    >"$canary_dir/Cargo.toml" || return 1
  printf '# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = "imports-canary"\nversion = "0.0.0"\n' \
    >"$canary_dir/Cargo.lock" || return 1
  canary_main >"$canary_dir/src/main.rs"
}

readobj_in_sysroot() {
  sysroot=$(rustc --print sysroot 2>/dev/null) || return 1
  [ -n "$sysroot" ] || return 1
  for found in "$sysroot"/lib/rustlib/*/bin/llvm-readobj.exe "$sysroot"/lib/rustlib/*/bin/llvm-readobj; do
    if [ -f "$found" ]; then
      reader=$found
      return 0
    fi
  done
  return 1
}

find_reader() {
  if [ "$job" = macos ]; then
    reader_kind='nm'
    command -v nm >/dev/null 2>&1
    return
  fi
  reader_kind=llvm-readobj
  readobj_in_sysroot && return 0
  if [ "${CI:-}" = true ]; then
    if rustup component add llvm-tools >"$artifacts/rustup.txt" 2>&1; then
      printf 'imports reader=llvm-readobj component=added\n'
    else
      printf 'imports reader=llvm-readobj component=not-added\n'
    fi
  fi
  readobj_in_sysroot
}

count_lines() {
  imports_text=$1 awk 'BEGIN {
    n = split(ENVIRON["imports_text"], line, "\n")
    for (i = 1; i <= n; i++) if (line[i] != "") m++
    print m + 0
  }'
}

executable_of() {
  awk '
    index($0, "{\"reason\":\"compiler-artifact\",") != 1 { next }
    match($0, /"executable":"[^"]*"/) {
      n++
      path = substr($0, RSTART + 14, RLENGTH - 15)
      gsub(/\\\\/, "/", path)
    }
    END {
      if (n != 1) exit 1
      print path
    }
  ' "$1"
}

pairs_of() {
  awk -v kind="$reader_kind" '
    { sub(/\r$/, "") }
    kind == "nm" {
      i = 2
      if ($i == "weak") i++
      lib = $(i + 3)
      if ($1 != "(undefined)" || $i != "external" || NF != i + 3 || $(i + 2) != "(from" || lib !~ /^[^()]+[)]$/) {
        bad++
        next
      }
      sub(/[)]$/, "", lib)
      print lib " " $(i + 1)
      n++
      next
    }
    /^Format: COFF-/ { coff = 1 }
    $1 == "Name:" {
      lib = $0
      sub(/^[ \t]*Name: /, "", lib)
      next
    }
    $1 == "Symbol:" {
      sym = $0
      sub(/^[ \t]*Symbol: /, "", sym)
      if (lib == "" || !match(sym, / ?[(][0-9]+[)]$/)) {
        bad++
        next
      }
      number = substr(sym, RSTART)
      gsub(/[ ()]/, "", number)
      sym = substr(sym, 1, RSTART - 1)
      if (sym == "") sym = "#" number
      print lib " " sym
      n++
    }
    END {
      if (!NR) why = "listing=empty"
      else if (bad) why = "listing=unread lines=" bad
      else if (kind != "nm" && !coff) why = "listing=not-coff"
      else if (!n) why = "listing=empty"
      if (why != "") {
        print why > "/dev/stderr"
        exit 3
      }
    }
  ' "$1"
}

judge() {
  imports_allowed=$allowed imports_outside=$2 awk '
    BEGIN {
      n = split(ENVIRON["imports_allowed"], line, "\n")
      for (i = 1; i <= n; i++) if (line[i] != "") ok[line[i]] = 1
      outside_file = ENVIRON["imports_outside"]
    }
    {
      symbols++
      lib = $0
      sub(/ .*/, "", lib)
      if (!(lib in seen)) {
        seen[lib] = 1
        libraries++
      }
      if (!($0 in ok)) {
        print > outside_file
        outside++
      }
    }
    END { printf "%d %d %d\n", libraries, symbols, outside }
  ' "$1"
}

print_outside() {
  awk -v binary="$binary" -v profile="$profile" '
    function shown(s) {
      if (s ~ /^[A-Za-z0-9_$.@?#+-]+$/) return s
      return "(" length(s) " bytes)"
    }
    NR <= 20 {
      lib = $0
      sub(/ .*/, "", lib)
      sym = substr($0, length(lib) + 2)
      printf "imports outside binary=%s profile=%s library=%s symbol=%s\n", binary, profile, shown(lib), shown(sym)
    }
    END { if (NR > 20) printf "imports outside binary=%s profile=%s not-printed=%d\n", binary, profile, NR - 20 }
  ' "$1"
}

missing_of() {
  imports_words=$2 awk '
    {
      sym = $0
      sub(/^[^ ]* /, "", sym)
      seen[sym] = 1
    }
    END {
      n = split(ENVIRON["imports_words"], word, " ")
      for (i = 1; i <= n; i++) if (!(word[i] in seen)) m++
      print m + 0
    }
  ' "$1"
}

note() {
  case $1 in
    build) rank=1 ;;
    reader) rank=2 ;;
    imports) rank=3 ;;
    *) rank=4 ;;
  esac
  if [ "$rank" -lt "$first_rank" ]; then
    first=$1
    first_rank=$rank
  fi
}

build_one() {
  if [ "$binary" = route-gate ]; then
    set -- -p halvo-route-gate --bin route-gate
  else
    set -- --manifest-path "$canary_dir/Cargo.toml" --target-dir "$work/canary-target"
  fi
  if [ "$profile" = release ]; then
    set -- "$@" --release
  fi
  cargo build --locked "$@" --message-format=json >"$out.json" 2>"$out.err"
  status=$?
  if [ "$status" -ne 0 ]; then
    printf 'imports build binary=%s profile=%s exit=%s\n' "$binary" "$profile" "$status"
    return 1
  fi
  exe=$(executable_of "$out.json") || exe=
  if [ -z "$exe" ] || [ ! -f "$exe" ]; then
    printf 'imports build binary=%s profile=%s executable=unread\n' "$binary" "$profile"
    return 1
  fi
}

read_one() {
  listing=$out.imports
  if [ "$reader_kind" = nm ]; then
    nm -u -m "$exe" >"$listing" 2>"$listing.err"
  else
    "$reader" --coff-imports "$exe" >"$listing" 2>"$listing.err"
  fi
  status=$?
  if [ "$status" -ne 0 ]; then
    printf 'imports reader=%s binary=%s profile=%s exit=%s\n' "$reader_kind" "$binary" "$profile" "$status"
    return 1
  fi
  if ! pairs_of "$listing" >"$out.read" 2>"$out.why"; then
    why=listing=unread
    IFS= read -r why <"$out.why" || why=listing=unread
    case $why in
      listing=empty | listing=not-coff | "listing=unread lines="*) ;;
      *) why=listing=unread ;;
    esac
    printf 'imports reader=%s binary=%s profile=%s %s\n' "$reader_kind" "$binary" "$profile" "$why"
    return 1
  fi
  sort -u "$out.read" >"$out.pairs" || fail setup
}

one() {
  binary=$1
  profile=$2
  out=$artifacts/$binary-$profile
  if ! build_one; then
    note build
    return
  fi
  if ! read_one; then
    note reader
    return
  fi
  : >"$out.outside" || fail setup
  judge "$out.pairs" "$out.outside" >"$out.counts" || fail setup
  read -r libraries symbols outside <"$out.counts" || fail setup
  line="imports binary=$binary profile=$profile libraries=$libraries symbols=$symbols outside=$outside"
  if [ "$binary" = route-gate ]; then
    printf '%s\n' "$line"
    print_outside "$out.outside" || fail setup
    [ "$outside" -eq 0 ] || note imports
    return
  fi
  missing=$(missing_of "$out.outside" "$connection") || fail setup
  connection_word=refused
  [ "$missing" -eq 0 ] || connection_word=through
  missing=$(missing_of "$out.outside" "$process") || fail setup
  process_word=refused
  [ "$missing" -eq 0 ] || process_word=through
  printf '%s connection=%s process=%s\n' "$line" "$connection_word" "$process_word"
  if [ "$connection_word" = through ] || [ "$process_word" = through ]; then
    note canary
  fi
}

check() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" 2>/dev/null || fail setup
  job_of
  if [ "$job" = macos ]; then
    allowed=$(macos_list) || fail setup
    connection=$macos_connection
    process=$macos_process
  else
    allowed=$(windows_list) || fail setup
    connection=$windows_connection
    process=$windows_process
  fi
  entries=$(count_lines "$allowed") || fail setup
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null) || fail setup
  write_canary || fail setup
  if ! find_reader; then
    printf 'imports reader=%s found=no\n' "$reader_kind"
    fail reader
  fi
  printf 'imports job=%s reader=%s allowed=%s\n' "$job" "$reader_kind" "$entries"
  first=
  first_rank=9
  for each in dev release; do
    one route-gate "$each"
    one canary "$each"
  done
  [ -z "$first" ] || fail "$first"
  printf 'PASS %s\n' "$name"
}

self_failed() {
  printf 'FAIL self-test:%s (%s)\n' "$name" "$1"
  exit 1
}

stub() {
  cat >"$1" && chmod +x "$1"
}

render() {
  awk -v kind="$1" '
    kind == "nm" {
      printf "                 (undefined) external %s (from %s)\n", $2, $1
      next
    }
    NR == 1 {
      print "File: route-gate.exe"
      print "Format: COFF-x86-64"
      print "Arch: x86_64"
      print "AddressSize: 64bit"
    }
    $1 != lib {
      if (lib != "") print "}"
      lib = $1
      print "Import {"
      print "  Name: " lib
      print "  ImportLookupTableRVA: 0x1000"
      print "  ImportAddressTableRVA: 0x2000"
    }
    { print "  Symbol: " $2 " (0)" }
    END { if (kind != "nm" && lib != "") print "}" }
  ' "$2"
}

fixtures() {
  macos_list >"$work/macos.pairs" || self_failed setup
  windows_list >"$work/windows.pairs" || self_failed setup
  cp "$work/macos.pairs" "$work/macos-canary.pairs" || self_failed setup
  printf 'libSystem %s\n' _socket _connect _posix_spawnp _fork _execvp _waitpid >>"$work/macos-canary.pairs" ||
    self_failed setup
  cp "$work/windows.pairs" "$work/windows-canary.pairs" || self_failed setup
  printf '%s\n' 'WS2_32.dll WSASocketW' 'WS2_32.dll connect' 'KERNEL32.dll CreateProcessW' >>"$work/windows-canary.pairs" ||
    self_failed setup
  render nm "$work/macos.pairs" >"$work/macos.listing" || self_failed setup
  render nm "$work/macos-canary.pairs" >"$work/macos-canary.listing" || self_failed setup
  render readobj "$work/windows.pairs" >"$work/windows.listing" || self_failed setup
  render readobj "$work/windows-canary.pairs" >"$work/windows-canary.listing" || self_failed setup
}

stubs() {
  mkdir -p "$work/bin" "$work/repo" "$work/sysroot/lib/rustlib/stub/bin" || self_failed setup
  stub "$work/bin/cargo" <<'EOF' || self_failed setup
#!/bin/sh
[ "${1:-}" = build ] || exit 101
[ "${2:-}" = --locked ] || exit 101
binary=route-gate
profile=dev
for arg in "$@"; do
  case $arg in
    --manifest-path) binary=canary ;;
    --release) profile=release ;;
  esac
done
case " $* " in
  *" --message-format=json "*) ;;
  *) exit 101 ;;
esac
if [ "$binary" = route-gate ]; then
  case " $* " in
    *" -p halvo-route-gate --bin route-gate "*) ;;
    *) exit 101 ;;
  esac
fi
if [ -f "$STUB_DIR/build-fails" ] && grep -q -x -e "$binary-$profile" "$STUB_DIR/build-fails"; then
  exit 101
fi
mkdir -p "$STUB_DIR/built" || exit 101
printf '%s %s\n' "$binary" "$profile" >"$STUB_DIR/built/$binary-$profile" || exit 101
copies=1
[ ! -f "$STUB_DIR/json-twice" ] || copies=2
printf '{"reason":"compiler-artifact","package_id":"lib","executable":null,"fresh":false}\n'
while [ "$copies" -gt 0 ]; do
  printf '{"reason":"compiler-artifact","package_id":"bin","executable":"%s","fresh":false}\n' "$STUB_DIR/built/$binary-$profile"
  copies=$((copies - 1))
done
printf '{"reason":"build-finished","success":true}\n'
EOF
  stub "$work/bin/nm" <<'EOF' || self_failed setup
#!/bin/sh
[ "$#" -eq 3 ] || exit 2
[ "$1" = -u ] || exit 2
[ "$2" = -m ] || exit 2
read -r binary profile <"$3" || exit 2
cat "$STUB_DIR/listings/$binary-$profile" 2>/dev/null
[ ! -f "$STUB_DIR/reader-fails" ] || exit 1
EOF
  stub "$work/readobj" <<'EOF' || self_failed setup
#!/bin/sh
[ "$#" -eq 2 ] || exit 2
[ "$1" = --coff-imports ] || exit 2
read -r binary profile <"$2" || exit 2
cat "$STUB_DIR/listings/$binary-$profile" 2>/dev/null
[ ! -f "$STUB_DIR/reader-fails" ] || exit 1
EOF
  cp "$work/readobj" "$work/sysroot/lib/rustlib/stub/bin/llvm-readobj" || self_failed setup
  stub "$work/bin/uname" <<'EOF' || self_failed setup
#!/bin/sh
printf '%s\n' "$STUB_UNAME"
EOF
  stub "$work/bin/rustc" <<'EOF' || self_failed setup
#!/bin/sh
[ "$*" = '--print sysroot' ] || exit 1
printf '%s\n' "$STUB_SYSROOT"
EOF
  stub "$work/bin/rustup" <<'EOF' || self_failed setup
#!/bin/sh
printf '%s\n' "$*" >>"$STUB_DIR/rustup-calls"
[ "$*" = 'component add llvm-tools' ] || exit 1
[ -f "$STUB_DIR/rustup-adds" ] || exit 0
mkdir -p "$STUB_SYSROOT/lib/rustlib/stub/bin" || exit 1
cp "$STUB_READOBJ" "$STUB_SYSROOT/lib/rustlib/stub/bin/llvm-readobj"
EOF
}

begin() {
  case_name=$1
  case_job=$2
  k=$((k + 1))
  cdir=$work/case-$k
  mkdir -p "$cdir/listings" "$cdir/tmp" || self_failed setup
  kind='nm'
  case_uname=Darwin
  if [ "$case_job" = windows ]; then
    kind=readobj
    case_uname=MINGW64_NT-10.0-20348
  fi
  for each in dev release; do
    cp "$work/$case_job.listing" "$cdir/listings/route-gate-$each" || self_failed setup
    cp "$work/$case_job-canary.listing" "$cdir/listings/canary-$each" || self_failed setup
  done
  case_sysroot=$work/sysroot
  need_rustup=no
}

add() {
  printf '%s %s\n' "$2" "$3" >"$cdir/add.pairs" || self_failed setup
  render "$kind" "$cdir/add.pairs" >>"$cdir/listings/$1" || self_failed setup
}

drop() {
  listing=$cdir/listings/$1
  shift
  imports_words=" $* " awk '
    {
      for (i = 1; i < NF; i++) {
        if (($i == "external" || $i == "Symbol:") && index(ENVIRON["imports_words"], " " $(i + 1) " ")) next
      }
      print
    }
  ' "$listing" >"$listing.kept" || self_failed setup
  mv "$listing.kept" "$listing" || self_failed setup
}

word_of() {
  case $1 in
    "PASS $name") printf 'pass\n' ;;
    "FAIL $name ("*")")
      w=${1#"FAIL $name ("}
      printf '%s\n' "${w%)}"
      ;;
    *) printf 'other\n' ;;
  esac
}

expect() {
  want=$1
  text=$2
  shift 2
  (
    cd "$work/repo" || exit 2
    PATH=$work/bin:$PATH CI=true CI_D_REPO=$work/repo CI_D_ARTIFACTS=$cdir/artifacts CI_D_JOB=$case_job \
      STUB_DIR=$cdir STUB_UNAME=$case_uname STUB_SYSROOT=$case_sysroot STUB_READOBJ=$work/readobj TMPDIR=$cdir/tmp \
      sh "$self" "$@"
  ) >"$cdir/out" 2>"$cdir/err"
  status=$?
  got=$(tail -n 1 "$cdir/out") || got=
  verdict=right
  [ "$got" = "$want" ] || verdict=wrong
  case $want in
    PASS*) [ "$status" -eq 0 ] || verdict=wrong ;;
    *) [ "$status" -eq 1 ] || verdict=wrong ;;
  esac
  if [ -n "$text" ] && ! grep -q -E -e "$text" "$cdir/out"; then
    verdict=wrong
  fi
  if [ "$need_rustup" = yes ] && ! grep -q -x -F -e 'component add llvm-tools' "$cdir/rustup-calls" 2>/dev/null; then
    verdict=wrong
  fi
  want_word=$(word_of "$want") || want_word=other
  got_word=$(word_of "$got") || got_word=other
  printf 'case %s/%s want=%s got=%s exit=%s\n' "$name" "$case_name" "$want_word" "$got_word" "$status"
  if [ "$verdict" = wrong ]; then
    wrong="$wrong $case_name"
  elif [ "$want_word" != pass ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null) || self_failed setup
  stubs
  fixtures
  k=0
  rejected=0
  wrong=
  pass="PASS $name"

  begin macos-clean macos
  expect "$pass" 'profile=release .* connection=refused process=refused'

  begin macos-weak-import-reads macos
  awk 'NR == 1 { sub(/ external /, " weak external ") } { print }' "$cdir/listings/route-gate-dev" >"$cdir/weak" ||
    self_failed setup
  mv "$cdir/weak" "$cdir/listings/route-gate-dev" || self_failed setup
  expect "$pass" ''

  begin macos-dev-imports-one-symbol-more macos
  add route-gate-dev libSystem _kqueue
  expect "FAIL $name (imports)" '^imports outside binary=route-gate profile=dev library=libSystem symbol=_kqueue$'

  begin macos-release-imports-one-symbol-more macos
  add route-gate-release libSystem _dlopen
  expect "FAIL $name (imports)" '^imports outside binary=route-gate profile=release library=libSystem symbol=_dlopen$'

  begin macos-listed-symbol-from-another-library macos
  add route-gate-dev libFake _close
  expect "FAIL $name (imports)" 'library=libFake symbol=_close$'

  begin macos-longer-name-of-a-listed-symbol macos
  add route-gate-release libSystem _closefrom
  expect "FAIL $name (imports)" 'library=libSystem symbol=_closefrom$'

  begin macos-dev-canary-through macos
  drop canary-dev _socket _connect _posix_spawnp _fork _execvp
  expect "FAIL $name (canary)" 'binary=canary profile=dev .* connection=through process=through$'

  begin macos-release-canary-through macos
  drop canary-release _socket _connect _posix_spawnp _fork _execvp
  expect "FAIL $name (canary)" 'binary=canary profile=release .* connection=through process=through$'

  begin macos-canary-connection-through macos
  drop canary-dev _connect
  expect "FAIL $name (canary)" 'binary=canary profile=dev .* connection=through process=refused$'

  begin macos-canary-process-through macos
  drop canary-release _execvp
  expect "FAIL $name (canary)" 'binary=canary profile=release .* connection=refused process=through$'

  begin macos-empty-listing macos
  : >"$cdir/listings/route-gate-dev" || self_failed setup
  expect "FAIL $name (reader)" '^imports reader=nm binary=route-gate profile=dev listing=empty$'

  begin macos-unreadable-line macos
  printf 'garbage\n' >>"$cdir/listings/route-gate-release" || self_failed setup
  expect "FAIL $name (reader)" 'profile=release listing=unread lines=1$'

  begin macos-reader-fails macos
  : >"$cdir/reader-fails" || self_failed setup
  expect "FAIL $name (reader)" '^imports reader=nm binary=route-gate profile=dev exit=1$'

  begin macos-route-gate-build-fails macos
  printf 'route-gate-release\n' >"$cdir/build-fails" || self_failed setup
  expect "FAIL $name (build)" '^imports build binary=route-gate profile=release exit=101$'

  begin macos-canary-build-fails macos
  printf 'canary-dev\n' >"$cdir/build-fails" || self_failed setup
  expect "FAIL $name (build)" '^imports build binary=canary profile=dev exit=101$'

  begin macos-two-executables macos
  : >"$cdir/json-twice" || self_failed setup
  expect "FAIL $name (build)" 'profile=dev executable=unread$'

  begin macos-build-is-the-first-cause macos
  add route-gate-dev libSystem _kqueue
  printf 'canary-release\n' >"$cdir/build-fails" || self_failed setup
  expect "FAIL $name (build)" 'symbol=_kqueue$'

  begin windows-clean windows
  expect "$pass" 'profile=release .* connection=refused process=refused'

  begin windows-dev-imports-one-symbol-more windows
  add route-gate-dev KERNEL32.dll VirtualAllocEx
  expect "FAIL $name (imports)" '^imports outside binary=route-gate profile=dev library=KERNEL32[.]dll symbol=VirtualAllocEx$'

  begin windows-release-imports-by-ordinal windows
  printf 'Import {\n  Name: WS2_32.dll\n  Symbol:  (23)\n}\n' >>"$cdir/listings/route-gate-release" || self_failed setup
  expect "FAIL $name (imports)" 'profile=release library=WS2_32[.]dll symbol=#23$'

  begin windows-dev-canary-through windows
  drop canary-dev WSASocketW connect CreateProcessW
  expect "FAIL $name (canary)" 'binary=canary profile=dev .* connection=through process=through$'

  begin windows-release-canary-through windows
  drop canary-release WSASocketW connect CreateProcessW
  expect "FAIL $name (canary)" 'binary=canary profile=release .* connection=through process=through$'

  begin windows-reader-missing windows
  case_sysroot=$cdir/sysroot
  need_rustup=yes
  expect "FAIL $name (reader)" '^imports reader=llvm-readobj found=no$'

  begin windows-reader-added-in-ci windows
  case_sysroot=$cdir/sysroot
  need_rustup=yes
  : >"$cdir/rustup-adds" || self_failed setup
  expect "$pass" '^imports reader=llvm-readobj component=added$'

  begin windows-empty-listing windows
  : >"$cdir/listings/route-gate-release" || self_failed setup
  expect "FAIL $name (reader)" 'binary=route-gate profile=release listing=empty$'

  begin windows-listing-of-no-coff-file windows
  awk '!/^Format: /' "$cdir/listings/route-gate-dev" >"$cdir/no-format" || self_failed setup
  mv "$cdir/no-format" "$cdir/listings/route-gate-dev" || self_failed setup
  expect "FAIL $name (reader)" 'binary=route-gate profile=dev listing=not-coff$'

  begin windows-job-on-a-mac-host windows
  case_uname=Darwin
  expect "FAIL $name (host)" '^imports job=windows host=macos '

  begin unknown-argument macos
  expect "FAIL $name (usage)" '' --no-such-argument

  if [ -n "$wrong" ]; then
    self_failed "wrong verdict in:$wrong"
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $# in
  0) check ;;
  1)
    case $1 in
      --self-test) self_test ;;
      --help) help ;;
      *) fail usage ;;
    esac
    ;;
  *) fail usage ;;
esac
