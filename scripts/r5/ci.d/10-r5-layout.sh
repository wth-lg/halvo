#!/bin/sh
# jobs: macos windows
# rule: Every tracked file under a root of scripts/r5/layout.txt matches one of its patterns, the files that docs/r5/freeze.toml freezes have its hashes, and docs/r5/heldout.txt names the tree of the drift harness at HEAD.
# lane: free
set -u
LC_ALL=C
export LC_ALL
name=10-r5-layout
lfx=$(printf '\nx')

finish() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

broken() {
  printf 'FAIL self-test:%s (scratch)\n' "$name"
  exit 1
}

ends_in_lf() {
  [ -f "$1" ] && [ -s "$1" ] || return 1
  last=$(tail -c 1 "$1" && printf x) || return 1
  [ "$last" = "$lfx" ]
}

no_nul() {
  tr -d '\000' <"$1" >"$art/without-nul.txt" || return 1
  cmp -s "$art/without-nul.txt" "$1"
}

sha_tool() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$@"
  else
    shasum -a 256 "$@"
  fi
}

digest_stdin() {
  sum=$(sha_tool) || return 1
  sum=${sum%%[!0-9a-f]*}
  [ "${#sum}" -eq 64 ] || return 1
  printf '%s\n' "$sum"
}

regular_in_index() {
  R5_PATH=$1 awk '
    BEGIN { FS = "\t" }
    $2 == ENVIRON["R5_PATH"] {
      n++
      if ($1 !~ /^100(644|755) [0-9a-f]+ 0$/) bad = 1
    }
    END { if (bad || n != 1) exit 1 }
  ' "$art/staged.txt"
}

file_digest() {
  regular_in_index "$1" || return 1
  [ -f "$1" ] || return 1
  digest_stdin <"$1"
}

list_digest() {
  sorted_list=$1
  set --
  while IFS= read -r path; do
    case $path in
      '') return 1 ;;
    esac
    [ -f "$path" ] || return 1
    set -- "$@" "$path"
  done <"$sorted_list"
  : >"$art/digest-lines.txt"
  if [ "$#" -gt 0 ]; then
    sha_tool "$@" >"$art/digest-sums.txt" || return 1
    R5_LIST=$sorted_list awk '
      BEGIN { list = ENVIRON["R5_LIST"] }
      {
        if (length($1) != 64 || $1 ~ /[^0-9a-f]/) exit 1
        if ((getline path < list) <= 0) exit 1
        printf "%s  %s\n", $1, path
      }
      END { if ((getline path < list) > 0) exit 1 }
    ' "$art/digest-sums.txt" >"$art/digest-lines.txt" || return 1
  fi
  digest_stdin <"$art/digest-lines.txt"
}

paths_under() {
  R5_PREFIXES=$* awk '
    BEGIN { FS = "\t"; n = split(ENVIRON["R5_PREFIXES"], prefix, " ") }
    {
      probe = $2
      if (substr(probe, 1, 1) == "\"") probe = substr(probe, 2)
      for (i = 1; i <= n; i++) {
        if (index(probe, prefix[i]) == 1) {
          if (probe != $2 || NF != 2 || $1 !~ /^100(644|755) [0-9a-f]+ 0$/) { bad = 1; exit 1 }
          print $2
          break
        }
      }
    }
    END { if (bad) exit 1 }
  ' "$art/staged.txt"
}

set_digest() {
  named=$1
  shift
  paths_under "$@" >"$art/set-paths.txt" || return 1
  if [ "$named" != - ]; then
    regular_in_index "$named" || return 1
    printf '%s\n' "$named" >>"$art/set-paths.txt"
  fi
  sort -u "$art/set-paths.txt" >"$art/set-sorted.txt" || return 1
  list_digest "$art/set-sorted.txt"
}

heldout_digits() {
  regular_in_index docs/r5/heldout.txt || return 1
  ends_in_lf docs/r5/heldout.txt || return 1
  no_nul docs/r5/heldout.txt || return 1
  awk '
    NR == 1 {
      if ($0 !~ /^heldout cases=24 benign=12 fault=12 sha256=[0-9a-f]+$/) exit 1
      if (length($0) != 107) exit 1
      print substr($0, 44)
      found = 1
      exit 0
    }
    END { if (!found) exit 1 }
  ' docs/r5/heldout.txt
}

key_digest() {
  case $1 in
    targets) file_digest docs/r5/targets.toml ;;
    cases) set_digest docs/r5/cases.md fixtures/r5/cases/dev/ ;;
    stat) set_digest - tools/r5-trials/stat/ ;;
    drift) set_digest - tools/r5-trials/drift/ ;;
    heldout) heldout_digits ;;
    crates) set_digest - crates/halvo-learn/ crates/halvo-learn-store/ ;;
    *) return 1 ;;
  esac
}

read_freeze() {
  ends_in_lf "$1" || return 1
  no_nul "$1" || return 1
  awk '
    BEGIN {
      keys["targets"] = "targets cases stat drift heldout"
      keys["learner"] = "crates"
      rank["targets"] = 1
      rank["learner"] = 2
    }
    function refuse() { refused = 1; exit 1 }
    /^\[[a-z]+\]$/ {
      if (table != "" && k < nk) refuse()
      t = substr($0, 2, length($0) - 2)
      if (!(t in rank) || rank[t] <= last) refuse()
      last = rank[t]
      table = t
      nk = split(keys[t], want, " ")
      k = 0
      next
    }
    {
      if (table == "" || k >= nk) refuse()
      k++
      head = want[k] " = \""
      if (index($0, head) != 1) refuse()
      value = substr($0, length(head) + 1)
      if (length(value) != 65 || substr(value, 65) != "\"") refuse()
      value = substr(value, 1, 64)
      if (value ~ /[^0-9a-f]/) refuse()
      print table, want[k], value
    }
    END { if (refused || table == "" || k < nk) exit 1 }
  ' "$1"
}

