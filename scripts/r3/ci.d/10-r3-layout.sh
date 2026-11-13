#!/bin/sh
# jobs: macos windows
# rule: Every tracked file under a root of the agent-setup run matches a pattern of scripts/r3/layout.txt, with --all every pattern of that file matches a tracked file, and webpki-root-certs, whose licence is not on the list of A0.8, is in no graph that cargo resolves for the workspace on aarch64-apple-darwin, x86_64-apple-darwin, x86_64-pc-windows-msvc or aarch64-pc-windows-msvc (CP-102).
set -u
LC_ALL=C
export LC_ALL
name=10-r3-layout
layout=scripts/r3/layout.txt
roots='docs/r3/ scripts/r3/ scripts/r3-gate/ formal/r3-confirm/ tools/r3-trials/ tools/fixture-server/variants/ crates/halvo-policy/ crates/halvo-web/ crates/halvo-connect/ crates/halvo-sheet/ crates/halvo-agent-store/ crates/halvo-agent-gate/ crates/halvo-runner/ crates/halvo-agent-client/ crates/halvo-model/ crates/halvo-checkpoint/'
built_targets='aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc aarch64-pc-windows-msvc'
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
nl='
'

program='
function fault(line, reason) {
  printf "layout fault=%s line=%s\n", reason, line
  faults++
}
function under_root(s,   i) {
  for (i = 1; i <= nroots; i++)
    if (substr(s, 1, length(root[i])) == root[i]) return 1
  return 0
}
function unquote(s,   out, i, n, c) {
  if (substr(s, 1, 1) != "\"") return s
  s = substr(s, 2, length(s) - 2)
  n = length(s)
  out = ""
  for (i = 1; i <= n; i++) {
    c = substr(s, i, 1)
    if (c != "\\") {
      out = out c
      continue
    }
    if (substr(s, i + 1, 1) ~ /^[0-7]$/) i += 3
    else i++
    out = out "\001"
  }
  return out
}
function show(s,   out, i, n, c) {
  if (index(s, " ") == 0) return s
  out = ""
  n = length(s)
  for (i = 1; i <= n; i++) {
    c = substr(s, i, 1)
    out = out (c == " " ? "\\040" : c)
  }
  if (substr(out, 1, 1) != "\"") out = "\"" out "\""
  return out
}
function to_regex(p,   r, i, n, c) {
  r = "^"
  n = length(p)
  for (i = 1; i <= n; i++) {
    c = substr(p, i, 1)
    if (c == "*" && substr(p, i + 1, 1) == "*") {
      r = r ".*"
      i++
    } else if (c == "*") r = r "[^/]*"
    else if (c == ".") r = r "[.]"
    else r = r c
  }
  return r "$"
}
function read_layout(line, n) {
  sub("\001$", "", line)
  if (split(line, f, " ") != 2 || f[1] " " f[2] != line) return fault(n, "fields")
  p = f[1]
  u = f[2]
  if (p !~ "^[-A-Za-z0-9._/*]+$" || index(p, "//") || substr(p, length(p)) == "/") return fault(n, "pattern")
  if (!under_root(p)) return fault(n, "root")
  if (u !~ "^U[0-9]+[.][0-9]+(,U[0-9]+[.][0-9]+)*$") return fault(n, "unit")
  if (p in seen) return fault(n, "duplicate")
  seen[p] = 1
  pat[++np] = p
  rex[np] = to_regex(p)
}
function read_file(raw,   path, i, hit) {
  path = unquote(raw)
  if (!under_root(path)) return
  nfiles++
  for (i = 1; i <= np; i++)
    if (path ~ rex[i]) {
      hit = 1
      used[i] = 1
    }
  if (!hit) stray[++nout] = show(raw)
}
BEGIN {
  lay = ENVIRON["R3_LAYOUT_COPY"]
  files = ENVIRON["R3_FILE_LIST"]
  resultfile = ENVIRON["R3_RESULT"]
  nroots = split(roots, root, " ")
  if (missing) fault("-", "unreadable")
  else {
    while ((got = (getline line < lay)) > 0) read_layout(line, ++nlay)
    if (got < 0) exit 2
    if (nlay == 0) fault("-", "no_lines")
    else if (nolf) fault(nlay, "no_final_lf")
  }
  while ((got = (getline line < files)) > 0) read_file(line)
  if (got < 0) exit 2
  printf "layout patterns=%d files=%d outside=%d\n", np, nfiles, nout
  for (i = 1; i <= nout; i++) printf "layout outside=%s\n", stray[i]
  if (all)
    for (i = 1; i <= np; i++)
      if (!(i in used)) {
        printf "layout empty=%s\n", pat[i]
        nempty++
      }
  result = faults ? "layout" : nout ? "outside" : nempty ? "empty" : "pass"
  print result > resultfile
}
'

