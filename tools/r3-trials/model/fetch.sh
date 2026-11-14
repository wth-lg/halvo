#!/bin/sh
set -u

usage() {
  cat <<'EOF'
usage: sh tools/r3-trials/model/fetch.sh TOKEN
       sh tools/r3-trials/model/fetch.sh --cmake
       sh tools/r3-trials/model/fetch.sh --cmake-dir
       sh tools/r3-trials/model/fetch.sh --check
       sh tools/r3-trials/model/fetch.sh --self-test
       sh tools/r3-trials/model/fetch.sh --help

TOKEN        Fetches the weights file of the line "weights model=TOKEN ..." of
             weights.txt from the address of that line and checks its size and its
             SHA-256 against the line. On macOS the file is kept as
             ~/Library/Caches/halvo-trials/weights/<sha256>, outside every repository,
             and target/r3-weights/<file> of the repository is a symbolic link to it.
             On another system the file is target/r3-weights/<file> itself. A file that
             is there and matches is not fetched again.
--cmake      On macOS with no cmake on PATH: fetches the archive of the line of
             fetch.txt, checks its size and SHA-256, and unpacks it under
             target/r3-tools/ of the repository. Elsewhere it needs cmake on PATH.
--cmake-dir  Prints the directory of the cmake to build with. Fetches nothing.
--check      Checks the form of every line of weights.txt and fetch.txt. Fetches
             nothing.
--self-test  Runs the checks of this script on fixtures of its own. Fetches nothing.

Each request is made by curl under the client name "curl", over https only, with
no token, no cookie and no account. A redirect is followed, and the SHA-256 of the
bytes decides, not the host.

Exit 0: the work is done and every check holds. 1: a fetch failed or a file or a
line does not match, and the lines say which. 2: a refusal, one line
"REFUSED <cause>: <detail>" on standard error; the causes are usage and weights.
EOF
}

refuse() {
  printf 'REFUSED %s: %s\n' "$1" "$2" >&2
  exit 2
}

here=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P) || refuse usage "the directory of fetch.sh cannot be read"
root=$(CDPATH='' cd -- "$here/../../.." && pwd -P) || refuse usage "the root of the repository cannot be read"
cmake_tools=$root/target/r3-tools
system=$(uname -s) || refuse usage "uname -s failed"

is_count() {
  case $1 in
    '' | 0* | *[!0123456789]*) return 1 ;;
  esac
  return 0
}

is_hex() {
  [ "${#1}" -eq "$2" ] || return 1
  case $1 in
    *[!0123456789abcdef]*) return 1 ;;
  esac
  return 0
}

is_plain_name() {
  case $1 in
    '' | . | .. | *[!ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-]*) return 1 ;;
  esac
  return 0
}

is_token() {
  [ "${#1}" -le 32 ] || return 1
  case $1 in
    '' | none | synthetic | *[!abcdefghijklmnopqrstuvwxyz0123456789_]*) return 1 ;;
  esac
  return 0
}

# Sets w_model, w_group, w_file, w_sha256, w_bytes, w_url and w_spdx from one line of
# weights.txt, or sets why and returns 1.
parse_weights() {
  set -f
  # shellcheck disable=SC2086
  set -- $1
  set +f
  why=
  if [ $# -ne 8 ] || [ "$1" != weights ]; then
    why='not the form "weights model= group= file= sha256= bytes= url= spdx="'
    return 1
  fi
  case $2 in model=*) w_model=${2#model=} ;; *) why="second word is not model=" && return 1 ;; esac
  case $3 in group=*) w_group=${3#group=} ;; *) why="third word is not group=" && return 1 ;; esac
  case $4 in file=*) w_file=${4#file=} ;; *) why="fourth word is not file=" && return 1 ;; esac
  case $5 in sha256=*) w_sha256=${5#sha256=} ;; *) why="fifth word is not sha256=" && return 1 ;; esac
  case $6 in bytes=*) w_bytes=${6#bytes=} ;; *) why="sixth word is not bytes=" && return 1 ;; esac
  case $7 in url=*) w_url=${7#url=} ;; *) why="seventh word is not url=" && return 1 ;; esac
  case $8 in spdx=*) w_spdx=${8#spdx=} ;; *) why="eighth word is not spdx=" && return 1 ;; esac
  is_token "$w_model" || { why="model $w_model is no token" && return 1; }
  case $w_group in agents | routing) ;; *) why="group $w_group is not agents or routing" && return 1 ;; esac
  case $w_file in *.gguf) ;; *) why="file $w_file is no .gguf file" && return 1 ;; esac
  is_plain_name "$w_file" || { why="file $w_file is no plain file name" && return 1; }
  is_hex "$w_sha256" 64 || { why="sha256 is not 64 lowercase hex digits" && return 1; }
  is_count "$w_bytes" || { why="bytes $w_bytes is no positive integer" && return 1; }
  case $w_spdx in MIT | Apache-2.0) ;; *) why="spdx $w_spdx is not MIT or Apache-2.0, the licences allowed for weights" && return 1 ;; esac
  check_hub_url "$w_url" "$w_file" || return 1
}

