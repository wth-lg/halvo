#!/bin/sh
# jobs: macos windows
# rule: The bootstrap script pins cargo-release's version and three digests and passes every toolchain component that rust-toolchain.toml lists, no workflow file holds rustup component add, and .gitignore ignores the capture store files at the root of the checkout alone.
# lane: free
set -u
name=63-prerequisites

pinned_version=1.1.7
pinned_digests='aarch64_apple_darwin=39e1121c5e9671b0ff4703692b7ce2d9576c540118869091b7ad85a15187d129
x86_64_apple_darwin=9abd1bb84a581e982e4ae00a4c4413c14657b4cba8cd76b0eb003e00fe361a7a
x86_64_pc_windows_msvc=7b67996840ec6d2fd8e78c67281d8807950818869f6d09e0c9678897b48f9837'
needed_components='clippy rustfmt'
store_rules='/store.db*
/store.key
/create.pending
/writer.lock
/daemon.lock
/daemon-state.json
/delete-all.intent
/*.seg'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

lines_matching() {
  grep -c -e "$1" "$2" || :
}

defects() {
  root=$1
  script=$root/scripts/prerequisites.sh
  toolchain=$root/rust-toolchain.toml
  if [ ! -f "$script" ]; then
    printf 'problem: scripts/prerequisites.sh does not exist\n'
    return
  fi
  versions=$(lines_matching '^cargo_release_version=' "$script")
  [ "$versions" = 1 ] || printf 'problem: scripts/prerequisites.sh holds %s lines that start with cargo_release_version=, not 1\n' "$versions"
  grep -q -x "cargo_release_version=$pinned_version" "$script" ||
    printf 'problem: scripts/prerequisites.sh holds no line cargo_release_version=%s, the form that check 60 reads\n' "$pinned_version"
  pins=$(lines_matching '^cargo_release_sha256_' "$script")
  [ "$pins" = 3 ] || printf 'problem: scripts/prerequisites.sh holds %s lines that start with cargo_release_sha256_, not 3\n' "$pins"
  for pin in $pinned_digests; do
    grep -q -x "cargo_release_sha256_$pin" "$script" ||
      printf 'problem: scripts/prerequisites.sh does not pin cargo_release_sha256_%s to the digest that this check holds\n' "${pin%%=*}"
  done
  if [ -f "$toolchain" ]; then
    listed=$(sed -n 's/^components *= *\[\(.*\)\].*/\1/p' "$toolchain") || listed=
    listed=$(printf '%s\n' "$listed" | tr ',"' '  ')
  else
    printf 'problem: rust-toolchain.toml does not exist\n'
    listed=
  fi
  for component in $needed_components; do
    case " $listed " in
      *" $component "*) ;;
      *) printf 'problem: the components line of rust-toolchain.toml does not list %s\n' "$component" ;;
    esac
  done
  reads=$(lines_matching "^for component in \$(sed -n .s/^components \\*= \\*\\\\\\[" "$script")
  [ "$reads" = 1 ] || printf 'problem: scripts/prerequisites.sh holds %s loops that read the components line of rust-toolchain.toml, not 1\n' "$reads"
  flags=$(lines_matching "^  set -- \"\$@\" --component \"\$component\"\$" "$script")
  [ "$flags" = 1 ] || printf 'problem: scripts/prerequisites.sh holds %s lines that add a --component flag for each listed component, not 1\n' "$flags"
  installs=$(lines_matching "^rustup toolchain install \"\$@\"\$" "$script")
  [ "$installs" = 1 ] || printf 'problem: scripts/prerequisites.sh holds %s lines rustup toolchain install "$@", not 1\n' "$installs"
  if [ -f "$root/.gitignore" ]; then
    tr -d '\r' <"$root/.gitignore" >"$work/gitignore"
  else
    : >"$work/gitignore"
  fi
  printf '%s\n' "$store_rules" >"$work/store-rules"
  while IFS= read -r rule; do
    anchored=$(grep -c -x -F -e "$rule" "$work/gitignore") || anchored=0
    [ "$anchored" = 1 ] || printf 'problem: .gitignore holds %s lines %s, not 1\n' "$anchored" "$rule"
    loose=$(grep -c -x -F -e "${rule#/}" -e "**/${rule#/}" "$work/gitignore") || loose=0
    [ "$loose" = 0 ] || printf 'problem: .gitignore ignores %s at every depth, not at the root alone\n' "${rule#/}"
  done <"$work/store-rules"
  git -C "$root" ls-files -z -- .github/workflows >"$work/workflows" || {
    printf 'problem: git ls-files failed in the checkout\n'
    return
  }
  tr '\0' '\n' <"$work/workflows" >"$work/workflow-list"
  while IFS= read -r file; do
    grep -n -F 'rustup component add' "$root/$file" >"$work/found" || :
    while IFS=: read -r line _; do
      printf 'problem: %s:%s holds rustup component add; rust-toolchain.toml lists the components and scripts/prerequisites.sh installs them\n' "$file" "$line"
    done <"$work/found"
  done <"$work/workflow-list"
}

