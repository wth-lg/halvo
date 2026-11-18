#!/bin/sh
# jobs: macos windows
# rule: Every workspace package reports one version in cargo metadata, no workflow and no ci.d script compares --version output with a literal on one line (a compare in either order, or a case over that output whose literal pattern stands on the same line; a literal held in a variable is not seen), on a tag ref the tag is v<that version>, each binary given to --check exits 0 and prints that version, and every tracked Cargo.toml and package.json is read for that version (the root workspace through cargo metadata, the root package.json holding that version or none) or named in this script with the reason that it is left, the other side of the wall of 24-r7-wall by its top directory alone.
set -u
name=61-version

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

version_from() {
  members=$(tr -d '\n' <"$1" | sed -n 's/.*"workspace_members":\[\([^]]*\)\].*/\1/p')
  [ -n "$members" ] || { printf 'cargo metadata lists no workspace member\n'; return 1; }
  member_count=$(printf '%s\n' "$members" | tr ',' '\n' | grep -c '"')
  packages=$(tr -d '\n' <"$1" | grep -o -E '\{"name":"[^"]*","version":"[^"]*","id":"' | sed 's/^{"name":"\([^"]*\)","version":"\([^"]*\)".*/\1 \2/')
  package_count=$(printf '%s\n' "$packages" | grep -c ' ')
  [ "$package_count" -eq "$member_count" ] || {
    printf 'cargo metadata lists %s workspace members and this script reads %s packages\n' "$member_count" "$package_count"
    return 1
  }
  versions=$(printf '%s\n' "$packages" | sed 's/^[^ ]* //' | sort -u)
  version_count=$(printf '%s\n' "$versions" | grep -c .)
  if [ "$version_count" -ne 1 ]; then
    lowest=$(printf '%s\n' "$versions" | head -n 1)
    others=$(printf '%s\n' "$packages" | awk -v first="$lowest" '$2 != first { printf "%s%s=%s", sep, $1, $2; sep = " " }')
    printf 'the workspace packages report %s versions; packages that differ from the lowest: %s\n' "$version_count" "$others"
    return 1
  fi
  case $versions in
    *[!0-9A-Za-z.+-]*|'') printf 'the version is not a semver string\n'; return 1 ;;
  esac
  printf '%s\n' "$versions"
}

literal_compares() {
  for file in "$@"; do
    grep -n -E \
      -e '--version[^#]*(=|-eq|-ne)[[:space:]]*["'\'']?v?[0-9]+\.[0-9]+\.[0-9]+' \
      -e '["'\'']?v?[0-9]+\.[0-9]+\.[0-9]+["'\'']?[[:space:]]*(=|!=|-eq|-ne)[^#]*--version' \
      -e 'case[[:space:]][^#]*--version[^#]*[[:space:]]in[[:space:]]+["'\'']?v?[0-9]+\.[0-9]+\.[0-9]+' \
      "$file" | sed "s|^\([0-9]*\):.*|$file:\1|"
  done
}

tag_defect() {
  [ "${GITHUB_REF_TYPE:-}" = tag ] || return 0
  [ "${GITHUB_REF_NAME:-}" = "v$1" ] && return 0
  printf 'the tag of this run is not v%s, the derived version\n' "$1"
  return 1
}

binary_defect() {
  [ -f "$1" ] || { printf '%s does not exist\n' "$2"; return 1; }
  printed=$("$1" --version 2>/dev/null)
  status=$?
  [ "$status" -eq 0 ] || { printf '%s --version exited %s\n' "$2" "$status"; return 1; }
  [ "$printed" = "$3" ] || { printf '%s --version does not print the derived version\n' "$2"; return 1; }
}