fail() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

tree_check() {
  started=$(date +%s)
  found=
  counted=0
  for target in $built_targets; do
    counted=$((counted + 1))
    if ! cargo tree --workspace --all-features -e all --target "$target" --prefix none --locked >"$artifacts/tree-$target.txt" 2>&1; then
      printf 'tree target=%s cargo=failed\n' "$target"
      fail tree
    fi
    if grep -q '^webpki-root-certs v' "$artifacts/tree-$target.txt"; then
      printf 'tree target=%s webpki-root-certs=yes\n' "$target"
      found=yes
    else
      printf 'tree target=%s webpki-root-certs=no\n' "$target"
    fi
  done
  printf 'tree targets=%s seconds=%s\n' "$counted" "$(($(date +%s) - started))"
  [ -z "$found" ] || fail tree
}

measure() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || fail setup
  fi
  cd "$repo" || fail setup
  artifacts=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p "$artifacts" || fail setup
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || fail setup
  git -c core.quotepath=on ls-files >"$scratch/index" || fail git
  tr -d '\r' <"$scratch/index" >"$scratch/files" || fail setup
  missing=0
  nolf=0
  if [ ! -f "$layout" ]; then
    missing=1
  else
    tr '\000\001\r' '\002\002\001' <"$layout" >"$scratch/layout" || fail setup
    if [ -s "$scratch/layout" ]; then
      last=$(tail -c 1 "$scratch/layout") || fail setup
      [ -z "$last" ] || nolf=1
    fi
  fi
  R3_LAYOUT_COPY=$scratch/layout R3_FILE_LIST=$scratch/files R3_RESULT=$scratch/result \
    awk -v roots="$roots" -v all="$1" -v missing="$missing" -v nolf="$nolf" "$program" >"$artifacts/report.txt" || fail awk
  cat "$artifacts/report.txt" || fail setup
  result=
  read -r result <"$scratch/result"
  case $result in
    pass) ;;
    layout | outside | empty) fail "$result" ;;
    *) fail awk ;;
  esac
  tree_check
  printf 'PASS %s\n' "$name"
}

broken() {
  printf 'FAIL self-test:%s (fixture %s)\n' "$name" "$1"
  exit 1
}

make_repo() {
  label=$1
  dir=$work/$label
  shift
  git init -q "$dir" >/dev/null 2>&1 || broken "$label: git init"
  git -C "$dir" config core.protectNTFS false || broken "$label: git config"
  blob=$(git -C "$dir" hash-object -w --stdin </dev/null) || broken "$label: hash-object"
  printf '%s\0' "$@" >"$work/$label.want" || broken "$label: list"
  count=$#
  for p do
    set -- "$@" --cacheinfo "100644,$blob,$p"
  done
  shift "$count"
  (cd "$dir" && MSYS_NO_PATHCONV=1 MSYS2_ARG_CONV_EXCL='*' git update-index --add "$@") || broken "$label: update-index"
  sort -z <"$work/$label.want" >"$work/$label.sorted" || broken "$label: sort"
  git -C "$dir" ls-files -z >"$work/$label.index" || broken "$label: ls-files"
  cmp -s "$work/$label.sorted" "$work/$label.index" || broken "$label: the index does not hold the listed paths byte for byte"
  mkdir -p "$dir/scripts/r3" || broken "$label: mkdir"
}

lay() {
  printf '%b' "$2" >"$work/$1/scripts/r3/layout.txt" || broken "$1: layout"
}

