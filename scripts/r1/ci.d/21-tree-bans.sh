#!/bin/sh
# jobs: macos windows
# rule: No package of the workspace links openssl-sys, and halvo, halvo-control and halvo-capture link no network client on the shipped targets.
set -u
name=21-tree-bans
clients='reqwest hyper ureq rustls ring aws-lc-rs native-tls openssl-sys'
triples='aarch64-apple-darwin x86_64-pc-windows-msvc'
nl='
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

judge() {
  printf '%s\n' "$tree" >"$CI_D_ARTIFACTS/$2.txt"
  lines=$(printf '%s\n' "$tree" | grep -c '')
  printf '%s: %s lines\n' "$1" "$lines"
  cause=$(predicate_tree_lines "$tree" "$3" "$4") ||
    fail "$1: ${cause:-predicate_tree_lines of scripts/r1-gate/lib.sh gave no cause}"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  [ -f scripts/r1-gate/lib.sh ] || fail "scripts/r1-gate/lib.sh is missing"
  GATE_ROOT=$CI_D_REPO
  export GATE_ROOT
  # shellcheck source=scripts/r1-gate/lib.sh disable=SC1091
  . scripts/r1-gate/lib.sh

  tree=$(cargo tree --workspace --target all --all-features --prefix none --locked) ||
    fail "cargo tree failed for the workspace"
  judge 'workspace on all targets' workspace halvo-capture openssl-sys

  metadata=$(cargo metadata --no-deps --locked --format-version 1) || fail "cargo metadata failed"
  members=$(printf '%s\n' "$metadata" | tr '{' '\n' | sed -n 's/^"name":"\([^"]*\)","version":.*/\1/p')
  for package in halvo halvo-control halvo-capture; do
    case $nl$members$nl in
      *"$nl$package$nl"*) ;;
      *)
        [ "$package" != halvo-control ] || continue
        fail "cargo metadata lists no package $package"
        ;;
    esac
    for triple in $triples; do
      tree=$(cargo tree -p "$package" -e normal --all-features --target "$triple" --prefix none --locked) ||
        fail "cargo tree failed for $package on $triple"
      judge "$package on $triple" "$package-$triple" "$package" "$clients"
    done
  done
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts
    STUB_TREES=$work/$1
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_TREES STUB_STALE
    measure 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"${3:-}"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$got"
  if [ "$got" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  STUB_STALE=no
}