compare_table() {
  table=$1
  shift
  present=no
  while read -r t k v; do
    if [ "$t" = "$table" ]; then
      present=yes
    fi
  done <"$art/freeze-read.txt" || present=unreadable
  case $present in
    no)
      printf 'none\n'
      return 0
      ;;
    unreadable)
      printf 'miss\n'
      return 0
      ;;
  esac
  state=ok
  for key in "$@"; do
    frozen=
    while read -r t k v; do
      if [ "$t $k" = "$table $key" ]; then
        frozen=$v
      fi
    done <"$art/freeze-read.txt" || frozen=
    computed=$(key_digest "$key") || computed=unreadable
    if [ -n "$frozen" ] && [ "$computed" = "$frozen" ]; then
      same=yes
    else
      same=no
      state=miss
    fi
    printf 'table=%s key=%s same=%s computed=%s frozen=%s\n' "$table" "$key" "$same" "$computed" "$frozen" >>"$art/freeze.txt"
  done
  printf '%s\n' "$state"
}

match_layout() {
  R5_TRACKED=$art/tracked.txt R5_OUTSIDE=$art/outside.txt R5_UNMATCHED=$1 awk '
    function bad(why) { printf "line %d: %s\n", NR, why > "/dev/stderr"; failed = 1; exit 1 }
    function regex(p,   r, i, c) {
      r = "^"
      for (i = 1; i <= length(p); i++) {
        c = substr(p, i, 1)
        if (c == "*") {
          if (substr(p, i + 1, 1) == "*") { r = r ".*"; i++ } else r = r "[^/]*"
        } else if (c == ".") r = r "[.]"
        else r = r c
      }
      return r "$"
    }
    function plain(f,   s, i, c, out) {
      if (substr(f, 1, 1) != "\"" || substr(f, length(f), 1) != "\"") return f
      s = substr(f, 2, length(f) - 2)
      out = ""
      for (i = 1; i <= length(s); i++) {
        c = substr(s, i, 1)
        if (c != "\\") { out = out c; continue }
        if (substr(s, i + 1, 1) ~ /[0-7]/) i += 3
        else i++
        out = out "#"
      }
      return out
    }
    BEGIN {
      FS = "\t"
      tracked = ENVIRON["R5_TRACKED"]
      outside = ENVIRON["R5_OUTSIDE"]
      unmatched = ENVIRON["R5_UNMATCHED"]
      known = split("docs/r5/ scripts/r5/ scripts/r5-gate/ formal/r5-admit/ tools/r5-trials/ fixtures/r5/ crates/halvo-learn/ crates/halvo-learn-store/ crates/halvo-learn-gate/", l13, " ")
      for (i = 1; i <= known; i++) of_l13[l13[i]] = 1
    }
    {
      if (NF != 2) bad("not two fields separated by one tab")
      if ($1 == "root" && np > 0) bad("a root line after a pattern line")
      if (seen[$0]++) bad("a repeated line")
      if ($1 == "root") {
        if (!($2 in of_l13)) bad("a root that is not one of the nine of L13")
        root[++nr] = $2
        next
      }
      if ($1 !~ /^U[0-9]+\.[0-9]+$/) bad("a first field that is neither root nor a unit")
      if ($2 !~ /^[A-Za-z0-9._*-]+(\/[A-Za-z0-9._*-]+)*$/) bad("a pattern with another byte or an empty component")
      if ($2 ~ /\*\*\*/) bad("three stars in a row")
      if (("/" $2 "/") ~ /\/\.\.?\//) bad("a dot component")
      under = 0
      for (i = 1; i <= nr; i++)
        if (index($2, root[i]) == 1) under = 1
      if (!under) bad("a pattern under no root")
      np++
      unit[np] = $1
      pattern[np] = $2
      re[np] = regex($2)
    }
    END {
      if (failed) exit 1
      if (nr != known) { print "not the nine root lines of L13" > "/dev/stderr"; exit 1 }
      while ((getline f < tracked) > 0) {
        probe = plain(f)
        found = 0
        for (i = 1; i <= nr; i++) if (index(tolower(probe), tolower(root[i])) == 1) found = 1
        if (!found) continue
        files++
        hit = 0
        for (j = 1; j <= np; j++) if (probe ~ re[j]) { hit = 1; used[j] = 1 }
        if (!hit) { out++; print f > outside }
      }
      for (j = 1; j <= np; j++) {
        if (used[j]) continue
        missing++
        if (unmatched != "") print unit[j] "\t" pattern[j] > unmatched
      }
      printf "%d %d %d %d %d\n", nr, np, files, out, missing
    }
  ' scripts/r5/layout.txt
}

measure() {
  all=$1
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    here=$(dirname "$0")
    repo=$(cd -P -- "$here/../../.." && pwd -P) || finish scratch
  fi
  art=${CI_D_ARTIFACTS:-$repo/target/ci-artifacts/$name}
  mkdir -p -- "$art" || finish scratch
  art=$(cd -P -- "$art" && pwd -P) || finish scratch
  cd -- "$repo" || finish scratch
  here=$(pwd -P) || finish scratch
  case $art/ in
    "$here"/target/*) ;;
    "$here"/*) finish scratch ;;
  esac

  command -v git >/dev/null 2>&1 || finish git
  git -c core.quotepath=on ls-files >"$art/tracked.txt" 2>"$art/git-error.txt" || finish git
  git -c core.quotepath=on ls-files -s >"$art/staged.txt" 2>>"$art/git-error.txt" || finish git

  if ! ends_in_lf scripts/r5/layout.txt || ! no_nul scripts/r5/layout.txt; then
    finish layout
  fi
  : >"$art/outside.txt"
  unmatched_file=
  if [ "$all" = yes ]; then
    unmatched_file=$art/unmatched.txt
    : >"$unmatched_file"
  fi
  match_layout "$unmatched_file" >"$art/counts.txt" 2>"$art/layout-error.txt" || finish layout
  read -r roots patterns files outside unmatched <"$art/counts.txt" || finish layout

  targets=none
  learner=none
  refused=no
  : >"$art/freeze.txt"
  if [ -e docs/r5/freeze.toml ]; then
    if regular_in_index docs/r5/freeze.toml && read_freeze docs/r5/freeze.toml >"$art/freeze-read.txt"; then
      targets=$(compare_table targets targets cases stat drift heldout)
      learner=$(compare_table learner crates)
    else
      refused=yes
      targets=miss
      learner=miss
    fi
  fi
  if [ "$targets" = none ]; then
    refused=yes
  fi

  harness=none
  tree_failed=no
  if [ -e docs/r5/heldout.txt ]; then
    if tree=$(git rev-parse --verify -q 'HEAD:tools/r5-trials/drift' 2>>"$art/git-error.txt"); then
      line=$(sed -n 2p docs/r5/heldout.txt) || line=
      if [ "$line" = "harness tree=$tree" ]; then
        harness=same
      else
        harness=differ
      fi
    else
      harness=differ
      tree_failed=yes
    fi
  fi

  printf 'layout roots=%s patterns=%s files=%s outside=%s targets=%s learner=%s harness=%s\n' \
    "$roots" "$patterns" "$files" "$outside" "$targets" "$learner" "$harness"
  if [ "$all" = yes ]; then
    printf 'layout unmatched=%s\n' "$unmatched"
  fi
  [ "$outside" -eq 0 ] || finish outside
  [ "$refused" = no ] || finish freeze
  [ "$targets" != miss ] || finish targets
  [ "$learner" != miss ] || finish learner
  [ "$tree_failed" = no ] || finish tree
  [ "$harness" != differ ] || finish harness
  if [ "$all" = yes ]; then
    case "$targets $learner $harness" in
      *none*) finish none ;;
    esac
  fi
  printf 'PASS %s\n' "$name"
}

archive=cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc
d_targets=18f97447ef9bb23ed38ac5d9958a6590d179ad79d2fd662f3d64eb4714c1e8cb
d_cases=f3231acf0f9b901c1a6f53b198cc129b7603562a0877af227ea7d40e52a45a39
d_stat=512d4eb2d649dbad496c0771d6afa521cd8b551d429dfeb8d82fbd46da653bea
d_drift=70f7662612bb4c9f2776179a4f8dd019ff1389873aa1af4bedc6824a817bd474
d_crates=02f94b0a4459fffc17c91782e0e4f79ff6cc18d459add74ff41ce3f1da4f9f37
d_crates_store=fcb99729441633423e9989dece60f00cb05ff497f6ed7381e001eec453d48065
d_tree=09784fa0279a0512247c46f7747fe60f2bed374e

commit_all() {
  git -C "$1" add -A || broken
  t=$(git -C "$1" write-tree) || broken
  c=$(printf 'tree %s\nauthor fixture <fixture@example.invalid> 1 +0000\ncommitter fixture <fixture@example.invalid> 1 +0000\n\nfixture\n' "$t" |
    git -C "$1" hash-object -t commit -w --stdin) || broken
  git -C "$1" update-ref HEAD "$c" || broken
}

freeze_text() {
  printf '[targets]\ntargets = "%s"\ncases = "%s"\nstat = "%s"\ndrift = "%s"\nheldout = "%s"\n' \
    "$1" "$d_cases" "$d_stat" "$d_drift" "$archive"
}

fixture() {
  if [ ! -d "$work/template" ]; then
    template "$work/template"
  fi
  r=$work/$1
  cp -R "$work/template" "$r" || broken
  if [ "${2:-}" = no-commit ]; then
    git -C "$r" update-ref -d HEAD || broken
  fi
}

template() {
  r=$1
  mkdir -p "$r/docs/r5" "$r/fixtures/r5/cases/dev" "$r/tools/r5-trials/stat" "$r/tools/r5-trials/drift" "$r/scripts/r5" || broken
  git init -q "$r" || broken
  git -C "$r" config core.autocrlf false || broken
  printf '[gate]\ndev_critical = 7\n' >"$r/docs/r5/targets.toml"
  printf '# cases\n' >"$r/docs/r5/cases.md"
  printf 'case a\n' >"$r/fixtures/r5/cases/dev/a.jsonl"
  printf 'stat\n' >"$r/tools/r5-trials/stat/s.txt"
  printf 'drift\n' >"$r/tools/r5-trials/drift/d.txt"
  printf 'outside every root\n' >"$r/README.md"
  printf 'heldout cases=24 benign=12 fault=12 sha256=%s\nharness tree=%s\n' "$archive" "$d_tree" >"$r/docs/r5/heldout.txt"
  freeze_text "$d_targets" >"$r/docs/r5/freeze.toml"
  printf '%s\n' 'root	docs/r5/' 'root	scripts/r5/' 'root	scripts/r5-gate/' 'root	formal/r5-admit/' \
    'root	tools/r5-trials/' 'root	fixtures/r5/' 'root	crates/halvo-learn/' 'root	crates/halvo-learn-store/' \
    'root	crates/halvo-learn-gate/' \
    'U0.2	docs/r5/targets.toml' 'U0.3	docs/r5/cases.md' 'U0.3	fixtures/r5/cases/dev/**' \
    'U0.2	tools/r5-trials/stat/**' 'U0.3	tools/r5-trials/drift/*' 'U0.4	docs/r5/freeze.toml' \
    'U0.5	docs/r5/heldout.txt' 'U1.4	scripts/r5/layout.txt' 'U9.1	docs/r5/gate.md' >"$r/scripts/r5/layout.txt"
  commit_all "$r"
}

check() {
  case_name=$1
  want=$2
  text=$3
  shift 3
  out=$(PATH=$path CI_D_REPO=$work/$case_name CI_D_ARTIFACTS=${artifacts:-$work/artifacts-$case_name} sh "$self" "$@" 2>&1)
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")")
      got=${last#"FAIL $name ("}
      got=FAIL-${got%)}
      ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$text"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$case_name" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $case_name"
  elif [ "$want" != PASS ]; then
    rejected=$((rejected + 1))
  fi
  path=$PATH
  artifacts=
}

self_test() {
  self=$0
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || broken
  path=$PATH
  artifacts=
  rejected=0
  wrong=
  line='layout roots=9 patterns=9 files=8 outside=0'

  fixture good
  check good PASS "$line targets=ok learner=none harness=same"
  check good FAIL-none "$line targets=ok learner=none harness=same
layout unmatched=1" --all

  fixture stray-path
  printf 'stray\n' >"$work/stray-path/docs/r5/stray.txt"
  git -C "$work/stray-path" add docs/r5/stray.txt || broken
  check stray-path FAIL-outside 'files=9 outside=1 targets=ok learner=none harness=same'

  fixture stray-below-a-star
  mkdir -p "$work/stray-below-a-star/tools/r5-trials/drift/sub" || broken
  printf 'stray\n' >"$work/stray-below-a-star/tools/r5-trials/drift/sub/x.txt"
  git -C "$work/stray-below-a-star" add tools/r5-trials/drift/sub/x.txt || broken
  check stray-below-a-star FAIL-outside 'files=9 outside=1 targets=miss'

  fixture freeze-targets-differs
  freeze_text "0${d_targets#?}" >"$work/freeze-targets-differs/docs/r5/freeze.toml"
  check freeze-targets-differs FAIL-targets "$line targets=miss learner=none harness=same"

  fixture freeze-differs-and-grep-cannot-run
  freeze_text "0${d_targets#?}" >"$work/freeze-differs-and-grep-cannot-run/docs/r5/freeze.toml"
  mkdir -p "$work/stub-grep" || broken
  printf '#!/bin/sh\nexit 2\n' >"$work/stub-grep/grep"
  chmod +x "$work/stub-grep/grep" || broken
  path=$work/stub-grep:$PATH
  check freeze-differs-and-grep-cannot-run FAIL-targets "$line targets=miss learner=none harness=same"

  fixture targets-file-one-byte
  printf '[gate]\ndev_critical = 8\n' >"$work/targets-file-one-byte/docs/r5/targets.toml"
  check targets-file-one-byte FAIL-targets 'targets=miss'

  fixture drift-file-one-byte
  printf 'drifT\n' >"$work/drift-file-one-byte/tools/r5-trials/drift/d.txt"
  check drift-file-one-byte FAIL-targets 'targets=miss learner=none harness=same'

  fixture frozen-file-is-a-link-in-the-index
  link=$(printf '%s' '../../../README.md' | git -C "$work/frozen-file-is-a-link-in-the-index" hash-object -w --stdin) || broken
  git -C "$work/frozen-file-is-a-link-in-the-index" update-index --cacheinfo "120000,$link,tools/r5-trials/stat/s.txt" || broken
  check frozen-file-is-a-link-in-the-index FAIL-targets "$line targets=miss learner=none harness=same"

  fixture freeze-file-is-a-link-in-the-index
  link=$(printf '%s' '../../README.md' | git -C "$work/freeze-file-is-a-link-in-the-index" hash-object -w --stdin) || broken
  git -C "$work/freeze-file-is-a-link-in-the-index" update-index --cacheinfo "120000,$link,docs/r5/freeze.toml" || broken
  check freeze-file-is-a-link-in-the-index FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture heldout-digits-differ
  printf 'heldout cases=24 benign=12 fault=12 sha256=d%s\nharness tree=%s\n' "${archive#?}" "$d_tree" >"$work/heldout-digits-differ/docs/r5/heldout.txt"
  check heldout-digits-differ FAIL-targets 'targets=miss learner=none harness=same'

  fixture heldout-counts-differ
  printf 'heldout cases=20 benign=10 fault=10 sha256=%s\nharness tree=%s\n' "$archive" "$d_tree" >"$work/heldout-counts-differ/docs/r5/heldout.txt"
  check heldout-counts-differ FAIL-targets "$line targets=miss learner=none harness=same"

  fixture heldout-count-with-a-leading-zero
  printf 'heldout cases=024 benign=12 fault=12 sha256=%s\nharness tree=%s\n' "$archive" "$d_tree" >"$work/heldout-count-with-a-leading-zero/docs/r5/heldout.txt"
  check heldout-count-with-a-leading-zero FAIL-targets "$line targets=miss learner=none harness=same"

  fixture heldout-without-its-final-line-feed
  printf 'heldout cases=24 benign=12 fault=12 sha256=%s\nharness tree=%s' "$archive" "$d_tree" >"$work/heldout-without-its-final-line-feed/docs/r5/heldout.txt"
  check heldout-without-its-final-line-feed FAIL-targets "$line targets=miss learner=none harness=same"

  fixture freeze-keys-out-of-order
  printf '[targets]\ncases = "%s"\ntargets = "%s"\nstat = "%s"\ndrift = "%s"\nheldout = "%s"\n' \
    "$d_cases" "$d_targets" "$d_stat" "$d_drift" "$archive" >"$work/freeze-keys-out-of-order/docs/r5/freeze.toml"
  check freeze-keys-out-of-order FAIL-freeze 'targets=miss learner=miss harness=same'

  fixture learner-same
  mkdir -p "$work/learner-same/crates/halvo-learn/src" || broken
  printf 'pub fn f() {}\n' >"$work/learner-same/crates/halvo-learn/src/lib.rs"
  printf '[learner]\ncrates = "%s"\n' "$d_crates" >>"$work/learner-same/docs/r5/freeze.toml"
  printf 'U1.2\tcrates/halvo-learn/**\n' >>"$work/learner-same/scripts/r5/layout.txt"
  commit_all "$work/learner-same"
  check learner-same PASS 'layout roots=9 patterns=10 files=9 outside=0 targets=ok learner=ok harness=same'

  fixture learner-differs
  mkdir -p "$work/learner-differs/crates/halvo-learn/src" || broken
  printf 'pub fn g() {}\n' >"$work/learner-differs/crates/halvo-learn/src/lib.rs"
  printf '[learner]\ncrates = "%s"\n' "$d_crates" >>"$work/learner-differs/docs/r5/freeze.toml"
  printf 'U1.2\tcrates/halvo-learn/**\n' >>"$work/learner-differs/scripts/r5/layout.txt"
  commit_all "$work/learner-differs"
  check learner-differs FAIL-learner 'targets=ok learner=miss harness=same'

  fixture tree-unreadable no-commit
  check tree-unreadable FAIL-tree "$line targets=ok learner=none harness=differ"

  fixture harness-differs
  printf 'heldout cases=24 benign=12 fault=12 sha256=%s\nharness tree=f%s\n' "$archive" "${d_tree#?}" >"$work/harness-differs/docs/r5/heldout.txt"
  check harness-differs FAIL-harness "$line targets=ok learner=none harness=differ"

  fixture layout-missing
  mv "$work/layout-missing/scripts/r5/layout.txt" "$work/layout-missing/layout.txt.moved" || broken
  check layout-missing FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-pattern-under-no-root
  printf 'U1.3\tcrates/halvo-schema/src/learning.rs\n' >>"$work/layout-pattern-under-no-root/scripts/r5/layout.txt"
  check layout-pattern-under-no-root FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-root-after-pattern
  awk '$0 != "root\tcrates/halvo-learn-gate/"' "$work/layout-root-after-pattern/scripts/r5/layout.txt" >"$work/layout-root-after-pattern.txt" || broken
  printf 'root\tcrates/halvo-learn-gate/\n' >>"$work/layout-root-after-pattern.txt"
  cp "$work/layout-root-after-pattern.txt" "$work/layout-root-after-pattern/scripts/r5/layout.txt" || broken
  check layout-root-after-pattern FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-tenth-root
  awk 'NR == 9 { print; print "root\tnowhere/"; next } { print }' "$work/layout-tenth-root/scripts/r5/layout.txt" >"$work/layout-tenth-root.txt" || broken
  cp "$work/layout-tenth-root.txt" "$work/layout-tenth-root/scripts/r5/layout.txt" || broken
  check layout-tenth-root FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture quoted-name-under-a-double-star
  printf 'U6.1\ttools/r5-trials/driver/**\n' >>"$work/quoted-name-under-a-double-star/scripts/r5/layout.txt"
  mkdir -p "$work/quoted-name-under-a-double-star/tools/r5-trials/driver" || broken
  printf 'x\n' >"$work/quoted-name-under-a-double-star/tools/r5-trials/driver/$(printf 'caf\303\251').txt"
  mkdir -p "$work/quoted-name-under-a-double-star/tools/r5-trials/driver/sub" || broken
  printf 'x\n' >"$work/quoted-name-under-a-double-star/tools/r5-trials/driver/sub/x.txt"
  git -C "$work/quoted-name-under-a-double-star" add -A || broken
  check quoted-name-under-a-double-star PASS 'layout roots=9 patterns=10 files=10 outside=0 targets=ok'

  fixture quoted-name-under-no-pattern
  printf 'x\n' >"$work/quoted-name-under-no-pattern/docs/r5/$(printf 'caf\303\251').md"
  git -C "$work/quoted-name-under-no-pattern" add -A || broken
  check quoted-name-under-no-pattern FAIL-outside 'files=9 outside=1 targets=ok'

  fixture root-spelled-in-another-case
  blob=$(printf 'x\n' | git -C "$work/root-spelled-in-another-case" hash-object -w --stdin) || broken
  git -C "$work/root-spelled-in-another-case" update-index --add --cacheinfo "100644,$blob,Docs/r5/targets.toml" || broken
  check root-spelled-in-another-case FAIL-outside 'files=9 outside=1 targets=ok learner=none harness=same'

  fixture no-freeze-no-heldout
  mv "$work/no-freeze-no-heldout/docs/r5/freeze.toml" "$work/no-freeze-no-heldout/freeze.toml.moved" || broken
  mv "$work/no-freeze-no-heldout/docs/r5/heldout.txt" "$work/no-freeze-no-heldout/heldout.txt.moved" || broken
  check no-freeze-no-heldout FAIL-freeze "$line targets=none learner=none harness=none"

  fixture freeze-with-learner-alone
  printf '[learner]\ncrates = "%s"\n' "$d_crates" >"$work/freeze-with-learner-alone/docs/r5/freeze.toml"
  check freeze-with-learner-alone FAIL-freeze "$line targets=none learner=miss harness=same"

  fixture unknown-argument
  check unknown-argument FAIL-usage 'FAIL 10-r5-layout (usage)' --no-such-argument

  fixture artifacts-directory-cannot-be-made
  printf 'a file\n' >"$work/artifacts-artifacts-directory-cannot-be-made"
  check artifacts-directory-cannot-be-made FAIL-scratch 'FAIL 10-r5-layout (scratch)'

  fixture stray-path-with-a-backslash-in-the-artifacts-path
  printf 'stray\n' >"$work/stray-path-with-a-backslash-in-the-artifacts-path/docs/r5/stray.txt"
  git -C "$work/stray-path-with-a-backslash-in-the-artifacts-path" add docs/r5/stray.txt || broken
  artifacts=$work/back\\slash
  check stray-path-with-a-backslash-in-the-artifacts-path FAIL-outside 'files=9 outside=1 targets=ok learner=none harness=same'

  fixture freeze-without-its-final-line-feed
  freeze_text "$d_targets" | awk '{ printf "%s%s", sep, $0; sep = "\n" }' >"$work/freeze-without-its-final-line-feed/docs/r5/freeze.toml"
  check freeze-without-its-final-line-feed FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture freeze-table-short-before-the-next
  printf '[targets]\ntargets = "%s"\ncases = "%s"\nstat = "%s"\ndrift = "%s"\n[learner]\ncrates = "%s"\n' "$d_targets" "$d_cases" "$d_stat" "$d_drift" "$d_crates" >"$work/freeze-table-short-before-the-next/docs/r5/freeze.toml"
  check freeze-table-short-before-the-next FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture freeze-learner-before-targets
  { printf '[learner]\ncrates = "%s"\n' "$d_crates"; freeze_text "$d_targets"; } >"$work/freeze-learner-before-targets/docs/r5/freeze.toml"
  check freeze-learner-before-targets FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture freeze-key-renamed
  freeze_text "$d_targets" | sed 's/^targets = /tergets = /' >"$work/freeze-key-renamed/docs/r5/freeze.toml"
  check freeze-key-renamed FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture freeze-value-with-a-byte-after-the-quote
  freeze_text "$d_targets" | sed '2s/$/x/' >"$work/freeze-value-with-a-byte-after-the-quote/docs/r5/freeze.toml"
  check freeze-value-with-a-byte-after-the-quote FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture freeze-value-in-upper-case
  freeze_text "$d_targets" | sed '2s/f/F/' >"$work/freeze-value-in-upper-case/docs/r5/freeze.toml"
  check freeze-value-in-upper-case FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture freeze-key-missing-at-the-end
  freeze_text "$d_targets" | sed '$d' >"$work/freeze-key-missing-at-the-end/docs/r5/freeze.toml"
  check freeze-key-missing-at-the-end FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture freeze-with-a-sixth-key
  { freeze_text "$d_targets"; printf 'extra = "%s"\n' "$archive"; } >"$work/freeze-with-a-sixth-key/docs/r5/freeze.toml"
  check freeze-with-a-sixth-key FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture layout-line-with-three-fields
  printf 'U1.1\tdocs/r5/x.md\textra\n' >>"$work/layout-line-with-three-fields/scripts/r5/layout.txt"
  check layout-line-with-three-fields FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-repeated-line
  printf 'U9.1\tdocs/r5/gate.md\n' >>"$work/layout-repeated-line/scripts/r5/layout.txt"
  check layout-repeated-line FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-root-not-of-l13
  awk '$0 == "root\tcrates/halvo-learn-gate/" { print "root\tdocs/r6/"; next } { print }' "$work/layout-root-not-of-l13/scripts/r5/layout.txt" >"$work/layout-root-not-of-l13.txt" || broken
  cp "$work/layout-root-not-of-l13.txt" "$work/layout-root-not-of-l13/scripts/r5/layout.txt" || broken
  check layout-root-not-of-l13 FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-eight-roots
  awk '$0 != "root\tcrates/halvo-learn-gate/"' "$work/layout-eight-roots/scripts/r5/layout.txt" >"$work/layout-eight-roots.txt" || broken
  cp "$work/layout-eight-roots.txt" "$work/layout-eight-roots/scripts/r5/layout.txt" || broken
  check layout-eight-roots FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-first-field-not-a-unit
  printf 'X1.1\tdocs/r5/x.md\n' >>"$work/layout-first-field-not-a-unit/scripts/r5/layout.txt"
  check layout-first-field-not-a-unit FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-pattern-with-a-space
  printf 'U1.1\tdocs/r5/a b.md\n' >>"$work/layout-pattern-with-a-space/scripts/r5/layout.txt"
  check layout-pattern-with-a-space FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-three-stars
  printf 'U1.1\tdocs/r5/***\n' >>"$work/layout-three-stars/scripts/r5/layout.txt"
  check layout-three-stars FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-dot-component
  printf 'U1.1\tdocs/r5/../x.md\n' >>"$work/layout-dot-component/scripts/r5/layout.txt"
  check layout-dot-component FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture layout-without-its-final-line-feed
  awk '{ printf "%s%s", sep, $0; sep = "\n" }' "$work/layout-without-its-final-line-feed/scripts/r5/layout.txt" >"$work/layout-without-its-final-line-feed.txt" || broken
  cp "$work/layout-without-its-final-line-feed.txt" "$work/layout-without-its-final-line-feed/scripts/r5/layout.txt" || broken
  check layout-without-its-final-line-feed FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture stray-that-an-unescaped-dot-would-match
  printf 'stray\n' >"$work/stray-that-an-unescaped-dot-would-match/docs/r5/targetsXtoml"
  git -C "$work/stray-that-an-unescaped-dot-would-match" add docs/r5/targetsXtoml || broken
  check stray-that-an-unescaped-dot-would-match FAIL-outside 'files=9 outside=1 targets=ok learner=none harness=same'

  fixture stray-that-starts-like-a-pattern
  printf 'stray\n' >"$work/stray-that-starts-like-a-pattern/docs/r5/targets.toml.bak"
  git -C "$work/stray-that-starts-like-a-pattern" add docs/r5/targets.toml.bak || broken
  check stray-that-starts-like-a-pattern FAIL-outside 'files=9 outside=1 targets=ok learner=none harness=same'

  fixture quoted-name-beside-a-plain-pattern
  printf 'U9.9\tdocs/r5/caf.md\n' >>"$work/quoted-name-beside-a-plain-pattern/scripts/r5/layout.txt"
  printf 'x\n' >"$work/quoted-name-beside-a-plain-pattern/docs/r5/$(printf 'caf\303\251').md"
  git -C "$work/quoted-name-beside-a-plain-pattern" add -A || broken
  check quoted-name-beside-a-plain-pattern FAIL-outside 'files=9 outside=1 targets=ok'

  fixture quoted-name-in-a-frozen-set
  printf 'x\n' >"$work/quoted-name-in-a-frozen-set/tools/r5-trials/stat/$(printf 'caf\303\251').txt"
  git -C "$work/quoted-name-in-a-frozen-set" add -A || broken
  check quoted-name-in-a-frozen-set FAIL-targets 'files=9 outside=0 targets=miss learner=none harness=same'

  fixture learner-same-with-the-store-crate
  mkdir -p "$work/learner-same-with-the-store-crate/crates/halvo-learn/src" "$work/learner-same-with-the-store-crate/crates/halvo-learn-store/src" || broken
  printf 'pub fn f() {}\n' >"$work/learner-same-with-the-store-crate/crates/halvo-learn/src/lib.rs"
  printf 'pub fn s() {}\n' >"$work/learner-same-with-the-store-crate/crates/halvo-learn-store/src/lib.rs"
  printf '[learner]\ncrates = "%s"\n' "$d_crates_store" >>"$work/learner-same-with-the-store-crate/docs/r5/freeze.toml"
  printf 'U1.2\tcrates/halvo-learn/**\nU2.1\tcrates/halvo-learn-store/**\n' >>"$work/learner-same-with-the-store-crate/scripts/r5/layout.txt"
  commit_all "$work/learner-same-with-the-store-crate"
  check learner-same-with-the-store-crate PASS 'layout roots=9 patterns=11 files=10 outside=0 targets=ok learner=ok harness=same'

  fixture cases-file-one-byte
  printf '# cases!\n' >"$work/cases-file-one-byte/docs/r5/cases.md"
  check cases-file-one-byte FAIL-targets "$line targets=miss learner=none harness=same"

  fixture file-beside-the-stat-directory
  printf 'x\n' >"$work/file-beside-the-stat-directory/tools/r5-trials/stat-extra.txt"
  printf 'U9.9\ttools/r5-trials/stat-extra.txt\n' >>"$work/file-beside-the-stat-directory/scripts/r5/layout.txt"
  git -C "$work/file-beside-the-stat-directory" add -A || broken
  check file-beside-the-stat-directory PASS 'layout roots=9 patterns=10 files=9 outside=0 targets=ok learner=none harness=same'

  fixture targets-file-is-a-link-in-the-index
  link=$(printf '%s' '../../README.md' | git -C "$work/targets-file-is-a-link-in-the-index" hash-object -w --stdin) || broken
  git -C "$work/targets-file-is-a-link-in-the-index" update-index --cacheinfo "120000,$link,docs/r5/targets.toml" || broken
  check targets-file-is-a-link-in-the-index FAIL-targets "$line targets=miss learner=none harness=same"

  fixture cases-file-is-a-link-in-the-index
  link=$(printf '%s' '../../README.md' | git -C "$work/cases-file-is-a-link-in-the-index" hash-object -w --stdin) || broken
  git -C "$work/cases-file-is-a-link-in-the-index" update-index --cacheinfo "120000,$link,docs/r5/cases.md" || broken
  check cases-file-is-a-link-in-the-index FAIL-targets "$line targets=miss learner=none harness=same"

  fixture heldout-is-a-link-in-the-index
  link=$(printf '%s' '../../README.md' | git -C "$work/heldout-is-a-link-in-the-index" hash-object -w --stdin) || broken
  git -C "$work/heldout-is-a-link-in-the-index" update-index --cacheinfo "120000,$link,docs/r5/heldout.txt" || broken
  check heldout-is-a-link-in-the-index FAIL-targets "$line targets=miss learner=none harness=same"

  fixture targets-file-not-in-the-index
  git -C "$work/targets-file-not-in-the-index" update-index --force-remove docs/r5/targets.toml || broken
  check targets-file-not-in-the-index FAIL-targets 'layout roots=9 patterns=9 files=7 outside=0 targets=miss learner=none harness=same'

  fixture stray-path-and-no-freeze
  printf 'stray\n' >"$work/stray-path-and-no-freeze/docs/r5/stray.txt"
  git -C "$work/stray-path-and-no-freeze" add docs/r5/stray.txt || broken
  mv "$work/stray-path-and-no-freeze/docs/r5/freeze.toml" "$work/stray-path-and-no-freeze/freeze.toml.moved" || broken
  check stray-path-and-no-freeze FAIL-outside 'files=9 outside=1 targets=none learner=none harness=same'

  fixture targets-and-learner-differ
  mkdir -p "$work/targets-and-learner-differ/crates/halvo-learn/src" || broken
  printf 'pub fn g() {}\n' >"$work/targets-and-learner-differ/crates/halvo-learn/src/lib.rs"
  { freeze_text "0${d_targets#?}"; printf '[learner]\ncrates = "%s"\n' "$d_crates"; } >"$work/targets-and-learner-differ/docs/r5/freeze.toml"
  printf 'U1.2\tcrates/halvo-learn/**\n' >>"$work/targets-and-learner-differ/scripts/r5/layout.txt"
  commit_all "$work/targets-and-learner-differ"
  check targets-and-learner-differ FAIL-targets 'targets=miss learner=miss harness=same'

  fixture heldout-line-with-a-nul-byte
  printf 'heldout cases=24 benign=12 fault=12 sha256=%s\000 changed\nharness tree=%s\n' "$archive" "$d_tree" >"$work/heldout-line-with-a-nul-byte/docs/r5/heldout.txt"
  check heldout-line-with-a-nul-byte FAIL-targets "$line targets=miss learner=none harness=same"

  fixture freeze-line-with-a-nul-byte
  printf '[targets]\ntargets = "%s"\000 changed\ncases = "%s"\nstat = "%s"\ndrift = "%s"\nheldout = "%s"\n' \
    "$d_targets" "$d_cases" "$d_stat" "$d_drift" "$archive" >"$work/freeze-line-with-a-nul-byte/docs/r5/freeze.toml"
  check freeze-line-with-a-nul-byte FAIL-freeze "$line targets=miss learner=miss harness=same"

  fixture layout-line-with-a-nul-byte
  printf 'U9.9\tdocs/r5/x.md\000y\n' >>"$work/layout-line-with-a-nul-byte/scripts/r5/layout.txt"
  check layout-line-with-a-nul-byte FAIL-layout 'FAIL 10-r5-layout (layout)'

  fixture artifacts-inside-the-repository
  artifacts=$work/artifacts-inside-the-repository/docs/r5
  check artifacts-inside-the-repository FAIL-scratch 'FAIL 10-r5-layout (scratch)'

  fixture artifacts-under-the-target-of-the-repository
  artifacts=$work/artifacts-under-the-target-of-the-repository/target/ci-artifacts/10-r5-layout
  check artifacts-under-the-target-of-the-repository PASS "$line targets=ok learner=none harness=same"

  fixture git-fails
  mkdir -p "$work/stub" || broken
  printf '#!/bin/sh\nexit 1\n' >"$work/stub/git"
  chmod +x "$work/stub/git" || broken
  path=$work/stub:$PATH
  check git-fails FAIL-git 'FAIL 10-r5-layout (git)'

  fixture staged-list-fails
  real_git=$(command -v git) || broken
  mkdir -p "$work/stub-staged" || broken
  printf '#!/bin/sh\n[ "$*" != "-c core.quotepath=on ls-files -s" ] || exit 1\nexec "%s" "$@"\n' "$real_git" >"$work/stub-staged/git"
  chmod +x "$work/stub-staged/git" || broken
  path=$work/stub-staged:$PATH
  check staged-list-fails FAIL-git 'FAIL 10-r5-layout (git)'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --all) measure yes ;;
  '') measure no ;;
  *) finish usage ;;
esac