measure() {
  root=$1
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail "mktemp failed"
  defects "$root" >"$work/defects"
  cat "$work/defects"
  problems=$(lines_matching '^problem' "$work/defects")
  printf 'problems %s\n' "$problems"
  [ "$problems" = 0 ] || fail "$problems problems in the pins, the components or a workflow file"
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(CI_D_REPO=$work/$1 sh "$0" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
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
  elif [ "$2" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

good_script() {
  printf '#!/bin/sh\ncargo_release_version=%s\n' "$pinned_version"
  for pin in $pinned_digests; do
    printf 'cargo_release_sha256_%s\n' "$pin"
  done
  printf '%s\n' 'channel=1' "set -- \"\$channel\" --profile minimal --no-self-update" \
    "for component in \$(sed -n 's/^components *= *\\[\\(.*\\)\\].*/\\1/p' rust-toolchain.toml | tr ',\"' '  '); do" \
    "  set -- \"\$@\" --component \"\$component\"" 'done' "rustup toolchain install \"\$@\""
}

fixture() {
  mkdir -p "$work/$1/scripts" "$work/$1/.github/workflows" || exit 2
  good_script >"$work/$1/scripts/prerequisites.sh" || exit 2
  printf '[toolchain]\nchannel = "1.2.3"\nprofile = "minimal"\ncomponents = ["clippy", "rustfmt"]\n' >"$work/$1/rust-toolchain.toml" || exit 2
  printf 'name: ci\njobs:\n  macos:\n    steps:\n      - run: sh scripts/prerequisites.sh\n' >"$work/$1/.github/workflows/ci.yml" || exit 2
  printf '/target/\n%s\n' "$store_rules" >"$work/$1/.gitignore" || exit 2
}

track() {
  git -C "$work/$1" init -q || exit 2
  git -C "$work/$1" add -A || exit 2
}

edit() {
  sed "$2" "$work/$1/$3" >"$work/$1/$3.new" || exit 2
  mv "$work/$1/$3.new" "$work/$1/$3" || exit 2
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=

  fixture good
  track good
  check good PASS 'problems 0'

  fixture crlf-toolchain
  printf '[toolchain]\r\nchannel = "1.2.3"\r\ncomponents = ["clippy", "rustfmt"]\r\n' >"$work/crlf-toolchain/rust-toolchain.toml" || exit 2
  track crlf-toolchain
  check crlf-toolchain PASS 'problems 0'

  fixture untracked-workflow
  track untracked-workflow
  printf 'run: rustup component add clippy\n' >"$work/untracked-workflow/.github/workflows/scratch.yml" || exit 2
  check untracked-workflow PASS 'problems 0'

  fixture component-add
  printf '      - run: rustup component add clippy rustfmt\n' >>"$work/component-add/.github/workflows/ci.yml" || exit 2
  track component-add
  check component-add FAIL 'problem: .github/workflows/ci.yml:6 holds rustup component add'

  fixture yaml-workflow
  printf 'jobs:\n  lint:\n    steps:\n      - run: rustup component add rustfmt\n' >"$work/yaml-workflow/.github/workflows/lint.yaml" || exit 2
  track yaml-workflow
  check yaml-workflow FAIL 'problem: .github/workflows/lint.yaml:4 holds rustup component add'

  fixture pin-suffix
  edit pin-suffix 's/^\(cargo_release_sha256_aarch64_apple_darwin=.*\)$/\1x/' scripts/prerequisites.sh
  track pin-suffix
  check pin-suffix FAIL 'does not pin cargo_release_sha256_aarch64_apple_darwin to the digest'

  fixture crlf-gitignore
  edit crlf-gitignore 's/$/\r/' .gitignore
  track crlf-gitignore
  check crlf-gitignore PASS 'problems 0'

  fixture loose-store-key
  edit loose-store-key 's|^/store.key$|store.key|' .gitignore
  track loose-store-key
  check loose-store-key FAIL 'problem: .gitignore holds 0 lines /store.key, not 1'

  fixture store-key-at-every-depth
  printf '**/store.key\n' >>"$work/store-key-at-every-depth/.gitignore" || exit 2
  track store-key-at-every-depth
  check store-key-at-every-depth FAIL 'problem: .gitignore ignores store.key at every depth, not at the root alone'

  fixture no-segment-rule
  edit no-segment-rule '/^\/\*\.seg$/d' .gitignore
  track no-segment-rule
  check no-segment-rule FAIL 'problem: .gitignore holds 0 lines /*.seg, not 1'

  fixture second-workflow
  printf 'jobs:\n  lint:\n    steps:\n      - run: rustup component add rustfmt\n' >"$work/second-workflow/.github/workflows/lint.yml" || exit 2
  track second-workflow
  check second-workflow FAIL 'problem: .github/workflows/lint.yml:4 holds rustup component add'

  fixture digest-one-character
  edit digest-one-character 's/^cargo_release_sha256_x86_64_apple_darwin=9/cargo_release_sha256_x86_64_apple_darwin=8/' scripts/prerequisites.sh
  track digest-one-character
  check digest-one-character FAIL 'does not pin cargo_release_sha256_x86_64_apple_darwin to the digest'

  fixture two-pins
  edit two-pins '/^cargo_release_sha256_x86_64_pc_windows_msvc=/d' scripts/prerequisites.sh
  track two-pins
  check two-pins FAIL 'holds 2 lines that start with cargo_release_sha256_, not 3'

  fixture fourth-pin
  printf 'cargo_release_sha256_aarch64_unknown_linux_musl=5fe8\n' >>"$work/fourth-pin/scripts/prerequisites.sh" || exit 2
  track fourth-pin
  check fourth-pin FAIL 'holds 4 lines that start with cargo_release_sha256_, not 3'

  fixture other-version
  edit other-version 's/^cargo_release_version=.*/cargo_release_version=1.1.8/' scripts/prerequisites.sh
  track other-version
  check other-version FAIL 'holds no line cargo_release_version=1.1.7, the form that check 60 reads'

  fixture longer-version
  edit longer-version 's/^cargo_release_version=.*/cargo_release_version=1.1.70/' scripts/prerequisites.sh
  track longer-version
  check longer-version FAIL 'holds no line cargo_release_version=1.1.7, the form that check 60 reads'

  fixture quoted-version
  edit quoted-version 's/^cargo_release_version=.*/cargo_release_version="1.1.7"/' scripts/prerequisites.sh
  track quoted-version
  check quoted-version FAIL 'holds no line cargo_release_version=1.1.7, the form that check 60 reads'

  fixture second-version
  printf 'cargo_release_version=1.1.7\n' >>"$work/second-version/scripts/prerequisites.sh" || exit 2
  track second-version
  check second-version FAIL 'holds 2 lines that start with cargo_release_version=, not 1'

  fixture no-rustfmt
  edit no-rustfmt 's/^components = .*/components = ["clippy"]/' rust-toolchain.toml
  track no-rustfmt
  check no-rustfmt FAIL 'the components line of rust-toolchain.toml does not list rustfmt'

  fixture no-components
  edit no-components '/^components/d' rust-toolchain.toml
  track no-components
  check no-components FAIL 'the components line of rust-toolchain.toml does not list clippy'

  fixture no-component-flag
  edit no-component-flag '/--component/d' scripts/prerequisites.sh
  track no-component-flag
  check no-component-flag FAIL 'holds 0 lines that add a --component flag for each listed component, not 1'

  fixture components-not-read
  edit components-not-read 's/^for component in .*/for component in clippy rustfmt; do/' scripts/prerequisites.sh
  track components-not-read
  check components-not-read FAIL 'holds 0 loops that read the components line of rust-toolchain.toml, not 1'

  fixture install-without-flags
  edit install-without-flags "s/^rustup toolchain install .*/rustup toolchain install \"\$channel\" --profile minimal/" scripts/prerequisites.sh
  track install-without-flags
  check install-without-flags FAIL 'holds 0 lines rustup toolchain install "$@", not 1'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure "$CI_D_REPO" ;;
  *) exit 64 ;;
esac