left_manifests() {
  cat <<'EOF'
account account/Cargo.toml
account account/crates/halvo-account/Cargo.toml
account account/crates/halvo-secrets/Cargo.toml
account account/crates/halvo-test-dir/Cargo.toml
account account/crates/halvo-wire/Cargo.toml
account account/stub/package.json
tool tools/fixture-server/package.json
tool tools/r2-probes/Cargo.toml
tool tools/r2-probes/input/package.json
tool tools/r3-trials/drive/Cargo.toml
tool tools/r3-trials/model/Cargo.toml
tool tools/r3-trials/profile/Cargo.toml
tool tools/r3-trials/sheet/Cargo.toml
tool tools/r3-trials/web/Cargo.toml
tool tools/r4-page/package.json
tool tools/r4-trials/app/Cargo.toml
tool tools/r4-trials/driver/Cargo.toml
tool tools/r4-trials/driver/wdio/package.json
tool tools/r5-trials/Cargo.toml
tool tools/r5-trials/drift/Cargo.toml
tool tools/r5-trials/routine/Cargo.toml
tool tools/r5-trials/stat/Cargo.toml
wall backend
EOF
}

left_reason() {
  case $1 in
    account) printf 'the account workspace, which the root workspace excludes and no package of the app depends on\n' ;;
    tool) printf 'trial and probe tools of the research runs, which no package of the app depends on\n' ;;
    wall) printf 'every manifest under the other side of the wall of 24-r7-wall, which an app file names by its top directory alone and no package of the app depends on\n' ;;
    *) return 1 ;;
  esac
}

member_manifests() {
  root=$(tr -d '\n' <"$1" | grep -o '"workspace_root":"[^"]*"' | sed 's/^"workspace_root":"//; s/"$//')
  [ -n "$root" ] || { printf 'cargo metadata names no workspace root\n'; return 1; }
  paths=$(tr -d '\n' <"$1" | grep -o '"manifest_path":"[^"]*"' | sed 's/^"manifest_path":"//; s/"$//')
  member_total=$(tr -d '\n' <"$1" | sed -n 's/.*"workspace_members":\[\([^]]*\)\].*/\1/p' | tr ',' '\n' | grep -c '"')
  path_count=$(printf '%s\n' "$paths" | grep -c .)
  [ "$path_count" -eq "$member_total" ] || {
    printf 'cargo metadata lists %s workspace members and this script reads %s manifest paths\n' "$member_total" "$path_count"
    return 1
  }
  relative=
  while IFS= read -r path; do
    below=${path#"$root"}
    case $below in
      "$path"|[!/\\]*) printf 'cargo metadata names a member manifest outside the workspace root\n'; return 1 ;;
    esac
    below=$(printf '%s\n' "$below" | sed 's|\\\\|/|g; s|^/||')
    relative="$relative$below
"
  done <<EOF
$paths
EOF
  printf '%s' "$relative"
}

package_version() {
  awk '{ text = text $0 "\n" }
    END {
      n = length(text); depth = 0; found = 0; value = ""; i = 1
      while (i <= n) {
        c = substr(text, i, 1)
        if (c == "\"") {
          j = i + 1; word = ""
          while (j <= n && substr(text, j, 1) != "\"") {
            if (substr(text, j, 1) == "\\") { word = word substr(text, j, 2); j += 2 } else { word = word substr(text, j, 1); j++ }
          }
          if (j > n) { print "unreadable"; exit }
          k = j + 1
          while (k <= n && substr(text, k, 1) ~ /[ \t\r\n]/) k++
          if (depth == 1 && substr(text, k, 1) == ":" && index(word, "\\") > 0) { print "unreadable"; exit }
          if (depth == 1 && word == "version" && substr(text, k, 1) == ":") {
            k++
            while (k <= n && substr(text, k, 1) ~ /[ \t\r\n]/) k++
            if (substr(text, k, 1) != "\"") { print "unreadable"; exit }
            j = k + 1; value = ""
            while (j <= n && substr(text, j, 1) != "\"") {
              if (substr(text, j, 1) == "\\") { value = value substr(text, j, 2); j += 2 } else { value = value substr(text, j, 1); j++ }
            }
            if (j > n) { print "unreadable"; exit }
            found++
          }
          i = j + 1
          continue
        }
        if (c == "{" || c == "[") depth++
        else if (c == "}" || c == "]") depth--
        i++
      }
      if (found > 1) print "unreadable"
      else if (found == 1) print "value " value
      else print "none"
    }' "$1"
}

