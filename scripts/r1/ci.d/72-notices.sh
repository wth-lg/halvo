#!/bin/sh
# jobs: macos
# rule: scripts/notices/generate.js, run twice for each of the triples aarch64-apple-darwin and x86_64-pc-windows-msvc, exits 0 and writes the same bytes both times, and its file names Zetetic, the holder of SQLCipher; the generator lists each crate that the app or a helper of packaging/helpers.d reaches over normal edges, past proc-macro crates and the members of the workspace, with the licence text files at the crate's root, or, for a crate that ships none, the standard text of each id of its licence field, and the texts that scripts/notices/config.json names, and it fails for a crate that ships no licence text file and names no licence, for a standard text that holds no line Copyright (c) <holder>, which it fills with the authors of the crate, and for a build-dependency whose name ends in -src that config.json does not name as compiled in.
set -u
name=72-notices
triples='aarch64-apple-darwin x86_64-pc-windows-msvc'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

# Runs the generator with the arguments after $1 and $2, its file into $1 and its standard error
# into $2; on a failure it prints the generator's last line.
generate_defect() {
  out=$1 err=$2
  shift 2
  node scripts/notices/generate.js "$@" >"$out" 2>"$err" && return 0
  status=$?
  cause=$(sed -n 's|^scripts/notices/generate\.js: ||p' "$err" | tail -n 1)
  printf '%s\n' "${cause:-the generator exited $status}"
  return 1
}

measure() {
  cd "$CI_D_REPO" || exit 2
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  for triple in $triples; do
    file=$work/$triple.txt
    found=$(generate_defect "$file" "$CI_D_ARTIFACTS/$triple.err" --triple "$triple") || fail "$triple: $found"
    found=$(generate_defect "$work/$triple.again" "$work/$triple.again.err" --triple "$triple") || fail "$triple, the second run: $found"
    cmp -s "$file" "$work/$triple.again" || fail "two runs for $triple wrote different bytes"
    zetetic=$(grep -i -c zetetic "$file")
    [ "$zetetic" -ge 1 ] || fail "the notices for $triple name no Zetetic, the holder of SQLCipher"
    cp "$file" "$CI_D_ARTIFACTS/THIRD-PARTY-NOTICES-$triple.txt" || fail "cp could not copy the notices for $triple to the artifacts"
    bytes=$(wc -c <"$file" | tr -d ' ')
    pieces=$(sed -n 's/^SOFTWARE OF OTHERS: //p' "$file")
    texts=$(sed -n 's/^LICENCE TEXTS: //p' "$file")
    standard=$(sed -n "s/^scripts\/notices\/generate\.js: $triple: \([0-9]*\) crates ship no licence text file.*/\1/p" "$CI_D_ARTIFACTS/$triple.err")
    printf '%s: bytes=%s pieces=%s texts=%s standard_text_crates=%s zetetic_lines=%s\n' "$triple" "$bytes" "$pieces" "$texts" "$standard" "$zetetic"
  done
  printf 'PASS %s\n' "$name"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  work=$(cd "$work" && pwd -P) || exit 2
  rejected=0
  wrong=
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then wrong="$wrong $1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
  }
  # The stand-in workspace: the app, which names a licence, one helper and halvo-sqlcipher are its
  # members; the app reaches dep-a, which ships a LICENSE, halvo-sqlcipher and a proc-macro crate;
  # the helper reaches bare, which ships no text file; halvo-sqlcipher build-depends on sqlcipher-src.
  ws=$work/ws
  mkdir -p "$ws/app" "$ws/helper" "$ws/store" "$ws/registry/dep-a" "$ws/registry/bare" "$ws/registry/sqlcipher-src/sqlcipher" "$ws/registry/a-macro" "$ws/packaging/helpers.d" "$work/tools" || exit 2
  for manifest in app helper store registry/dep-a registry/bare registry/sqlcipher-src registry/a-macro; do
    : >"$ws/$manifest/Cargo.toml" || exit 2
  done
  printf '%s\n' 'MIT License, the stand-in text of dep-a' >"$ws/registry/dep-a/LICENSE" || exit 2
  printf '%s\n' 'MIT License, the stand-in text of a-macro' >"$ws/registry/a-macro/LICENSE" || exit 2
  printf '%s\n' 'MIT License, the stand-in text of sqlcipher-src' >"$ws/registry/sqlcipher-src/LICENSE" || exit 2
  printf '%s\n' 'Copyright (c) ZETETIC LLC, a stand-in line' >"$ws/registry/sqlcipher-src/sqlcipher/LICENSE-sqlcipher" || exit 2
  printf '%s\n' package=stand-in-helper bin=stand-in-helper manifest=helper/Cargo.toml >"$ws/packaging/helpers.d/stand-in-helper" || exit 2
  # The stand-in metadata, with $1 as the licence field of bare.
  stand_in_metadata() {
    cat <<EOF
{"packages":[
{"id":"app","name":"stand-in-app","version":"0.0.0","license":"MIT","license_file":null,"authors":[],"manifest_path":"$ws/app/Cargo.toml","targets":[{"kind":["bin"]}]},
{"id":"helper","name":"stand-in-helper","version":"0.0.0","license":null,"license_file":null,"authors":[],"manifest_path":"$ws/helper/Cargo.toml","targets":[{"kind":["bin"]}]},
{"id":"store","name":"halvo-sqlcipher","version":"0.0.0","license":null,"license_file":null,"authors":[],"manifest_path":"$ws/store/Cargo.toml","targets":[{"kind":["lib"]}]},
{"id":"dep-a","name":"dep-a","version":"1.0.0","license":"MIT","license_file":null,"authors":["A. Author"],"manifest_path":"$ws/registry/dep-a/Cargo.toml","targets":[{"kind":["lib"]}]},
{"id":"bare","name":"bare","version":"0.1.0","license":$1,"license_file":null,"authors":["B. Author"],"manifest_path":"$ws/registry/bare/Cargo.toml","targets":[{"kind":["lib"]}]},
{"id":"src","name":"sqlcipher-src","version":"420.0.0","description":"The SQLCipher amalgamation, a stand-in","license":"MIT AND BSD-3-Clause","license_file":null,"authors":[],"manifest_path":"$ws/registry/sqlcipher-src/Cargo.toml","targets":[{"kind":["lib"]}]},
{"id":"macro","name":"a-macro","version":"1.0.0","license":"MIT","license_file":null,"authors":[],"manifest_path":"$ws/registry/a-macro/Cargo.toml","targets":[{"kind":["proc-macro"]}]}
],
"workspace_members":["app","helper","store"],
"resolve":{"nodes":[
{"id":"app","deps":[{"pkg":"dep-a","dep_kinds":[{"kind":null}]},{"pkg":"store","dep_kinds":[{"kind":null}]},{"pkg":"macro","dep_kinds":[{"kind":null}]}]},
{"id":"helper","deps":[{"pkg":"bare","dep_kinds":[{"kind":null}]}]},
{"id":"store","deps":[{"pkg":"src","dep_kinds":[{"kind":"build"}]}]},
{"id":"dep-a","deps":[]},{"id":"bare","deps":[]},{"id":"src","deps":[]},{"id":"macro","deps":[]}
]}}
EOF
  }
  stand_in_metadata '"MIT"' >"$work/metadata.json" || exit 2
  stand_in_metadata null >"$work/metadata-no-licence.json" || exit 2
  # A stand-in cargo: metadata prints the file that NOTICES_METADATA names.
  cat >"$work/tools/cargo" <<'SH' || exit 2
