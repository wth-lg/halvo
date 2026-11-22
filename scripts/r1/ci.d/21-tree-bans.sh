#!/bin/sh
# jobs: macos windows
# rule: No package of the workspace links openssl-sys, and halvo, halvo-control, halvo-capture, halvo-agent-client, which relays a run, halvo-owner, which opens the derived store, halvo-analysis, which reads the capture records, and halvo-learn-store, which keeps what the agents learn, link no network client on the shipped targets; the feature test-seam stands in crates/halvo-capture/Cargo.toml alone, on its features line and on the crate's dev-dependency on itself, and in no other manifest (ruling W3-8 (b) (2)). halvo-runner, the program that runs a person's agents, reaches a closed list (its supervisor's frames; HTTPS to the origins of its agent's bindings through halvo-connect; the browser that it starts), and its tree on each shipped target holds no store crate (halvo-sqlcipher, halvo-capture, halvo-derived, halvo-owner, halvo-agent-store, halvo-coord-store and halvo-learn-store, the packages of the workspace that open a store, on disk or in memory, or keep its rows, one of which every other package that opens a store takes into its tree) and no network client (a crate of the list clients of this script) beyond those that the trees of halvo-connect and halvo-web hold on that target, by name and version. The tree of halvo-runner's package alone, read with features on each shipped target, holds no feature client of halvo-control: the client of the capture service, which can pause the capture and delete everything (a build of the runner with another member, such as --workspace, joins their features and compiles the client in).
set -u
name=21-tree-bans
clients='reqwest hyper ureq rustls ring aws-lc-rs native-tls openssl-sys'
stores='halvo-sqlcipher halvo-capture halvo-derived halvo-owner halvo-agent-store halvo-coord-store halvo-learn-store'
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

package_tree() {
  tree=$(cargo tree -p "$1" -e normal --all-features --target "$2" --prefix none --locked) ||
    fail "cargo tree failed for $1 on $2"
  judge "$1 on $2" "$1-$2" "$1" "$3"
}

client_beyond() {
  for client in $clients; do
    held=$(printf '%s\n' "$1" | sed -n "s/^\($client v[^ ]*\).*/\1/p")
    while IFS= read -r line; do
      [ -n "$line" ] || continue
      case $nl$2$nl in
        *"$nl$line$nl"* | *"$nl$line "*) ;;
        *)
          printf '%s\n' "$line"
          return 1
          ;;
      esac
    done <<EOF
$held
EOF
  done
}

client_tree() {
  tree=$(cargo tree -p halvo-runner -e features --all-features --target "$1" --prefix none --locked) ||
    fail "cargo tree with features failed for halvo-runner on $1"
  judge "halvo-runner with features on $1" "halvo-runner-features-$1" halvo-runner ''
  if line=$(printf '%s\n' "$tree" | grep '^halvo-control feature "client"'); then
    fail "halvo-runner on $1: its tree, read with features, holds the control client: $(printf '%s\n' "$line" | sed 1q)"
  fi
}

seam_manifests() {
  own=crates/halvo-capture/Cargo.toml
  manifests=$(git ls-files --cached --others --exclude-standard -- '*Cargo.toml') ||
    fail "git cannot list the manifests"
  for manifest in $manifests; do
    [ "$manifest" != "$own" ] || continue
    if grep -q 'test-seam' "$manifest"; then
      fail "the feature test-seam stands in another manifest: $manifest"
    fi
  done
  [ -f "$own" ] || fail "$own is missing"
  lines=$(grep -c 'test-seam' "$own")
  features=$(grep -c '^test-seam = \[\]$' "$own")
  itself=$(grep -c -E '^halvo-capture = \{ path = "\.", features = \[("[a-z0-9-]+", )*"test-seam"(, "[a-z0-9-]+")*\] \}$' "$own")
  [ "$lines" -eq 2 ] && [ "$features" -eq 1 ] && [ "$itself" -eq 1 ] ||
    fail "$own names test-seam on $lines lines, and not on its features line and its own dev-dependency line alone"
  printf 'manifests that name test-seam: 1 (%s, %s lines)\n' "$own" "$lines"
}