manifest_defect() {
  problems=
  while IFS=' ' read -r reason path; do
    [ -n "$reason$path" ] || continue
    left_reason "$reason" >/dev/null || problems="$problems; the line '$reason $path' gives a reason that this script does not give"
  done <"$3"
  listed=$(awk '
    function reads(n) { return (n in member) || n == "Cargo.toml" || n == "package.json" }
    FILENAME == ARGV[1] { if ($0 != "") member[$0] = 1; next }
    FILENAME == ARGV[2] {
      if ($0 == "") next
      at = index($0, " ")
      n = at ? substr($0, at + 1) : ""
      if (n == "") { out = out "; the line " $0 " names no manifest"; next }
      if ($1 == "wall") {
        if (n ~ /\//) out = out "; the wall line " n " names more than one top directory"
        if (n in wall) out = out "; " n " is named twice"
        wall[n] = 1; worder[++wcount] = n; next
      }
      if (n in named) out = out "; " n " is named twice"
      named[n] = 1; order[++count] = n; next
    }
    $0 != "" { tracked[$0] = 1; seen[++tcount] = $0 }
    END {
      for (i = 1; i <= count; i++) {
        n = order[i]
        if (!(n in tracked)) out = out "; " n " is named and the repository does not track it"
        else if (reads(n)) out = out "; " n " is named and this check reads its version"
      }
      for (i = 1; i <= tcount; i++) {
        t = seen[i]
        top = substr(t, 1, index(t "/", "/") - 1)
        if (top in wall) {
          covered[top]++
          if (reads(t)) out = out "; " t " is under a wall line and this check reads its version"
          continue
        }
        if (reads(t) || (t in named)) continue
        out = out "; " t " is neither read by this check nor named with a reason"
      }
      for (i = 1; i <= wcount; i++) if (!(worder[i] in covered)) out = out "; the wall line " worder[i] " covers no tracked manifest"
      printf "%s", out
    }' "$2" "$3" "$1")
  problems="$problems$listed"
  if grep -q -x 'package.json' "$1"; then
    read_value=$(package_version "$4")
    case $read_value in
      none) ;;
      "value $5") ;;
      "value "*) problems="$problems; package.json holds the version ${read_value#value } and the derived version is $5" ;;
      *) problems="$problems; package.json holds a version that this check cannot read" ;;
    esac
  fi
  [ -z "$problems" ] && return 0
  printf '%s\n' "${problems#; }"
  return 1
}

derive() {
  root=${CI_D_REPO:-}
  [ -n "$root" ] || root=$(git rev-parse --show-toplevel) || exit 2
  cd "$root" || exit 2
  PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  before=$(git status --porcelain) || fail "git status failed"
  if ! cargo metadata --locked --no-deps --format-version 1 >"$work/metadata.json" 2>"$work/metadata.err"; then
    cause=$(grep -m 1 '^error' "$work/metadata.err")
    fail "cargo metadata --locked --no-deps failed: $cause"
  fi
  after=$(git status --porcelain) || fail "git status failed"
  [ "$before" = "$after" ] || fail "a tracked file differs after cargo metadata"
  found=$(version_from "$work/metadata.json") || fail "$found"
  version=$found
}