check() {
  case $mode in
    driver) out=$(CI_D_REPO=$work/$2 CI_D_ARTIFACTS=$work/artifacts sh "$self" ${5:+"$5"} 2>&1) ;;
    alone) out=$(cd "$work/$2" && unset CI_D_REPO CI_D_ARTIFACTS && sh "$self" 2>&1) ;;
    stub) out=$(PATH=$work/$stub:$PATH CI_D_REPO=$work/$2 CI_D_ARTIFACTS=$work/artifacts sh "$self" 2>&1) ;;
    tmpdir) out=$(TMPDIR=$tmp CI_D_REPO=$work/$2 CI_D_ARTIFACTS=$work/artifacts sh "$self" 2>&1) ;;
  esac
  code=$?
  last=${out##*"$nl"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      got=${last#"FAIL $name ("}
      got=${got%")"}
      ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$4"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$3" "$got"
  if [ "$got" != "$3" ]; then
    wrong="$wrong $1"
    printf '%s\n' "$out" >"$artifacts/self-test-$1.txt"
  elif [ "$3" != PASS ]; then
    rejected=$((rejected + 1))
  fi
  mode=driver
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || broken mktemp
  artifacts=${CI_D_ARTIFACTS:-$work/artifacts}
  mkdir -p "$work/artifacts" "$artifacts" "$work/stub-git" "$work/stub-awk" "$work/stub-crlf-git" || broken mkdir
  printf '#!/bin/sh\nexit 1\n' >"$work/stub-git/git" || broken stub
  printf '#!/bin/sh\nexit 0\n' >"$work/stub-awk/awk" || broken stub
  R3_REAL_GIT=$(command -v git) || broken git
  R3_STUB_OUT=$work/stub-crlf-git/out
  export R3_REAL_GIT R3_STUB_OUT
  printf '%s\n' '#!/bin/sh' "\"\$R3_REAL_GIT\" \"\$@\" >\"\$R3_STUB_OUT\" || exit 1" \
    "awk '{ printf \"%s\\r\\n\", \$0 }' \"\$R3_STUB_OUT\"" >"$work/stub-crlf-git/git" || broken stub
  chmod +x "$work/stub-git/git" "$work/stub-awk/awk" "$work/stub-crlf-git/git" || broken chmod
  mkdir -p "$work/stub-cargo" || broken mkdir
  cat >"$work/stub-cargo/cargo" <<'EOF' || broken stub
#!/bin/sh
case " $* " in
  *" tree --workspace --all-features -e all --target "*" --prefix none --locked "*) ;;
  *) exit 97 ;;
esac
[ "${R3_CARGO_FAILS:-}" != yes ] || exit 101
printf 'halvo-connect v0.0.0\nrustls v0.23.46\n'
case " $* " in
  *" --target ${R3_WEBPKI_TARGET:-none} "*) printf 'webpki-root-certs v1.0.11\n' ;;