# The address of a file in a repository of the host at a pinned commit:
# https://huggingface.co/<owner>/<repository>/resolve/<40 hex>/<file>.
check_hub_url() {
  case $1 in
    https://huggingface.co/*) ;;
    *) why="url is not an https address of huggingface.co" && return 1 ;;
  esac
  rest=${1#https://huggingface.co/}
  old_ifs=$IFS
  IFS=/
  set -f
  # shellcheck disable=SC2086
  set -- $rest "$2"
  set +f
  IFS=$old_ifs
  if [ $# -ne 6 ] || [ "$3" != resolve ]; then
    why="url is not <owner>/<repository>/resolve/<commit>/<file>"
    return 1
  fi
  if ! is_plain_name "$1" || ! is_plain_name "$2"; then
    why="url names no plain owner and repository"
    return 1
  fi
  is_hex "$4" 40 || { why="url pins no commit of 40 lowercase hex digits" && return 1; }
  [ "$5" = "$6" ] || { why="url names the file $5, not $6" && return 1; }
}

# Sets f_file, f_version, f_bytes, f_sha256 and f_url from the line of fetch.txt, or sets
# why and returns 1.
parse_fetch() {
  set -f
  # shellcheck disable=SC2086
  set -- $1
  set +f
  why=
  if [ $# -ne 6 ] || [ "$1" != fetch ]; then
    why='not the form "fetch file= version= bytes= sha256= url="'
    return 1
  fi
  case $2 in file=*) f_file=${2#file=} ;; *) why="second word is not file=" && return 1 ;; esac
  case $3 in version=*) f_version=${3#version=} ;; *) why="third word is not version=" && return 1 ;; esac
  case $4 in bytes=*) f_bytes=${4#bytes=} ;; *) why="fourth word is not bytes=" && return 1 ;; esac
  case $5 in sha256=*) f_sha256=${5#sha256=} ;; *) why="fifth word is not sha256=" && return 1 ;; esac
  case $6 in url=*) f_url=${6#url=} ;; *) why="sixth word is not url=" && return 1 ;; esac
  is_plain_name "$f_file" || { why="file $f_file is no plain file name" && return 1; }
  is_count "$f_bytes" || { why="bytes $f_bytes is no positive integer" && return 1; }
  is_hex "$f_sha256" 64 || { why="sha256 is not 64 lowercase hex digits" && return 1; }
  [ "$f_url" = "https://github.com/Kitware/CMake/releases/download/v$f_version/$f_file" ] ||
    { why="url is not the file of the release v$f_version of Kitware/CMake" && return 1; }
}

sha256_of() {
  if command -v shasum >/dev/null 2>&1; then
    sum=$(shasum -a 256 "$1") || return 1
  else
    sum=$(sha256sum "$1") || return 1
  fi
  printf '%s\n' "${sum%% *}"
}

# Returns 0 when FILE has BYTES bytes and the SHA-256 HEX.
matches() {
  [ -f "$1" ] || return 1
  got_bytes=$(wc -c <"$1") || return 1
  got_bytes=$(printf '%s' "$got_bytes" | tr -d ' ')
  [ "$got_bytes" = "$2" ] || return 1
  got_sha256=$(sha256_of "$1") || return 1
  [ "$got_sha256" = "$3" ]
}

# Fetches URL to DEST unless DEST matches BYTES and HEX already; sets state and seconds.
fetch_to() {
  dest=$1
  url=$2
  bytes=$3
  sha256=$4
  seconds=0
  if matches "$dest" "$bytes" "$sha256"; then
    state=present
    return 0
  fi
  mkdir -p "$(dirname -- "$dest")" || return 1
  started=$(date +%s)
  if ! curl -q -sS -f -L --proto '=https' --proto-redir '=https' -A curl -o "$dest.part" "$url"; then
    echo "fetch.sh: the fetch of $url failed; nothing was kept under the file's name" >&2
    return 1
  fi
  ended=$(date +%s)
  seconds=$((ended - started))
  if ! matches "$dest.part" "$bytes" "$sha256"; then
    echo "fetch.sh: the bytes of $url do not match: got bytes=$got_bytes sha256=${got_sha256:--}, want bytes=$bytes sha256=$sha256; they stay in a .part file that the next run replaces" >&2
    return 1
  fi
  mv -f "$dest.part" "$dest" || return 1
  state=fetched
}

weights_line_of() {
  found=
  while IFS= read -r line || [ -n "$line" ]; do
    case $line in
      "weights model=$1 "*) found=$line ;;
    esac
  done <"$here/weights.txt"
  [ -n "$found" ]
}

fetch_weights() {
  weights_line_of "$1" || refuse weights "weights.txt has no line for the model $1"
  parse_weights "$found" || refuse weights "the line of $1 in weights.txt: $why"
  link=$root/target/r3-weights/$w_file
  if [ "$system" = Darwin ]; then
    case ${HOME:-} in
      /*) ;;
      *) refuse usage "HOME is not an absolute path" ;;
    esac
    store=$HOME/Library/Caches/halvo-trials/weights
    fetch_to "$store/$w_sha256" "$w_url" "$w_bytes" "$w_sha256" || exit 1
    mkdir -p "$root/target/r3-weights" || exit 1
    ln -sfn "$store/$w_sha256" "$link" || exit 1
  else
    fetch_to "$link" "$w_url" "$w_bytes" "$w_sha256" || exit 1
  fi
  matches "$link" "$w_bytes" "$w_sha256" || {
    echo "fetch.sh: target/r3-weights/$w_file does not match its line after the fetch" >&2
    exit 1
  }
  echo "fetch model=$w_model state=$state bytes=$w_bytes sha256=$w_sha256 seconds=$seconds path=target/r3-weights/$w_file"
}

cmake_bin_dir() {
  printf '%s\n' "$cmake_tools/cmake-$f_version/cmake-$f_version-macos-universal/CMake.app/Contents/bin"
}

read_fetch_line() {
  IFS= read -r line <"$here/fetch.txt" || refuse usage "fetch.txt has no line"
  parse_fetch "$line" || refuse usage "the line of fetch.txt: $why"
}

fetch_cmake() {
  if command -v cmake >/dev/null 2>&1; then
    echo "cmake state=on-path"
    return 0
  fi
  [ "$system" = Darwin ] || {
    echo "fetch.sh: no cmake is on PATH, and this script brings cmake on macOS only" >&2
    exit 1
  }
  read_fetch_line
  fetch_to "$cmake_tools/$f_file" "$f_url" "$f_bytes" "$f_sha256" || exit 1
  bin=$(cmake_bin_dir)
  if [ ! -x "$bin/cmake" ]; then
    unpack=$(mktemp -d "$cmake_tools/unpack.XXXXXX") || exit 1
    tar -xzf "$cmake_tools/$f_file" -C "$unpack" || exit 1
    mv "$unpack" "$cmake_tools/cmake-$f_version" || exit 1
  fi
  "$bin/cmake" --version >/dev/null || exit 1
  echo "cmake state=$state version=$f_version bytes=$f_bytes sha256=$f_sha256 seconds=$seconds path=target/r3-tools/cmake-$f_version"
}

cmake_dir() {
  if on_path=$(command -v cmake); then
    dirname -- "$on_path"
    return 0
  fi
  read_fetch_line
  bin=$(cmake_bin_dir)
  [ -x "$bin/cmake" ] || {
    echo "fetch.sh: no cmake is on PATH or under target/r3-tools/; run sh tools/r3-trials/model/fetch.sh --cmake first" >&2
    exit 1
  }
  printf '%s\n' "$bin"
}

check_lines() {
  bad=0
  n=0
  seen=' '
  while IFS= read -r line || [ -n "$line" ]; do
    n=$((n + 1))
    if ! parse_weights "$line"; then
      echo "weights.txt line $n: $why"
      bad=$((bad + 1))
      continue
    fi
    case $seen in
      *" $w_model "*)
        echo "weights.txt line $n: a second line for the model $w_model"
        bad=$((bad + 1))
        ;;
    esac
    seen="$seen$w_model "
  done <"$here/weights.txt"
  [ "$n" -gt 0 ] || {
    echo "weights.txt has no line"
    bad=$((bad + 1))
  }
  m=0
  while IFS= read -r line || [ -n "$line" ]; do
    m=$((m + 1))
    parse_fetch "$line" || {
      echo "fetch.txt line $m: $why"
      bad=$((bad + 1))
    }
  done <"$here/fetch.txt"
  [ "$m" -eq 1 ] || {
    echo "fetch.txt has $m lines, not 1"
    bad=$((bad + 1))
  }
  echo "check weights_lines=$n fetch_lines=$m faults=$bad"
  [ "$bad" -eq 0 ]
}

self_test() {
  dir=$(mktemp -d "${TMPDIR:-/tmp}/r3-model-fetch.XXXXXX") || exit 1
  hex64=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef
  hex40=0123456789abcdef0123456789abcdef01234567
  good="weights model=m_one group=agents file=m-Q4_K_M.gguf sha256=$hex64 bytes=10 url=https://huggingface.co/pub/m-GGUF/resolve/$hex40/m-Q4_K_M.gguf spdx=Apache-2.0"
  passed=0
  failed=0
  expect() {
    if [ "$1" = "$2" ]; then
      passed=$((passed + 1))
    else
      failed=$((failed + 1))
      echo "self-test: $3: got $2, want $1"
    fi
  }
  parse_weights "$good" && got=ok || got=refused
  expect ok "$got" "a good line"
  rejected=0
  for bad in \
    "${good% spdx=*} spdx=GPL-3.0" \
    "${good% spdx=*} spdx=CC-BY-4.0" \
    "$(printf '%s' "$good" | sed 's|huggingface.co/pub|example.com/pub|')" \
    "$(printf '%s' "$good" | sed "s|resolve/$hex40|resolve/main|")" \
    "$(printf '%s' "$good" | sed 's|resolve/\(.*\)/m-Q4_K_M.gguf|resolve/\1/other.gguf|')" \
    "$(printf '%s' "$good" | sed 's|sha256=0123|sha256=ABCD|')" \
    "$(printf '%s' "$good" | sed 's|sha256=0123|sha256=123|')" \
    "$(printf '%s' "$good" | sed 's|bytes=10|bytes=010|')" \
    "$(printf '%s' "$good" | sed 's|group=agents|group=other|')" \
    "$(printf '%s' "$good" | sed 's|model=m_one|model=none|')" \
    "$(printf '%s' "$good" | sed 's|model=m_one|model=M-One|')" \
    "$(printf '%s' "$good" | sed 's|file=m-Q4_K_M.gguf|file=m-Q4_K_M.bin|')" \
    "$(printf '%s' "$good" | sed 's| spdx=Apache-2.0||')" \
    "$(printf '%s' "$good" | sed 's|^weights|weight|')"; do
    if parse_weights "$bad"; then
      echo "self-test: a bad line passed: $bad"
      failed=$((failed + 1))
    else
      rejected=$((rejected + 1))
    fi
  done
  expect 14 "$rejected" "bad weights lines rejected"
  parse_fetch "fetch file=c.tar.gz version=1.2.3 bytes=5 sha256=$hex64 url=https://github.com/Kitware/CMake/releases/download/v1.2.3/c.tar.gz" && got=ok || got=refused
  expect ok "$got" "a good fetch line"
  parse_fetch "fetch file=c.tar.gz version=1.2.3 bytes=5 sha256=$hex64 url=https://example.com/v1.2.3/c.tar.gz" && got=ok || got=refused
  expect refused "$got" "a fetch line of another host"
  printf 'abc' >"$dir/f"
  abc=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
  matches "$dir/f" 3 "$abc" && got=ok || got=refused
  expect ok "$got" "a file that matches"
  matches "$dir/f" 3 "$hex64" && got=ok || got=refused
  expect refused "$got" "a file with another SHA-256"
  matches "$dir/f" 4 "$abc" && got=ok || got=refused
  expect refused "$got" "a file with another size"
  matches "$dir/missing" 3 "$abc" && got=ok || got=refused
  expect refused "$got" "a file that is missing"
  if [ "$failed" -eq 0 ]; then
    echo "PASS self-test: fetch.sh ($passed checks, $rejected bad lines rejected)"
    return 0
  fi
  echo "FAIL self-test: fetch.sh ($failed checks missed)"
  return 1
}

[ $# -eq 1 ] || refuse usage "fetch.sh takes one argument; see --help"
case $1 in
  --help) usage ;;
  --cmake) fetch_cmake ;;
  --cmake-dir) cmake_dir ;;
  --check) check_lines ;;
  --self-test) self_test ;;
  -*) refuse usage "unknown option $1" ;;
  *)
    is_token "$1" || refuse usage "$1 is no model token"
    fetch_weights "$1"
    ;;
esac