#!/bin/sh
[ "$1" = metadata ] || exit 101
cat "$NOTICES_METADATA"
SH
  chmod +x "$work/tools/cargo" || exit 2
  printf '%s\n' '{"app":"app/Cargo.toml","compiled":[{"crate":"sqlcipher-src","by":"halvo-sqlcipher","texts":[{"file":"sqlcipher/LICENSE-sqlcipher"}]}],"tracked":[]}' >"$work/config.json" || exit 2
  printf '%s\n' '{"app":"app/Cargo.toml","tracked":[]}' >"$work/config-without-the-build-dependency.json" || exit 2

  # Runs the generator on the stand-in workspace with the metadata $2, the configuration $3 and the
  # arguments after them.
  run_case() {
    case_name=$1 case_metadata=$2 case_config=$3
    shift 3
    case_out=$work/$case_name.txt
    output=$(
      NOTICES_METADATA=$case_metadata PATH=$work/tools:$PATH
      export NOTICES_METADATA PATH
      generate_defect "$case_out" "$work/$case_name.err" --triple stand-in-triple --repo "$ws" --config "$case_config" "$@"
    )
    code=$?
  }
  # A fixture is rejected when the generator fails with the line $4; arguments after it go to the generator.
  rejected_case() {
    case_label=$1 case_metadata_in=$2 case_config_in=$3 case_want=$4
    shift 4
    run_case "$case_label" "$case_metadata_in" "$case_config_in" "$@"
    got=unexpected-exit-$code
    [ "$code" = 0 ] && got=PASS
    [ "$code:$output" = "1:$case_want" ] && got=FAIL
    verdict "$case_label" FAIL "$got"
  }

  # Fixture 1: a configuration without the rule for the build-dependency sqlcipher-src.
  rejected_case config-without-the-build-dependency-rule "$work/metadata.json" "$work/config-without-the-build-dependency.json" \
    'halvo-sqlcipher build-depends on sqlcipher-src 420.0.0 (The SQLCipher amalgamation, a stand-in), whose name ends in -src, so its build may compile its sources in: name it in scripts/notices/config.json under "compiled", with its texts'

  # Fixture 2: a crate with no licence text file and no licence field.
  rejected_case crate-with-no-text-and-no-licence "$work/metadata-no-licence.json" "$work/config.json" \
    'bare 0.1.0 ships no licence text file and names no licence'

  # Fixture 7: a standard text with no line for the holder, for bare, which ships no text file.
  mkdir -p "$work/licences-without-a-holder" || exit 2
  printf '%s\n' 'MIT License, a stand-in text with no line for the holder' >"$work/licences-without-a-holder/MIT.txt" || exit 2
  rejected_case standard-text-without-a-holder-line "$work/metadata.json" "$work/config.json" \
    'the standard text MIT.txt holds no line "Copyright (c) <holder>", so it would name no holder for bare 0.1.0' \
    --licences "$work/licences-without-a-holder"

  # Case 5: the app, a member of the workspace that names a licence, is not listed, while dep-a is.
  run_case member-that-names-a-licence "$work/metadata.json" "$work/config.json"
  got=unexpected-exit-$code
  if [ "$code" = 0 ]; then
    got=PASS
    grep -q -x -F 'dep-a 1.0.0  MIT' "$case_out" || got=dep-a-not-listed
    grep -q '^stand-in-app' "$case_out" && got=member-listed
  fi
  verdict member-that-names-a-licence PASS "$got"

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