trees() {
  mkdir -p "$work/$1" || exit 2
  cp "$work/main"/* "$work/$1/"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/artifacts" "$work/main" "$work/repo/scripts/r1-gate"
  cat >"$work/repo/scripts/r1-gate/lib.sh" <<'EOF'
predicate_tree_lines() {
  for required in $2; do
    printf '%s\n' "$1" | grep -q "^$required v" || {
      printf 'required line missing: %s v\n' "$required"
      return 1
    }
  done
  for forbidden in $3; do
    if line=$(printf '%s\n' "$1" | grep "^$forbidden v"); then
      printf 'forbidden crate line: %s\n' "$line" | sed 1q
      return 1
    fi
  done
  return 0
}
EOF
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'tree --workspace --target all --all-features --prefix none --locked')
    [ "$STUB_STALE" = no ] || exit 101
    cat "$STUB_TREES/workspace.txt"
    ;;
  'metadata --no-deps --locked --format-version 1') cat "$STUB_TREES/metadata.json" ;;
  *)
    if [ $# -eq 11 ] && [ "$1 $2 $4 $5 $6 $7 $9 ${10} ${11}" = 'tree -p -e normal --all-features --target --prefix none --locked' ]; then
      cat "$STUB_TREES/$3-$8.txt"
    else
      printf 'stub cargo: got the arguments: %s\n' "$*"
      exit 97
    fi
    ;;
esac
EOF
  chmod +x "$work/bin/cargo"
  printf 'halvo v0.0.0 (/w/src-tauri)\nhalvo-capture v0.0.0 (/w/crates/halvo-capture)\ntauri v2.12.5\nreqwest v0.13.5\nhyper v1.9.0\n' >"$work/main/workspace.txt"
  printf '{"packages":[{"name":"halvo","version":"0.0.0","id":"path+file:///w/src-tauri#halvo@0.0.0","dependencies":[{"name":"halvo-capture","source":null,"req":"*"}],"targets":[{"kind":["bin"],"crate_types":["bin"],"name":"halvo","src_path":"/w/src-tauri/src/main.rs"}]},{"name":"halvo-capture","version":"0.0.0","id":"path+file:///w/crates/halvo-capture#0.0.0","dependencies":[],"targets":[{"kind":["lib"],"crate_types":["lib"],"name":"halvo_capture","src_path":"/w/crates/halvo-capture/src/lib.rs"}]}],"version":1}\n' >"$work/main/metadata.json"
  for triple in $triples; do
    printf 'halvo v0.0.0 (/w/src-tauri)\nhalvo-capture v0.0.0 (/w/crates/halvo-capture)\ntauri v2.12.5\n' >"$work/main/halvo-$triple.txt"
    printf 'halvo-capture v0.0.0 (/w/crates/halvo-capture)\nrusqlite v0.41.1\n' >"$work/main/halvo-capture-$triple.txt"
  done
  rejected=0
  wrong=
  STUB_STALE=no

  trees what-main-links
  check what-main-links PASS 'halvo-capture on x86_64-pc-windows-msvc: 2 lines'
  trees stale-lockfile
  STUB_STALE=yes
  check stale-lockfile FAIL 'cargo tree failed for the workspace'
  trees package-renamed
  sed 's/"name":"halvo","version"/"name":"halvo-app","version"/' "$work/main/metadata.json" >"$work/package-renamed/metadata.json"
  check package-renamed FAIL 'cargo metadata lists no package halvo)'
  trees required-line-missing
  grep -v '^halvo-capture v' "$work/main/workspace.txt" >"$work/required-line-missing/workspace.txt"
  check required-line-missing FAIL 'workspace on all targets: required line missing: halvo-capture v)'
  trees forbidden-crate
  printf 'openssl-sys v0.9.111\n' >>"$work/forbidden-crate/workspace.txt"
  check forbidden-crate FAIL 'workspace on all targets: forbidden crate line: openssl-sys v0.9.111)'
  trees reqwest-line
  printf 'reqwest v0.13.5\n' >>"$work/reqwest-line/halvo-x86_64-pc-windows-msvc.txt"
  check reqwest-line FAIL 'halvo on x86_64-pc-windows-msvc: forbidden crate line: reqwest v0.13.5)'
  trees ring-in-the-capture-crate
  printf 'ring v0.17.14\n' >>"$work/ring-in-the-capture-crate/halvo-capture-aarch64-apple-darwin.txt"
  check ring-in-the-capture-crate FAIL 'halvo-capture on aarch64-apple-darwin: forbidden crate line: ring v0.17.14)'
  trees control-crate-links-hyper
  sed 's/{"name":"halvo-capture","version"/{"name":"halvo-control","version":"0.0.0","id":"path+file:\/\/\/w\/crates\/halvo-control#0.0.0","dependencies":[],"targets":[]},&/' "$work/main/metadata.json" >"$work/control-crate-links-hyper/metadata.json"
  printf 'halvo-control v0.0.0 (/w/crates/halvo-control)\n' >"$work/control-crate-links-hyper/halvo-control-aarch64-apple-darwin.txt"
  printf 'halvo-control v0.0.0 (/w/crates/halvo-control)\nhyper v1.9.0\n' >"$work/control-crate-links-hyper/halvo-control-x86_64-pc-windows-msvc.txt"
  check control-crate-links-hyper FAIL 'halvo-control on x86_64-pc-windows-msvc: forbidden crate line: hyper v1.9.0)'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *) exit 64 ;;
esac