measure() {
  cd "$CI_D_REPO" || exit 2
  [ -f scripts/r1-gate/lib.sh ] || fail "scripts/r1-gate/lib.sh is missing"
  GATE_ROOT=$CI_D_REPO
  export GATE_ROOT
  . scripts/r1-gate/lib.sh

  seam_manifests

  tree=$(cargo tree --workspace --target all --all-features --prefix none --locked) ||
    fail "cargo tree failed for the workspace"
  judge 'workspace on all targets' workspace halvo-capture openssl-sys

  metadata=$(cargo metadata --no-deps --locked --format-version 1) || fail "cargo metadata failed"
  members=$(printf '%s\n' "$metadata" | tr '{' '\n' | sed -n 's/^"name":"\([^"]*\)","version":.*/\1/p')
  for package in halvo halvo-control halvo-capture halvo-agent-client halvo-owner halvo-analysis halvo-learn-store; do
    case $nl$members$nl in
      *"$nl$package$nl"*) ;;
      *)
        [ "$package" != halvo-control ] || continue
        fail "cargo metadata lists no package $package"
        ;;
    esac
    for triple in $triples; do
      package_tree "$package" "$triple" "$clients"
    done
  done
  for triple in $triples; do
    package_tree halvo-connect "$triple" ''
    allowed=$tree
    package_tree halvo-web "$triple" ''
    allowed=$allowed$nl$tree
    package_tree halvo-runner "$triple" "$stores"
    beyond=$(client_beyond "$tree" "$allowed") ||
      fail "halvo-runner on $triple: a network client beyond the trees of halvo-connect and halvo-web: $beyond"
    client_tree "$triple"
  done
  printf 'PASS %s\n' "$name"
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$repo
    CI_D_ARTIFACTS=$work/artifacts
    STUB_TREES=$work/$1
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_TREES STUB_STALE STUB_METADATA STUB_BROKEN
    sh "$0" 2>&1
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
  repo=$work/repo
  STUB_STALE=no
  STUB_METADATA=ok
  STUB_BROKEN=none
}