esac
EOF
  chmod +x "$work/stub-cargo/cargo" || broken chmod
  PATH=$work/stub-cargo:$PATH
  R3_WEBPKI_TARGET=
  R3_CARGO_FAILS=
  export PATH R3_WEBPKI_TARGET R3_CARGO_FAILS
  mode=driver
  rejected=0
  wrong=
  tab=$(printf '\t')
  ctl=$(printf '\001')
  e_acute=$(printf '\303\251')

  make_repo a docs/r3/README.md docs/r3/trials/sheet.md tools/r3-trials/web/run.sh \
    tools/r3-trials/web/sub/deep/file.txt crates/halvo-runner/tests/injection_one.rs \
    crates/halvo-runner/src/lib.rs scripts/r3-gate/run.sh crates/halvo-schema/src/lib.rs \
    docs/r2/README.md docs/r3x/notes.md x/docs/r3/y.md
  a_head='docs/r3/README.md U1.4\n'
  a_web='tools/r3-trials/web/** U0.2\n'
  a_rest='crates/halvo-runner/** U4.1,U4.2\ncrates/halvo-runner/tests/injection_*.rs U6.2\nscripts/r3-gate/** U10.1\n'
  a_tail="docs/r3/trials/*.md U0.2\\n$a_web$a_rest"
  a_good="$a_head$a_tail"

  check layout-missing a layout 'layout fault=unreadable line=-'
  lay a ''
  check layout-empty a layout 'layout fault=no_lines line=-'
  lay a "$a_good"
  check all-files-match a PASS 'layout patterns=6 files=7 outside=0'
  check all-patterns-have-a-file a PASS 'layout patterns=6 files=7 outside=0' --all
  mode=alone
  check without-the-driver a PASS 'layout patterns=6 files=7 outside=0'
  [ -f "$work/a/target/ci-artifacts/$name/report.txt" ] || {
    printf 'case %s/%s want=report got=no-report\n' "$name" without-the-driver-artifacts
    wrong="$wrong without-the-driver-artifacts"
  }
  lay a "$a_tail"
  check one-file-matches-no-line a outside 'layout outside=docs/r3/README.md'
  lay a "${a_good}docs/r3/gate.md U10.1\n"
  check pattern-without-a-file-under-all a empty 'layout empty=docs/r3/gate.md' --all
  check pattern-without-a-file-without-all a PASS 'layout patterns=7 files=7 outside=0'
  lay a "${a_tail}docs/r3/gate.md U10.1\n"
  check stray-file-before-empty-pattern a outside 'layout empty=docs/r3/gate.md' --all
  lay a "docs/r3/* U1.4\n$a_web$a_rest"
  check star-stops-at-a-slash a outside 'layout outside=docs/r3/trials/sheet.md'
  lay a "${a_head}docs/r3/trials/*.md U0.2\\ntools/r3-trials/web/* U0.2\\n$a_rest"
  check one-star-is-not-two a outside 'layout outside=tools/r3-trials/web/sub/deep/file.txt'
  printf '%b' "$a_good" >"$work/a.lf" || broken "a: layout"
  awk '{ printf "%s\r\n", $0 }' "$work/a.lf" >"$work/a/scripts/r3/layout.txt" || broken "a: layout"
  check carriage-returns-before-lf a PASS 'layout patterns=6 files=7 outside=0'
  lay a "${a_good}docs/r3/README.md U1.4\r\r\n"
  check carriage-return-inside-a-line a layout 'layout fault=unit line=7'
  lay a "${a_good%??}"
  check no-lf-at-the-end a layout 'layout fault=no_final_lf line=6'
  lay a "\0357\0273\0277$a_good"
  check byte-order-mark a layout 'layout fault=pattern line=1'
  lay a "docs/r3/README.md${tab}U1.4\n$a_tail"
  check tab-between-fields a layout 'layout fault=fields line=1'
  lay a "docs/r3/README.md  U1.4\n$a_tail"
  check two-spaces-between-fields a layout 'layout fault=fields line=1'
  lay a "docs/r3/README.md U1.4 U1.5\n$a_tail"
  check three-fields a layout 'layout fault=fields line=1'
  lay a "docs/r3/README.md U1.4\n\n$a_tail"
  check empty-line a layout 'layout fault=fields line=2'
  others='!"#$%&()+,:;<=>?@[\]^`{|}~'"'"
  : >"$work/a/scripts/r3/layout.txt" || broken "a: layout"
  want=
  n=0
  rest=$others
  while [ -n "$rest" ]; do
    c=${rest%"${rest#?}"}
    rest=${rest#?}
    n=$((n + 1))
    printf 'docs/r3/a%s.md U1.4\n' "$c" >>"$work/a/scripts/r3/layout.txt" || broken "a: layout"
    want="${want}layout fault=pattern line=$n$nl"
  done
  check every-byte-outside-the-pattern-set a layout "$want"
  lay a "docs/r3/README.md U1.4\0000junk\n$a_tail"
  check nul-byte-inside-a-line a layout 'layout fault=unit line=1'
  lay a "${a_good}docs/r3/gate.md U10.1\0000"
  check nul-byte-at-the-end a layout 'layout fault=no_final_lf line=7'
  lay a "$a_good"
  mode=stub
  stub='stub-crlf-git'
  check file-list-with-crlf a PASS 'layout patterns=6 files=7 outside=0'
  tmp="$work/t\tx"
  mkdir -p "$tmp" || broken "tmpdir"
  mode=tmpdir
  check tmpdir-with-a-backslash a PASS 'layout patterns=6 files=7 outside=0'
  lay a "docs/r3//README.md U1.4\n$a_tail"
  check double-slash a layout 'layout fault=pattern line=1'
  lay a "docs/r3/trials/ U0.2\n$a_good"
  check pattern-ends-with-a-slash a layout 'layout fault=pattern line=1'
  lay a "docs/r2/README.md U1.4\n$a_good"
  check pattern-under-no-root a layout 'layout fault=root line=1'
  lay a "docs/r3x/** U1.4\n$a_good"
  check root-without-its-slash a layout 'layout fault=root line=1'
  lay a "tools/docs/r3/x.md U1.4\n$a_good"
  check root-after-the-start-of-a-pattern a layout 'layout fault=root line=1'
  lay a "$a_good"
  check root-after-the-start-of-a-path a PASS 'layout patterns=6 files=7 outside=0'
  lay a "docs/r3/README.md 1.4\n$a_tail"
  check unit-without-u a layout 'layout fault=unit line=1'
  lay a "docs/r3/README.md U1.4,\n$a_tail"
  check unit-list-with-a-trailing-comma a layout 'layout fault=unit line=1'
  lay a "${a_good}docs/r3/README.md U0.1\n"
  check pattern-twice a layout 'layout fault=duplicate line=7'

  mode=stub
  stub='stub-git'
  lay a "$a_good"
  check git-fails a git 'FAIL 10-r3-layout (git)'
  mode=stub
  stub='stub-awk'
  check awk-prints-nothing a awk 'FAIL 10-r3-layout (awk)'
  check unknown-argument a usage 'FAIL 10-r3-layout (usage)' --bogus
  out=$(CI_D_REPO=$work/a CI_D_ARTIFACTS=$work/artifacts sh "$self" --all --all 2>&1)
  case $?:$out in
    "1:FAIL $name (usage)") rejected=$((rejected + 1)) && printf 'case %s/two-arguments want=usage got=usage\n' "$name" ;;
    *) printf 'case %s/two-arguments want=usage got=other\n' "$name" && wrong="$wrong two-arguments" ;;
  esac

  lay a "$a_good"
  check tree-reads-the-four-targets a PASS 'tree target=aarch64-pc-windows-msvc webpki-root-certs=no'
  R3_WEBPKI_TARGET=aarch64-apple-darwin
  check tree-holds-webpki-on-the-first-target a tree 'tree target=aarch64-apple-darwin webpki-root-certs=yes'
  R3_WEBPKI_TARGET=aarch64-pc-windows-msvc
  check tree-holds-webpki-on-the-last-target a tree 'tree target=aarch64-pc-windows-msvc webpki-root-certs=yes'
  R3_WEBPKI_TARGET=wasm32-unknown-unknown
  check tree-holds-webpki-on-wasm32-alone a PASS 'tree targets=4'
  R3_WEBPKI_TARGET=
  R3_CARGO_FAILS=yes
  check tree-cargo-fails a tree 'tree target=aarch64-apple-darwin cargo=failed'
  R3_CARGO_FAILS=

  make_repo b docs/r3/README.md.bak docs/r3/notes docs/r3/.md docs/r3/log-xyz docs/r3/a docs/r3/READMEXmd
  b_rest='docs/r3/notes** U1.4\ndocs/r3/*.md U1.4\ndocs/r3/log-* U1.4\n'
  lay b "docs/r3/README.md.bak U1.4\n${b_rest}docs/r3/a U1.4\ndocs/r3/READMEXmd U1.4\n"
  check stars-match-no-byte-and-many b PASS 'layout patterns=6 files=6 outside=0' --all
  lay b "docs/r3/README.md U1.4\n${b_rest}docs/r3/a U1.4\ndocs/r3/READMEXmd U1.4\n"
  check literal-pattern-is-not-a-prefix b outside 'layout outside=docs/r3/README.md.bak'
  lay b "docs/r3/README.md.bak U1.4\n${b_rest}docs/r3/a U1.4\ndocs/r3/README.md U1.4\n"
  check dot-is-literal b outside 'layout outside=docs/r3/READMEXmd'
  lay b "docs/r3/README.md.bak U1.4\n${b_rest}docs/r3/a.md U1.4\ndocs/r3/READMEXmd U1.4\n"
  check path-shorter-than-the-pattern b outside 'layout outside=docs/r3/a'
  lay b "docs/r3/README.md.bak U1.4\ndocs/r3/notes** U1.4\ndocs/r3/*.md U1.4\ndocs/r3/log- U1.4\ndocs/r3/a U1.4\ndocs/r3/READMEXmd U1.4\n"
  check literal-pattern-is-not-a-suffix-match b outside 'layout outside=docs/r3/log-xyz'

  make_repo c "tools/r3-trials/web/caf$e_acute.txt" "docs/r3/caf$e_acute.md" "docs/r3/tab${tab}name.md" \
    'docs/r3/q"uote.md' 'docs/r3/back\slash.md' "docs/r3/ctl$ctl.md" 'docs/r3/two words.md' \
    "docs/r3/two words$e_acute.md" \
    'docs/r3/d"/n.md' "docs/r3/e$e_acute/n.md" "crates/halvo-schema/caf$e_acute"
  c_base='tools/r3-trials/web/** U0.2\ndocs/r3/*.md U1.4\n'
  lay c "${c_base}docs/r3/*/n.md U1.4\n"
  check quoted-names-match c PASS 'layout patterns=3 files=10 outside=0'
  check quoted-names-fill-patterns c PASS 'layout patterns=3 files=10 outside=0' --all
  lay c "$c_base"
  check quoted-byte-before-a-slash c outside 'layout patterns=2 files=10 outside=2'
  lay c 'tools/r3-trials/web/** U0.2\n'
  check quoted-name-printed-as-git-quotes-it c outside 'layout outside="docs/r3/caf\303\251.md"'
  check space-printed-as-an-escape c outside 'layout outside="docs/r3/two\040words.md"'
  check quoted-name-with-a-space c outside 'layout outside="docs/r3/two\040words\303\251.md"'
  allowed=abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-
  c_each='tools/r3-trials/web/** U0.2\n'
  rest=$allowed
  while [ -n "$rest" ]; do
    c=${rest%"${rest#?}"}
    rest=${rest#?}
    c_each="${c_each}docs/r3/caf$c$c.md U1.4\n"
  done
  lay c "$c_each"
  check escaped-byte-is-no-pattern-byte c outside 'layout outside="docs/r3/caf\303\251.md"'
  lay c 'tools/r3-trials/web/** U0.2\ndocs/r3/caf*3*1.md U1.4\ndocs/r3/caf.md U1.4\ndocs/r3/tab*tname.md U1.4\n'
  check octal-escape-is-one-byte c outside 'layout outside="docs/r3/caf\303\251.md"'
  check letter-escape-is-one-byte c outside 'layout outside="docs/r3/tab\tname.md"'

  make_repo d docs/r3/x scripts/r3/x scripts/r3-gate/x formal/r3-confirm/x tools/r3-trials/x \
    tools/fixture-server/variants/x crates/halvo-policy/x crates/halvo-web/x crates/halvo-connect/x \
    crates/halvo-sheet/x crates/halvo-agent-store/x crates/halvo-agent-gate/x crates/halvo-runner/x \
    crates/halvo-agent-client/x crates/halvo-model/x crates/halvo-checkpoint/x \
    tools/fixture-server/x crates/halvo-schema/x docs/r3/y/docs/r3/x
  d_layout=
  d_rest=
  for r in $roots; do
    d_layout="$d_layout$r** U1.4\n"
    [ "$r" = docs/r3/ ] || d_rest="$d_rest$r** U1.4\n"
  done
  lay d "$d_layout"
  check every-root-counts d PASS 'layout patterns=16 files=17 outside=0' --all
  lay d 'docs/r3/** U1.4\n'
  check every-root-is-checked d outside 'layout patterns=1 files=17 outside=15'
  lay d "docs/r3/x U1.4\n$d_rest"
  check pattern-is-anchored-at-the-start d outside 'layout outside=docs/r3/y/docs/r3/x'

  make_repo e crates/halvo-runner/tests/injection_a.b.rs
  lay e 'crates/halvo-runner/tests/injection_*.rs U6.2\n'
  check star-matches-a-dot e PASS 'layout patterns=1 files=1 outside=0' --all

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case $#:${1:-} in
  0:) measure 0 ;;
  1:--all) measure 1 ;;
  1:--self-test) self_test ;;
  *) fail usage ;;
esac