measure() {
  derive
  files=$(git ls-files -- .github/workflows 'scripts/*/ci.d/*.sh') || fail "git ls-files failed"
  [ -n "$files" ] || fail "git ls-files lists no workflow and no ci.d script"
  # shellcheck disable=SC2086
  compares=$(literal_compares $files)
  places=$(printf '%s\n' "$compares" | tr '\n' ' ')
  [ -z "$compares" ] || fail "--version output compared with a literal at $places"
  found=$(tag_defect "$version") || fail "$found"
  member_paths=$(member_manifests "$work/metadata.json") || fail "$member_paths"
  printf '%s\n' "$member_paths" >"$work/members"
  git ls-files -z >"$work/tracked.z" || fail "git ls-files failed"
  LC_ALL=C tr '\0' '\n' <"$work/tracked.z" | grep -E '(^|/)(Cargo\.toml|package\.json)$' >"$work/tracked"
  left_manifests >"$work/left"
  found=$(manifest_defect "$work/tracked" "$work/members" "$work/left" package.json "$version") || fail "$found"
  packages=$(tr -d '\n' <"$work/metadata.json" | grep -o -E '\{"name":"[^"]*","version":"[^"]*","id":"' | grep -c .)
  file_count=$(printf '%s\n' "$files" | grep -c .)
  printf 'workspace packages: %s, versions: 1, workflow and ci.d files read: %s\n' "$packages" "$file_count"
  awk '
    FILENAME == ARGV[1] { at = index($0, " "); r = substr($0, 1, at - 1); n = substr($0, at + 1); if (r == "wall") wall[n] = 1; else { reason[n] = r; seen_reason[r] = 1 }; next }
    { top = substr($0, 1, index($0 "/", "/") - 1); if (top in wall) count["wall"]++; else if ($0 in reason) count[reason[$0]]++; total++ }
    END { for (r in count) { left += count[r]; print r, count[r] }; print "total", total + 0, left + 0 }' "$work/left" "$work/tracked" >"$work/counts"
  total_line=$(sed -n 's/^total //p' "$work/counts")
  tracked_count=${total_line% *}
  left_count=${total_line#* }
  printf 'manifests tracked: %s, read for the version: %s, left by name: %s\n' "$tracked_count" "$((tracked_count - left_count))" "$left_count"
  reasons=$(sed -n '/^total /!s/ .*//p' "$work/counts" | sort)
  for reason in $reasons; do
    count=$(sed -n "s/^$reason //p" "$work/counts")
    text=$(left_reason "$reason")
    printf 'left by name, %s: %s\n' "$count" "$text"
  done
  printf 'PASS %s\n' "$name"
}

check_binaries() {
  derive
  found=$(tag_defect "$version") || fail "$found"
  for binary in "$@"; do
    label=$(basename "$binary")
    found=$(binary_defect "$binary" "$label" "$version") || fail "$found"
    printf '%s --version: the derived version\n' "$label"
  done
  printf 'PASS %s\n' "$name"
}

metadata_fixture() {
  printf '{"packages":['
  sep=
  for pair in "$@"; do
    printf '%s{"name":"%s","version":"%s","id":"path+file:///w/crates/%s#%s","dependencies":[{"name":"serde","source":"registry","req":"^1","path":"/w/x"}],"manifest_path":"/w/crates/%s/Cargo.toml"}' "$sep" "${pair%=*}" "${pair#*=}" "${pair%=*}" "${pair#*=}" "${pair%=*}"
    sep=,
  done
  printf '],"workspace_members":['
  sep=
  for pair in "$@"; do
    printf '%s"path+file:///w/crates/%s#%s"' "$sep" "${pair%=*}" "${pair#*=}"
    sep=,
  done
  printf '],"workspace_root":"/w"}\n'
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then wrong="$wrong $1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
  }
  metadata_case() {
    case_name=$1 want=$2 expected=$3
    shift 3
    metadata_fixture "$@" >"$work/$case_name.json"
    got=FAIL
    found=$(version_from "$work/$case_name.json") && [ "$found" = "$expected" ] && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  metadata_case one-version PASS 1.2.3 halvo=1.2.3 halvo-capture=1.2.3 halvo-control=1.2.3
  metadata_case one-member-differs FAIL 1.2.3 halvo=1.2.3 halvo-capture=1.2.4 halvo-control=1.2.3
  metadata_case only-the-first-differs FAIL 1.2.4 halvo=1.2.4 halvo-capture=1.2.3
  metadata_case one-package PASS 9.9.9 halvo=9.9.9
  : >"$work/empty.json"
  got=FAIL
  version_from "$work/empty.json" >/dev/null && got=PASS
  verdict empty-metadata FAIL "$got"
  metadata_fixture halvo=1.2.3 halvo-capture=1.2.3 | sed 's/{"name":"halvo-capture","version"/{"version":"1.2.3","name":"halvo-capture","x"/' >"$work/unread.json"
  got=FAIL
  version_from "$work/unread.json" >/dev/null && got=PASS
  verdict a-package-that-this-script-cannot-read FAIL "$got"
  metadata_fixture halvo=1.2.3 | sed 's/"1.2.3"/"1.2 3"/g' >"$work/space.json"
  got=FAIL
  version_from "$work/space.json" >/dev/null && got=PASS
  verdict a-version-with-a-blank FAIL "$got"

  compare_case() {
    case_name=$1 want=$2 text=$3
    printf '%s\n' "$text" >"$work/$case_name.yml"
    got=PASS
    compares=$(literal_compares "$work/$case_name.yml")
    [ -z "$compares" ] || got=FAIL
    verdict "$case_name" "$want" "$got"
  }
  flag=--version
  compare_case workflow-step-of-the-base FAIL "        run: test \"\$(target/release/halvo.exe $flag)\" = 9.9.9"
  compare_case not-equal-in-a-script FAIL "[ \"\$(halvo $flag)\" != '1.2.3' ] && exit 1"
  compare_case double-equals-with-a-v FAIL "[[ \$(halvo $flag) == v1.2.3 ]]"
  compare_case derived-value PASS "test \"\$(halvo $flag)\" = \"\$(sh scripts/r1/ci.d/61-version.sh --print)\""
  compare_case literal-on-another-line PASS "pinned_version=1.7.12"
  compare_case reversed-compare FAIL "test 9.9.9 = \"\$(halvo $flag)\""
  compare_case reversed-not-equal FAIL "[ '1.2.3' != \"\$(halvo $flag)\" ] && exit 1"
  compare_case case-over-the-version-output FAIL "case \"\$(halvo $flag)\" in 9.9.9) exit 0 ;; esac"
  compare_case case-pattern-that-is-the-flag PASS "case \"\$1\" in $flag) printf 'version: 0.10.0\\n' ;; esac"
  compare_case eq-after-the-output FAIL "[ \"\$(halvo $flag)\" -eq 1.2.3 ]"
  compare_case ne-after-the-output FAIL "test \"\$(halvo $flag)\" -ne 1.2.3 && exit 1"
  compare_case eq-before-the-output FAIL "[ 1.2.3 -eq \"\$(halvo $flag)\" ]"
  compare_case ne-before-the-output FAIL "test '1.2.3' -ne \"\$(halvo $flag)\" && exit 1"
  compare_case case-with-a-double-quoted-literal FAIL "case \"\$(halvo $flag)\" in \"9.9.9\") exit 0 ;; esac"
  compare_case case-with-a-single-quoted-literal FAIL "case \$(halvo $flag) in '9.9.9') exit 0 ;; esac"
  compare_case case-with-a-v-literal FAIL "case \"\$(halvo $flag)\" in v9.9.9) exit 0 ;; esac"
  compare_case version-printed-not-compared PASS "printf 'tool: %s\\n' \"\$(tool $flag)\" # 1.2.3"

  stub() {
    printf '#!/bin/sh\nprintf "%%s\\n" "%s"\nexit %s\n' "$2" "$3" >"$work/$1"
    chmod +x "$work/$1"
  }
  binary_case() {
    case_name=$1 want=$2
    got=FAIL
    binary_defect "$work/$3" "$3" 1.2.3 >/dev/null && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  stub right 1.2.3 0
  stub right-then-exit-3 1.2.3 3
  stub other 1.2.4 0
  stub with-a-v v1.2.3 0
  binary_case binary-prints-the-version PASS right
  binary_case binary-prints-the-version-and-exits-3 FAIL right-then-exit-3
  binary_case binary-prints-another-version FAIL other
  binary_case binary-prints-a-v FAIL with-a-v
  binary_case binary-missing FAIL missing

  tag_case() {
    case_name=$1 want=$2
    got=FAIL
    GITHUB_REF_TYPE=$3 GITHUB_REF_NAME=$4 tag_defect 1.2.3 >/dev/null && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  tag_case tag-of-another-version FAIL tag v9.9.9
  tag_case tag-of-the-version PASS tag v1.2.3
  tag_case tag-without-the-v FAIL tag 1.2.3
  tag_case branch-named-like-a-tag PASS branch v9.9.9
  tag_case no-ref-type PASS '' ''

  members_case() {
    case_name=$1 want=$2 expected=$3
    got=FAIL
    found=$(member_manifests "$work/$case_name.json") && { [ "$want" = FAIL ] || [ "$found" = "$expected" ]; } && got=PASS
    verdict "$case_name" "$want" "$got"
  }
  metadata_fixture halvo=1.2.3 halvo-capture=1.2.3 >"$work/members-of-unix-metadata.json"
  members_case members-of-unix-metadata PASS "crates/halvo/Cargo.toml
crates/halvo-capture/Cargo.toml"
  metadata_fixture halvo=1.2.3 halvo-capture=1.2.3 | sed -e 's|"/w|"D:\\\\a\\\\w|g' -e 's|/crates/\([^/"]*\)/Cargo.toml|\\\\crates\\\\\1\\\\Cargo.toml|g' >"$work/members-of-windows-metadata.json"
  members_case members-of-windows-metadata PASS "crates/halvo/Cargo.toml
crates/halvo-capture/Cargo.toml"
  metadata_fixture halvo=1.2.3 halvo-capture=1.2.3 | sed 's|"manifest_path":"/w/crates/halvo-capture|"manifest_path":"/v/crates/halvo-capture|' >"$work/member-outside-the-workspace-root.json"
  members_case member-outside-the-workspace-root FAIL ''
  metadata_fixture halvo=1.2.3 halvo-capture=1.2.3 | sed 's|"manifest_path":"/w/crates/halvo-capture|"manifest_path":"/wx/crates/halvo-capture|' >"$work/member-in-a-sibling-of-the-workspace-root.json"
  members_case member-in-a-sibling-of-the-workspace-root FAIL ''
  metadata_fixture halvo=1.2.3 halvo-capture=1.2.3 | sed 's|,"manifest_path":"/w/crates/halvo-capture/Cargo.toml"||' >"$work/member-without-a-manifest-path.json"
  members_case member-without-a-manifest-path FAIL ''
  metadata_fixture halvo=1.2.3 | sed 's|,"workspace_root":"/w"||' >"$work/metadata-without-a-workspace-root.json"
  members_case metadata-without-a-workspace-root FAIL ''

  manifest_fixture() {
    printf '%s\n' Cargo.toml src-tauri/Cargo.toml crates/a/Cargo.toml package.json tools/t/Cargo.toml 'tools/my tool/package.json' side/package.json side/x/Cargo.toml >"$work/tracked"
    printf '%s\n' src-tauri/Cargo.toml crates/a/Cargo.toml >"$work/members"
    printf '%s\n' 'tool tools/t/Cargo.toml' 'tool tools/my tool/package.json' 'wall side' >"$work/left"
    printf '{\n  "name": "halvo",\n  "private": true,\n  "devDependencies": { "x": "1.0.0" }\n}\n' >"$work/package.json"
  }
  manifest_case() {
    case_name=$1 want=$2
    got=FAIL
    manifest_defect "$work/tracked" "$work/members" "$work/left" "$work/package.json" 1.2.3 >/dev/null && got=PASS
    verdict "$case_name" "$want" "$got"
    manifest_fixture
  }
  manifest_fixture
  manifest_case every-manifest-read-or-named PASS
  printf 'tools/new/Cargo.toml\n' >>"$work/tracked"
  manifest_case manifest-outside-the-workspace-at-another-version-not-named FAIL
  printf 'tools/new/package.json\n' >>"$work/tracked"
  manifest_case package-json-outside-the-workspace-not-named FAIL
  printf '{"name":"halvo","version":"9.9.9"}\n' >"$work/package.json"
  manifest_case package-json-at-another-version FAIL
  printf '{"name":"halvo","version":"1.2.3"}\n' >"$work/package.json"
  manifest_case package-json-at-the-version PASS
  printf '{"name":"halvo","version":"v1.2.3"}\n' >"$work/package.json"
  manifest_case package-json-at-the-version-with-a-v FAIL
  printf '{"name":"halvo",\r\n "version"\r\n:\r\n"9.9.9"\r\n}\r\n' >"$work/package.json"
  manifest_case package-json-at-another-version-with-crlf-around-the-colon FAIL
  printf '{"name":"halvo","engines":{"version":"9.9.9"},"files":["version"]}\n' >"$work/package.json"
  manifest_case package-json-with-a-nested-version-alone PASS
  printf '{"name":"halvo","version":"1.2.3","version":"9.9.9"}\n' >"$work/package.json"
  manifest_case package-json-with-two-versions FAIL
  printf '{"name":"halvo","version":123}\n' >"$work/package.json"
  manifest_case package-json-with-a-version-that-is-no-string FAIL
  printf '{"name":"halvo","version":01.2.3"}\n' >"$work/package.json"
  manifest_case package-json-with-a-version-that-is-no-string-and-ends-in-a-quote FAIL
  printf '{"d":"\\"","version":"1.2.3"}\n' >"$work/package.json"
  manifest_case package-json-with-an-escaped-quote-before-the-version PASS
  printf '{"name":"halvo","version":"1.2.3\n' >"$work/package.json"
  manifest_case package-json-cut-inside-the-version FAIL
  printf '{"name":"ha\\"lvo","x":"\\\\","version":"9.9.9"}\n' >"$work/package.json"
  manifest_case package-json-at-another-version-after-escapes FAIL
  printf '{"name":"halvo","vers\\u0069on":"9.9.9"}\n' >"$work/package.json"
  manifest_case package-json-with-an-escaped-version-key FAIL
  printf 'package.json\n' >"$work/tracked"
  : >"$work/left"
  printf '{"name":"halvo","version":"9.9.9"}\n' >"$work/package.json"
  manifest_case package-json-at-another-version-alone FAIL
  grep -v -x package.json "$work/tracked" >"$work/tracked.next"
  mv "$work/tracked.next" "$work/tracked"
  printf '{"name":"halvo","version":"9.9.9"}\n' >"$work/package.json"
  manifest_case package-json-not-tracked PASS
  printf 'tool tools/gone/Cargo.toml\n' >>"$work/left"
  manifest_case named-manifest-not-tracked FAIL
  printf 'tool crates/a/Cargo.toml\n' >>"$work/left"
  manifest_case named-manifest-that-is-a-member FAIL
  printf 'tool Cargo.toml\n' >>"$work/left"
  manifest_case named-root-manifest FAIL
  printf 'tool package.json\n' >>"$work/left"
  manifest_case named-package-json FAIL
  printf 'tool tools/t/Cargo.toml\n' >>"$work/left"
  manifest_case named-manifest-twice FAIL
  sed 's/^tool /other /' "$work/left" >"$work/left.next"
  mv "$work/left.next" "$work/left"
  manifest_case named-manifest-with-a-reason-that-the-script-does-not-give FAIL
  printf 'tool\n' >>"$work/left"
  manifest_case line-with-a-reason-and-no-manifest FAIL
  grep -v -x 'tool tools/my tool/package.json' "$work/left" >"$work/left.next"
  mv "$work/left.next" "$work/left"
  manifest_case manifest-path-with-a-space-not-named FAIL
  grep -v -x 'wall side' "$work/left" >"$work/left.next"
  mv "$work/left.next" "$work/left"
  manifest_case manifests-under-a-wall-line-that-is-gone FAIL
  printf 'wall side/x\n' >>"$work/left"
  manifest_case wall-line-with-a-slash FAIL
  printf 'wall empty\n' >>"$work/left"
  manifest_case wall-line-that-covers-no-manifest FAIL
  printf 'wall crates\n' >>"$work/left"
  manifest_case wall-line-over-a-member FAIL
  printf 'wall side\n' >>"$work/left"
  manifest_case wall-line-twice FAIL
  printf 'sides/package.json\n' >>"$work/tracked"
  manifest_case manifest-in-a-sibling-of-a-wall-directory FAIL
  left_manifests >"$work/left"
  sed -n 's/^[a-z]* //p' "$work/left" | grep / >"$work/tracked"
  sed -n 's|^wall \(.*\)$|\1/package.json|p' "$work/left" >>"$work/tracked"
  printf '%s\n' Cargo.toml package.json src-tauri/Cargo.toml crates/a/Cargo.toml >>"$work/tracked"
  manifest_case the-list-of-this-script PASS

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --print)
    derive
    printf '%s\n' "$version"
    ;;
  --check)
    shift
    [ $# -gt 0 ] || { printf 'usage: sh %s --check <binary>...\n' "scripts/r1/ci.d/$name.sh" >&2; exit 64; }
    check_binaries "$@"
    ;;
  '') measure ;;
  *) printf 'usage: sh %s [--print | --check <binary>... | --self-test]\n' "scripts/r1/ci.d/$name.sh" >&2; exit 64 ;;
esac