trees() {
  mkdir -p "$work/$1" || exit 2
  cp "$work/main"/* "$work/$1/"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/artifacts" "$work/main" "$work/repo/scripts/r1-gate" "$work/no-library"
  cat >"$work/repo/scripts/r1-gate/lib.sh" <<'EOF'
predicate_tree_lines() {
  [ "${GATE_ROOT:-}" = "$CI_D_REPO" ] || {
    printf 'GATE_ROOT is not the repo root\n'
    return 1
  }
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
[ -f fixture-root ] || exit 98
case $* in
  'tree --workspace --target all --all-features --prefix none --locked')
    [ "$STUB_STALE" = no ] || exit 101
    cat "$STUB_TREES/workspace.txt"
    ;;
  'metadata --no-deps --locked --format-version 1')
    [ "$STUB_METADATA" = ok ] || exit 101
    cat "$STUB_TREES/metadata.json"
    ;;
  *)
    if [ $# -eq 11 ] && [ "$1 $2 $4 $5 $6 $7 $9 ${10} ${11}" = 'tree -p -e normal --all-features --target --prefix none --locked' ]; then
      [ "$3-$8" != "$STUB_BROKEN" ] || exit 101
      cat "$STUB_TREES/$3-$8.txt"
    elif [ $# -eq 11 ] && [ "$1 $2 $4 $5 $6 $7 $9 ${10} ${11}" = 'tree -p -e features --all-features --target --prefix none --locked' ]; then
      cat "$STUB_TREES/$3-features-$8.txt"
    else
      printf 'stub cargo: got the arguments: %s\n' "$*"
      exit 97
    fi
    ;;
esac
EOF
  chmod +x "$work/bin/cargo"
  : >"$work/repo/fixture-root"
  git -C "$work/repo" init -q || exit 2
  mkdir -p "$work/repo/crates/halvo-capture" "$work/repo/src-tauri" || exit 2
  printf '[features]\nr2 = []\ntest-seam = []\n\n[dev-dependencies]\nhalvo-capture = { path = ".", features = ["test-seam"] }\n' >"$work/repo/crates/halvo-capture/Cargo.toml"
  printf '[dependencies]\nhalvo-capture = { path = "../crates/halvo-capture" }\n' >"$work/repo/src-tauri/Cargo.toml"
  printf 'halvo v0.0.0 (/w/src-tauri)\nhalvo-capture v0.0.0 (/w/crates/halvo-capture)\ntauri v2.12.5\nreqwest v0.13.5\nhyper v1.9.0\n' >"$work/main/workspace.txt"
  printf '{"packages":[{"name":"halvo","version":"0.0.0","id":"path+file:///w/src-tauri#halvo@0.0.0","dependencies":[{"name":"halvo-capture","source":null,"req":"*"}],"targets":[{"kind":["bin"],"crate_types":["bin"],"name":"halvo","src_path":"/w/src-tauri/src/main.rs"}]},{"name":"halvo-capture","version":"0.0.0","id":"path+file:///w/crates/halvo-capture#0.0.0","dependencies":[],"targets":[{"kind":["lib"],"crate_types":["lib"],"name":"halvo_capture","src_path":"/w/crates/halvo-capture/src/lib.rs"}]},{"name":"halvo-agent-client","version":"0.0.0","id":"path+file:///w/crates/halvo-agent-client#0.0.0","dependencies":[],"targets":[]},{"name":"halvo-owner","version":"0.0.0","id":"path+file:///w/crates/halvo-owner#0.0.0","dependencies":[],"targets":[]},{"name":"halvo-analysis","version":"0.0.0","id":"path+file:///w/crates/halvo-analysis#0.0.0","dependencies":[],"targets":[]},{"name":"halvo-learn-store","version":"0.0.0","id":"path+file:///w/crates/halvo-learn-store#0.0.0","dependencies":[],"targets":[]}],"version":1}\n' >"$work/main/metadata.json"
  for triple in aarch64-apple-darwin x86_64-pc-windows-msvc; do
    printf 'halvo v0.0.0 (/w/src-tauri)\nhalvo-capture v0.0.0 (/w/crates/halvo-capture)\ntauri v2.12.5\n' >"$work/main/halvo-$triple.txt"
    printf 'halvo-capture v0.0.0 (/w/crates/halvo-capture)\nrusqlite v0.41.1\n' >"$work/main/halvo-capture-$triple.txt"
    printf 'halvo-agent-client v0.0.0 (/w/crates/halvo-agent-client)\nhalvo-control v0.0.0 (/w/crates/halvo-control)\n' >"$work/main/halvo-agent-client-$triple.txt"
    printf 'halvo-owner v0.0.0 (/w/crates/halvo-owner)\nhalvo-derived v0.0.0 (/w/crates/halvo-derived)\n' >"$work/main/halvo-owner-$triple.txt"
    printf 'halvo-analysis v0.0.0 (/w/crates/halvo-analysis)\nhalvo-seam v0.0.0 (/w/crates/halvo-seam)\n' >"$work/main/halvo-analysis-$triple.txt"
    printf 'halvo-learn-store v0.0.0 (/w/crates/halvo-learn-store)\nhalvo-learn v0.0.0 (/w/crates/halvo-learn)\n' >"$work/main/halvo-learn-store-$triple.txt"
    printf 'halvo-connect v0.0.0 (/w/crates/halvo-connect)\nreqwest v0.13.5\nhyper v1.12.0\nrustls v0.23.46\nring v0.17.14\n' >"$work/main/halvo-connect-$triple.txt"
    printf 'halvo-web v0.0.0 (/w/crates/halvo-web)\nchromiumoxide v0.9.1\ntungstenite v0.28.0\n' >"$work/main/halvo-web-$triple.txt"
    printf 'halvo-runner v0.0.0 (/w/crates/halvo-runner)\nhalvo-connect v0.0.0 (/w/crates/halvo-connect)\nreqwest v0.13.5\nhyper v1.12.0 (*)\nrustls v0.23.46\nring v0.17.14 (*)\nhalvo-web v0.0.0 (/w/crates/halvo-web)\nchromiumoxide v0.9.1\ntungstenite v0.28.0\nhalvo-sheet v0.0.0 (/w/crates/halvo-sheet)\n' >"$work/main/halvo-runner-$triple.txt"
    printf 'halvo-runner v0.0.0 (/w/crates/halvo-runner)\nhalvo-control v0.0.0 (/w/crates/halvo-control)\nhalvo-connect feature "loopback"\nhalvo-connect v0.0.0 (/w/crates/halvo-connect)\nhalvo-runner feature "default" (command-line)\n' >"$work/main/halvo-runner-features-$triple.txt"
  done
  rejected=0
  wrong=
  repo=$work/repo
  STUB_STALE=no
  STUB_METADATA=ok
  STUB_BROKEN=none

  trees what-main-links
  check what-main-links PASS 'halvo-capture on x86_64-pc-windows-msvc: 2 lines'
  trees stale-lockfile
  STUB_STALE=yes
  check stale-lockfile FAIL 'cargo tree failed for the workspace)'
  trees metadata-fails
  STUB_METADATA=broken
  check metadata-fails FAIL 'cargo metadata failed)'
  trees package-tree-fails
  STUB_BROKEN=halvo-capture-x86_64-pc-windows-msvc
  check package-tree-fails FAIL 'cargo tree failed for halvo-capture on x86_64-pc-windows-msvc)'
  trees library-missing
  repo=$work/no-library
  check library-missing FAIL 'scripts/r1-gate/lib.sh is missing)'
  trees package-renamed
  sed 's/"name":"halvo","version"/"name":"halvo-app","version"/' "$work/main/metadata.json" >"$work/package-renamed/metadata.json"
  check package-renamed FAIL 'cargo metadata lists no package halvo)'
  trees capture-package-renamed
  sed 's/{"name":"halvo-capture","version"/{"name":"halvo-capture-x","version"/' "$work/main/metadata.json" >"$work/capture-package-renamed/metadata.json"
  check capture-package-renamed FAIL 'cargo metadata lists no package halvo-capture)'
  trees required-line-missing
  grep -v '^halvo-capture v' "$work/main/workspace.txt" >"$work/required-line-missing/workspace.txt"
  check required-line-missing FAIL 'workspace on all targets: required line missing: halvo-capture v)'
  trees package-tree-of-another-package
  printf 'another v0.0.0 (/w/crates/another)\n' >"$work/package-tree-of-another-package/halvo-capture-aarch64-apple-darwin.txt"
  check package-tree-of-another-package FAIL 'halvo-capture on aarch64-apple-darwin: required line missing: halvo-capture v)'
  trees forbidden-crate
  printf 'openssl-sys v0.9.111\n' >>"$work/forbidden-crate/workspace.txt"
  check forbidden-crate FAIL 'workspace on all targets: forbidden crate line: openssl-sys v0.9.111)'
  trees reqwest-line
  printf 'reqwest v0.13.5\n' >>"$work/reqwest-line/halvo-x86_64-pc-windows-msvc.txt"
  check reqwest-line FAIL 'halvo on x86_64-pc-windows-msvc: forbidden crate line: reqwest v0.13.5)'
  trees ring-in-the-capture-crate
  printf 'ring v0.17.14\n' >>"$work/ring-in-the-capture-crate/halvo-capture-aarch64-apple-darwin.txt"
  check ring-in-the-capture-crate FAIL 'halvo-capture on aarch64-apple-darwin: forbidden crate line: ring v0.17.14)'
  for client in reqwest hyper ureq rustls ring aws-lc-rs native-tls openssl-sys; do
    trees "$client-in-the-app"
    printf '%s v1.0.0\n' "$client" >>"$work/$client-in-the-app/halvo-aarch64-apple-darwin.txt"
    check "$client-in-the-app" FAIL "halvo on aarch64-apple-darwin: forbidden crate line: $client v1.0.0)"
  done
  trees seam-in-another-manifest
  repo=$work/seam-repo
  mkdir -p "$repo" || exit 2
  cp -R "$work/repo/." "$repo/"
  printf '[dependencies]\nhalvo-capture = { path = "../crates/halvo-capture", features = ["test-seam"] }\n' >"$repo/src-tauri/Cargo.toml"
  check seam-in-another-manifest FAIL 'the feature test-seam stands in another manifest: src-tauri/Cargo.toml)'
  trees seam-beside-another-feature-on-the-dev-dependency
  repo=$work/seam-beside-repo
  mkdir -p "$repo" || exit 2
  cp -R "$work/repo/." "$repo/"
  printf '[features]\nr2 = []\nstall-seam = []\ntest-seam = []\n\n[dev-dependencies]\nhalvo-capture = { path = ".", features = ["stall-seam", "test-seam"] }\n' >"$repo/crates/halvo-capture/Cargo.toml"
  check seam-beside-another-feature-on-the-dev-dependency PASS 'manifests that name test-seam: 1 (crates/halvo-capture/Cargo.toml, 2 lines)'
  trees seam-in-another-dependency-of-the-crate
  repo=$work/seam-other-dependency-repo
  mkdir -p "$repo" || exit 2
  cp -R "$work/repo/." "$repo/"
  printf '[features]\nr2 = []\ntest-seam = []\n\n[dev-dependencies]\nhalvo-capture = { path = ".", features = ["stall-seam"] }\nother = { path = "../other", features = ["test-seam"] }\n' >"$repo/crates/halvo-capture/Cargo.toml"
  check seam-in-another-dependency-of-the-crate FAIL 'crates/halvo-capture/Cargo.toml names test-seam on 2 lines, and not on its features line and its own dev-dependency line alone)'
  trees seam-as-a-default-feature
  repo=$work/seam-default-repo
  mkdir -p "$repo" || exit 2
  cp -R "$work/repo/." "$repo/"
  printf 'default = ["test-seam"]\n' >>"$repo/crates/halvo-capture/Cargo.toml"
  check seam-as-a-default-feature FAIL 'crates/halvo-capture/Cargo.toml names test-seam on 3 lines, and not on its features line and its own dev-dependency line alone)'
  trees control-crate-links-hyper
  sed 's/{"name":"halvo-capture","version"/{"name":"halvo-control","version":"0.0.0","id":"path+file:\/\/\/w\/crates\/halvo-control#0.0.0","dependencies":[],"targets":[]},&/' "$work/main/metadata.json" >"$work/control-crate-links-hyper/metadata.json"
  printf 'halvo-control v0.0.0 (/w/crates/halvo-control)\n' >"$work/control-crate-links-hyper/halvo-control-aarch64-apple-darwin.txt"
  printf 'halvo-control v0.0.0 (/w/crates/halvo-control)\nhyper v1.9.0\n' >"$work/control-crate-links-hyper/halvo-control-x86_64-pc-windows-msvc.txt"
  check control-crate-links-hyper FAIL 'halvo-control on x86_64-pc-windows-msvc: forbidden crate line: hyper v1.9.0)'
  trees reqwest-in-the-agent-client
  printf 'reqwest v0.13.5\n' >>"$work/reqwest-in-the-agent-client/halvo-agent-client-x86_64-pc-windows-msvc.txt"
  check reqwest-in-the-agent-client FAIL 'halvo-agent-client on x86_64-pc-windows-msvc: forbidden crate line: reqwest v0.13.5)'
  trees runner-links-a-store
  printf 'halvo-derived v0.0.0 (/w/crates/halvo-derived)\n' >>"$work/runner-links-a-store/halvo-runner-x86_64-pc-windows-msvc.txt"
  check runner-links-a-store FAIL 'halvo-runner on x86_64-pc-windows-msvc: forbidden crate line: halvo-derived v0.0.0 (/w/crates/halvo-derived))'
  trees runner-links-another-client
  printf 'ureq v3.1.2\n' >>"$work/runner-links-another-client/halvo-runner-aarch64-apple-darwin.txt"
  check runner-links-another-client FAIL 'halvo-runner on aarch64-apple-darwin: a network client beyond the trees of halvo-connect and halvo-web: ureq v3.1.2)'
  trees runner-links-the-control-client
  printf 'halvo-control feature "client"\nhalvo-control v0.0.0 (/w/crates/halvo-control) (*)\n' >>"$work/runner-links-the-control-client/halvo-runner-features-aarch64-apple-darwin.txt"
  check runner-links-the-control-client FAIL 'halvo-runner on aarch64-apple-darwin: its tree, read with features, holds the control client: halvo-control feature "client")'

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
