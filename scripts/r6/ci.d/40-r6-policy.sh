#!/bin/sh
# jobs: macos windows
# rule: The policy table equals both what route-gate derive writes and the first table of the contract, the router product passes the gate, docs/r6/bar.md holds the two constants, the default cap line is the contract's and a cap file that does not parse gives no cap, and no tracked file sets compiler flags or the test configuration for a build, except scripts/r6/ci.d/80-r6-absent.sh while HEAD holds it as a regular file reached through trees alone whose git blob id is the one that this script names and the work-tree file is that blob.
set -u
LC_ALL=C
export LC_ALL
name=40-r6-policy
policy=crates/halvo-route/data/policy.tsv
cap=crates/halvo-route/data/cap.txt
bar=docs/r6/bar.md
recorded=docs/r6/outcomes/recorded.tsv
freeze=docs/r6/outcomes/freeze.txt
pinned_path=scripts/r6/ci.d/80-r6-absent.sh
pinned_blob=a0cbd1d17f92e6135678879353c57137fc2ac1fb
specimen=crates/halvo-route-gate/tests/data/specimen.tsv
cap_line='default_cap amount=2000 period=month status=provisional'
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
q="'"
word_flags=rust
word_flags=${word_flags}flags
word_cfg=cfg
flag_pattern="${word_flags}|--${word_cfg}[[:space:]=,\"${q}]+test([^[:alnum:]_]|\$)|rustc-${word_cfg}=[\"${q}]?test([^[:alnum:]_]|\$)"

first_table() {
  tr ' ' '\t' <<'EOF'
category kind pick level basis k n
utility_bill_payment compile hand_back - untested 0 0
utility_bill_payment recover hand_back - untested 0 0
utility_bill_payment write hand_back - untested 0 0
utility_bill_payment replay none - no_model 0 0
tenant_rent compile hand_back - no_runtime 0 0
tenant_rent recover hand_back - no_runtime 0 0
tenant_rent write hand_back - no_runtime 0 0
tenant_rent replay none - no_model 0 0
lease_renewal_or_change compile hand_back - no_runtime 0 0
lease_renewal_or_change recover hand_back - no_runtime 0 0
lease_renewal_or_change write hand_back - no_runtime 0 0
lease_renewal_or_change replay none - no_model 0 0
tenant_application_review compile hand_back - no_runtime 0 0
tenant_application_review recover hand_back - no_runtime 0 0
tenant_application_review write hand_back - no_runtime 0 0
tenant_application_review replay none - no_model 0 0
email compile hand_back - untested 0 0
email recover hand_back - untested 0 0
email write hand_back - untested 0 0
email replay none - no_model 0 0
spreadsheet_fill compile hand_back - untested 0 0
spreadsheet_fill recover hand_back - untested 0 0
spreadsheet_fill write hand_back - untested 0 0
spreadsheet_fill replay none - no_model 0 0
shop_order compile hand_back - untested 0 0
shop_order recover hand_back - untested 0 0
shop_order write hand_back - untested 0 0
shop_order replay none - no_model 0 0
calendar_cleanup compile hand_back - untested 0 0
calendar_cleanup recover hand_back - untested 0 0
calendar_cleanup write hand_back - untested 0 0
calendar_cleanup replay none - no_model 0 0
ride_order compile hand_back - no_runtime 0 0
ride_order recover hand_back - no_runtime 0 0
ride_order write hand_back - no_runtime 0 0
ride_order replay none - no_model 0 0
daily_chart_check compile hand_back - no_runtime 0 0
daily_chart_check recover hand_back - no_runtime 0 0
daily_chart_check write hand_back - no_runtime 0 0
daily_chart_check replay none - no_model 0 0
other compile hand_back - not_seed 0 0
other recover hand_back - not_seed 0 0
other write hand_back - not_seed 0 0
other replay none - no_model 0 0
EOF
}

outcome_header() {
  printf 'task\tcategory\tkind\ttier\tmodel\tmachine\tseed\tsplit\tattempt\tpass\tsteps\twall_s\ttokens_in\ttokens_out\tlevel\tsource\n'
}

cannot() {
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 2
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel) || cannot repo
  fi
  cd "$repo" || cannot repo
}

tool() {
  cargo run -q --locked -p halvo-route-gate --bin route-gate -- "$@"
}

bar_value() {
  : >"$scratch/bar-$1.txt"
  if [ -f "$bar" ]; then
    grep -oE "(^|[^A-Za-z0-9_])$1=[A-Za-z0-9_]*" "$bar" >"$scratch/bar-$1.txt"
  fi
  sed 's/.*=//' "$scratch/bar-$1.txt" >"$scratch/bar-$1.all"
  sort -u "$scratch/bar-$1.all" >"$scratch/bar-$1.values"
  count=$(grep -c '' "$scratch/bar-$1.values")
  value=$(cat "$scratch/bar-$1.values")
  case $count:$value in
    1:*[!0-9]* | 1:) printf '%s\n' - ;;
    1:*) printf '%s\n' "$value" ;;
    *) printf '%s\n' - ;;
  esac
}

decoded_hits() {
  PATTERN=$flag_pattern awk '
    function hexval(s, i, v, c) {
      v = 0
      for (i = 1; i <= length(s); i++) {
        c = index("0123456789abcdef", tolower(substr(s, i, 1))) - 1
        if (c < 0) return -1
        v = v * 16 + c
      }
      return v
    }
    function decode(s, out, i, c, n, h, d) {
      out = ""
      i = 1
      while (i <= length(s)) {
        c = substr(s, i, 1)
        n = substr(s, i + 1, 1)
        h = -1
        if (c == "\\" && n == "x" && substr(s, i + 2, 2) ~ /^[0-9A-Fa-f][0-9A-Fa-f]$/) {
          h = hexval(substr(s, i + 2, 2)); d = 4
        } else if (c == "\\" && n == "u" && match(substr(s, i + 2), /^\{[0-9A-Fa-f]+\}/)) {
          h = hexval(substr(s, i + 3, RLENGTH - 2)); d = 2 + RLENGTH
        } else if (c == "\\" && n == "u" && substr(s, i + 2, 4) ~ /^[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]$/) {
          h = hexval(substr(s, i + 2, 4)); d = 6
        } else if (c == "\\" && n == "U" && substr(s, i + 2, 8) ~ /^[0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f][0-9A-Fa-f]$/) {
          h = hexval(substr(s, i + 2, 8)); d = 10
        } else if (c == "\\" && match(substr(s, i + 1), /^0?[0-7][0-7]?[0-7]?/)) {
          h = 0
          for (d = 2; d <= RLENGTH + 1; d++) h = h * 8 + substr(s, i + d - 1, 1)
          d = RLENGTH + 1
        }
        if (h < 0) {
          out = out c
          i++
        } else {
          out = out ((h >= 32 && h < 127) ? sprintf("%c", h) : "?")
          i += d
        }
      }
      return out
    }
    function check(text, file, t) {
      t = tolower(decode(text))
      gsub("[" sprintf("%c%c", 34, 39) "]", "", t)
      if ((t ~ ENVIRON["PATTERN"] || tolower(text) ~ ENVIRON["PATTERN"]) && !(file in seen)) {
        seen[file] = 1
        print "sets " file
      }
    }
    FNR == 1 && held != "" {
      check(held, heldfile)
      held = ""
    }
    {
      line = $0
      sub(/\r$/, "", line)
      if (held != "") {
        sub(/^[ \t]+/, "", line)
        line = held line
        held = ""
      }
      if (line ~ /\\$/) {
        held = substr(line, 1, length(line) - 1)
        heldfile = FILENAME
        next
      }
      check(line, FILENAME)
    }
    END {
      if (held != "") check(held, heldfile)
    }
  ' "$@"
}

pinned_holds() {
  tab=$(printf '\t')
  entry=$(git ls-tree HEAD -- "$pinned_path" 2>/dev/null) || return 1
  case $entry in
    "100644 blob $pinned_blob$tab$pinned_path" | "100755 blob $pinned_blob$tab$pinned_path") ;;
    *) return 1 ;;
  esac
  if [ ! -f "$pinned_path" ]; then
    return 1
  fi
  work_blob=$(git hash-object --no-filters -- "$pinned_path" 2>/dev/null) || return 1
  [ "$work_blob" = "$pinned_blob" ]
}

pin_report() {
  tab=$(printf '\t')
  entry=$(git ls-tree HEAD -- "$pinned_path" 2>/dev/null) || entry=
  head_entry=absent
  if [ -n "$entry" ]; then
    read -r entry_mode entry_type entry_id <<EOF
${entry%%"$tab"*}
EOF
    head_entry=$entry_mode:$entry_type:$entry_id
  fi
  work_entry=absent
  if [ -f "$pinned_path" ]; then
    work_entry=$(git hash-object --no-filters -- "$pinned_path" 2>/dev/null) || work_entry=absent
  fi
  printf 'pin path=%s want=%s head=%s work=%s\n' "$pinned_path" "$pinned_blob" "$head_entry" "$work_entry"
}

scan_flags() {
  exempt=
  if pinned_holds; then
    exempt=$pinned_path
  fi
  git -c core.quotepath=off ls-files -z >"$scratch/tracked.z" || return 2
  tr '\000' '\n' <"$scratch/tracked.z" >"$scratch/tracked.txt" || return 2
  found=0
  grep -E '(^|/)\.cargo/config(\.toml)?$' "$scratch/tracked.txt" >"$scratch/config.txt"
  if [ -s "$scratch/config.txt" ]; then
    sed 's/^/config /' "$scratch/config.txt"
    found=1
  fi
  grep -E '\.(sh|bash|zsh|ps1|psm1|bat|cmd|py|rb|pl|lua|js|mjs|cjs|ts|mts|cts|toml|mk)$|(^|/)(build\.rs|Makefile|GNUmakefile|makefile|justfile|Justfile|\.justfile|package\.json)$|^\.githooks/|^\.github/' \
    "$scratch/tracked.txt" >"$scratch/scan.txt"
  git grep -l -z -I -E -e '^#! ?/' >"$scratch/interpreter.z"
  [ $? -le 1 ] || return 2
  tr '\000' '\n' <"$scratch/interpreter.z" >"$scratch/interpreter.txt" || return 2
  while IFS= read -r file; do
    first=
    IFS= read -r first <"$file"
    case $first in
      '#!/'* | '#! /'*) printf '%s\n' "$file" >>"$scratch/scan.txt" ;;
    esac
  done <"$scratch/interpreter.txt"
  grep -E '(^|/)Cargo\.toml$' "$scratch/tracked.txt" >"$scratch/manifests.txt"
  while IFS= read -r manifest; do
    dir=${manifest%Cargo.toml}
    sed -n "s/^[[:space:]]*build[[:space:]]*=[[:space:]]*[\"$q]\\([^\"$q]*\\)[\"$q].*/\\1/p" "$manifest" >"$scratch/build.txt"
    while IFS= read -r script; do
      printf '%s%s\n' "$dir" "$script" >>"$scratch/scan.txt"
    done <"$scratch/build.txt"
  done <"$scratch/manifests.txt"
  sort -u "$scratch/scan.txt" >"$scratch/scan-set.txt"
  set --
  while IFS= read -r file; do
    [ -f "$file" ] && set -- "$@" "$file"
  done <"$scratch/scan-set.txt"
  printf 'read %s\n' "$#" >"$scratch/scan-count.txt"
  if [ "$#" -gt 0 ]; then
    git grep -l -z -a -i -E -e "$flag_pattern" -- "$@" >"$scratch/hits.z"
    case $? in
      0)
        tr '\000' '\n' <"$scratch/hits.z" >"$scratch/hits.txt" || return 2
        while IFS= read -r hit; do
          [ "$hit" = "$exempt" ] && continue
          printf 'sets %s\n' "$hit"
          found=1
        done <"$scratch/hits.txt"
        ;;
      1) ;;
      *) return 2 ;;
    esac
    decoded_hits "$@" >"$scratch/decoded.txt" || return 2
    while IFS= read -r hit; do
      [ "$hit" = "sets $exempt" ] && continue
      printf '%s\n' "$hit"
      found=1
    done <"$scratch/decoded.txt"
  fi
  return "$found"
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  mkdir -p "$artifacts" || cannot directory
  scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || cannot directory
  cause=

  if [ -f "$recorded" ]; then
    table=$recorded
    want_table=recorded
    set -- --freeze "$freeze"
  else
    table=$scratch/header.tsv
    want_table=generated
    outcome_header >"$table" || cannot directory
    set --
  fi
  tool derive --table "$table" "$@" --out "$scratch/derived.tsv" \
    >"$artifacts/derive.out" 2>"$artifacts/derive.err"
  derive_code=$?
  rows=0
  if [ -f "$policy" ]; then
    lines=$(grep -c '' "$policy")
    [ "$lines" -gt 0 ] && rows=$((lines - 1))
  fi
  same=no
  if [ "$derive_code" -eq 0 ] && cmp -s "$scratch/derived.tsv" "$policy"; then
    if [ -f "$recorded" ]; then
      same=yes
    else
      first_table >"$scratch/first.tsv" || cannot directory
      cmp -s "$scratch/first.tsv" "$policy" && same=yes
    fi
  fi
  printf 'policy rows=%s same=%s\n' "$rows" "$same"
  [ "$same" = yes ] || cause=derive

  if [ -f "$recorded" ]; then
    set -- --table "$recorded" --router product --policy "$policy" --freeze "$freeze"
  else
    tool derive --table "$specimen" --out "$scratch/specimen-policy.tsv" \
      >"$artifacts/derive-specimen.out" 2>"$artifacts/derive-specimen.err"
    set -- --table "$specimen" --router product --policy "$scratch/specimen-policy.tsv"
  fi
  tool score "$@" >"$artifacts/score.out" 2>"$artifacts/score.err"
  score_code=$?
  grep -E '^(cell|category|versus|gate) ' "$artifacts/score.out"
  last=$(tail -n 1 "$artifacts/score.out")
  verdict="^gate table=$want_table router=product cells=44 routed=[0-9]+ handed_back=[0-9]+ derivation=same held=ok result=pass\$"
  if [ "$score_code" -ne 0 ] || ! printf '%s\n' "$last" | grep -qE "$verdict"; then
    cause=${cause:-score}
  fi

  dev=$(bar_value LINE_DEV_MIN)
  held=$(bar_value LINE_HELD_MIN)
  ok=no
  [ "$dev:$held" = 4:2 ] && ok=yes
  printf 'bar dev_min=%s held_min=%s ok=%s\n' "$dev" "$held" "$ok"
  [ "$ok" = yes ] || cause=${cause:-bar}

  printf '%s\n' "$cap_line" >"$scratch/cap.txt" || cannot directory
  line=differs
  cmp -s "$scratch/cap.txt" "$cap" && line=same
  printf 'cap line=%s\n' "$line"
  [ "$line" = same ] || cause=${cause:-cap}

  if ! scan_flags >"$artifacts/flags.txt" 2>"$artifacts/flags.err"; then
    cause=${cause:-flags}
    if grep -qxF "sets $pinned_path" "$artifacts/flags.txt"; then
      pin_report
    fi
  fi

  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

make_repo() {
  mkdir -p "$1/crates/halvo-route/data" "$1/docs/r6" "$1/crates/halvo-route-gate/tests/data" || exit 2
  first_table >"$1/$policy" || exit 2
  printf '%s\n' "$cap_line" >"$1/$cap" || exit 2
  printf '# Bar\n\nLINE_DEV_MIN=4 and LINE_HELD_MIN=2.\n' >"$1/$bar" || exit 2
  printf 'specimen\n' >"$1/$specimen" || exit 2
  git -C "$1" init -q || exit 2
  git -C "$1" add -A || exit 2
}

stub_reset() {
  STUB_DERIVE_EXIT=0
  STUB_DERIVED=$work/first.tsv
  STUB_WANT_FREEZE=no
  STUB_SCORE='cell category=other kind=replay pick=none
gate table=generated router=product cells=44 routed=23 handed_back=7 derivation=same held=ok result=pass'
  STUB_SCORE_EXIT=0
}

check() {
  out=$(
    PATH=$work/bin:$PATH
    CI_D_REPO=$2
    CI_D_ARTIFACTS=$work/artifacts/$1
    export PATH CI_D_REPO CI_D_ARTIFACTS STUB_DERIVE_EXIT STUB_DERIVED STUB_WANT_FREEZE STUB_SCORE STUB_SCORE_EXIT
    sh "$self" 2>&1
  )
  code=$?
  last=$(printf '%s\n' "$out" | tail -n 1)
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  case $out in
    *"$4"*) ;;
    *) got=$got-without-the-text ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$3" "$got"
  if [ "$got" != "$3" ]; then
    wrong="$wrong $1"
  elif [ "$3" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
  stub_reset
}

flags_case() {
  label=$1
  want=$2
  shift 2
  dir=$work/flags/$label
  make_repo "$dir"
  while [ "$#" -ge 2 ]; do
    mkdir -p "$(dirname "$dir/$1")" || exit 2
    printf '%s\n' "$2" >"$dir/$1" || exit 2
    git -C "$dir" add -- "$1" || exit 2
    shift 2
  done
  if [ "$want" = PASS ]; then
    check "$label" "$dir" PASS "PASS $name"
  else
    check "$label" "$dir" FAIL "FAIL $name (flags)"
  fi
}

pinned_case() {
  label=$1
  want=$2
  dir=$work/flags/$label
  mkdir -p "$dir/scripts/r6/ci.d" || exit 2
  git -C "$dir" init -q || exit 2
  case $label in
    pinned-bytes-at-another-path) cp "$pinned_path" "$dir/scripts/r6/ci.d/81-r6-copy.sh" || exit 2 ;;
    pinned-name-in-another-directory) cp "$pinned_path" "$dir/scripts/r6/80-r6-absent.sh" || exit 2 ;;
    pinned-path-in-another-letter-case) cp "$pinned_path" "$dir/scripts/r6/ci.d/80-R6-absent.sh" || exit 2 ;;
    *) cp "$pinned_path" "$dir/$pinned_path" || exit 2 ;;
  esac
  case $label in
    pinned-path-with-one-byte-more | pinned-bytes-in-the-work-tree-alone) printf '\n' >>"$dir/$pinned_path" || exit 2 ;;
    pinned-beside-a-second-script) printf 'export %s%s=x\n' RUST FLAGS >"$dir/scripts/x.sh" || exit 2 ;;
    pinned-beside-an-escaped-second-script) printf 'export %s\\x46%s=x\n' RUST LAGS >"$dir/scripts/x.sh" || exit 2 ;;
    pinned-beside-a-second-script-with-a-nul) printf 'x\000 export %s%s=x\n' RUST FLAGS >"$dir/scripts/x.sh" || exit 2 ;;
  esac
  git -C "$dir" add -A || exit 2
  case $label in
    pinned-path-as-a-symbolic-link)
      link=$(printf '../../../pinned-target.sh' | git -C "$dir" hash-object -w --stdin) || exit 2
      git -C "$dir" update-index --cacheinfo "120000,$link,$pinned_path" || exit 2
      ;;
    pinned-blob-under-the-mode-of-a-link)
      git -C "$dir" update-index --cacheinfo "120000,$pinned_blob,$pinned_path" || exit 2
      ;;
  esac
  git -C "$dir" commit -q -m fixture || exit 2
  case $label in
    pinned-bytes-in-the-work-tree-alone) cp "$pinned_path" "$dir/$pinned_path" || exit 2 ;;
    pinned-bytes-in-the-commit-alone) printf '\n' >>"$dir/$pinned_path" || exit 2 ;;
  esac
  (cd "$dir" && scratch=$work/flags/$label.scan && mkdir "$scratch" && scan_flags >"$scratch.out" 2>&1)
  code=$?
  got=exit-$code
  [ "$code" -eq 1 ] && got=FAIL
  [ "$code" -eq 0 ] && got=PASS
  case $label in
    pinned-beside-*)
      grep -qx 'sets scripts/x.sh' "$work/flags/$label.scan.out" || got=$got-without-the-second-script
      grep -q "^sets $pinned_path\$" "$work/flags/$label.scan.out" && got=$got-naming-the-pinned-script
      ;;
  esac
  printf 'case %s/%s want=%s got=%s\n' "$name" "$label" "$want" "$got"
  if [ "$got" != "$want" ]; then
    wrong="$wrong $label"
  elif [ "$want" = FAIL ]; then
    rejected=$((rejected + 1))
  fi
}

cap_build() {
  copy=$work/cap/halvo-route
  if [ ! -d "$copy" ]; then
    mkdir -p "$copy/examples" || exit 2
    cp -R crates/halvo-route/src crates/halvo-route/data "$copy/" || exit 2
    awk '{ line = $0; sub(/\r$/, "", line) } line == "[dev-dependencies]" { skip = 1; next } /^\[/ { skip = 0 } !skip' crates/halvo-route/Cargo.toml >"$copy/Cargo.toml" || exit 2
    printf '\n[workspace]\n' >>"$copy/Cargo.toml" || exit 2
    cat >"$copy/Cargo.lock" <<'EOF'
# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 4

[[package]]
name = "halvo-route"
version = "0.0.0"
EOF
    cat >"$copy/examples/cap_probe.rs" <<'EOF'
fn main() {
    match halvo_route::release::cap() {
        Some(cap) => println!(
            "cap amount={} period={} status={}",
            cap.amount,
            cap.period.as_str(),
            cap.status.as_str()
        ),
        None => println!("cap none"),
    }
}
EOF
  fi
  cp "$2" "$copy/data/cap.txt" || exit 2
  got=$(cargo run -q --locked --manifest-path "$copy/Cargo.toml" --target-dir "$work/cap/target" --example cap_probe 2>"$work/cap/$1.err")
  code=$?
  printf 'case %s/cap-build-%s want=%s got=%s\n' "$name" "$1" "$3" "$code:$got"
  if [ "$code:$got" != "0:$3" ]; then
    wrong="$wrong cap-build-$1"
  elif [ "$3" = "cap none" ]; then
    rejected=$((rejected + 1))
  fi
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  mkdir -p "$work/bin" "$work/artifacts" "$work/flags" "$work/cap" || exit 2
  first_table >"$work/first.tsv" || exit 2
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
case $* in
  'run -q --locked -p halvo-route-gate --bin route-gate -- derive '*)
    shift 9
    table=
    frozen=
    out=
    while [ "$#" -ge 2 ]; do
      case $1 in
        --table) table=$2 ;;
        --freeze) frozen=$2 ;;
        --out) out=$2 ;;
      esac
      shift 2
    done
    case $table in
      *specimen.tsv)
        printf 'specimen policy\n' >"$out"
        exit 0
        ;;
    esac
    case $STUB_WANT_FREEZE:$frozen in
      yes: | no:?*) exit 97 ;;
    esac
    if [ "$STUB_DERIVE_EXIT" != 0 ]; then
      printf 'REFUSED usage: not built\n' >&2
      exit "$STUB_DERIVE_EXIT"
    fi
    cp "$STUB_DERIVED" "$out"
    ;;
  'run -q --locked -p halvo-route-gate --bin route-gate -- score '*)
    case $STUB_WANT_FREEZE:$* in
      yes:*--freeze*) ;;
      yes:*) exit 97 ;;
    esac
    printf '%s\n' "$STUB_SCORE"
    exit "$STUB_SCORE_EXIT"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/cargo" || exit 2
  rejected=0
  wrong=
  stub_reset
  base=$work/base
  make_repo "$base"

  check all-hold "$base" PASS "policy rows=44 same=yes
cell category=other kind=replay pick=none
gate table=generated router=product cells=44 routed=23 handed_back=7 derivation=same held=ok result=pass
bar dev_min=4 held_min=2 ok=yes
cap line=same
PASS $name"

  STUB_DERIVE_EXIT=2
  check derive-refused "$base" FAIL "policy rows=44 same=no"
  sed '2s/untested/line_missed/' "$work/first.tsv" >"$work/changed.tsv" || exit 2
  STUB_DERIVED=$work/changed.tsv
  check derive-differs-from-the-file "$base" FAIL "FAIL $name (derive)"
  cp "$work/changed.tsv" "$base/$policy" || exit 2
  STUB_DERIVED=$work/changed.tsv
  check file-and-derive-agree-off-the-contract "$base" FAIL "policy rows=44 same=no"
  sed '$d' "$work/first.tsv" >"$base/$policy" || exit 2
  sed '$d' "$work/first.tsv" >"$work/short.tsv" || exit 2
  STUB_DERIVED=$work/short.tsv
  check forty-three-rows "$base" FAIL "policy rows=43 same=no"
  cp "$work/first.tsv" "$base/$policy" || exit 2

  recorded_repo=$work/recorded
  make_repo "$recorded_repo"
  mkdir -p "$recorded_repo/docs/r6/outcomes" || exit 2
  outcome_header >"$recorded_repo/$recorded" || exit 2
  printf 'freeze rows=0 models=0 sources=2 date=2026-11-10\non_device driver=none writer=none\n' >"$recorded_repo/$freeze" || exit 2
  cp "$work/changed.tsv" "$recorded_repo/$policy" || exit 2
  STUB_WANT_FREEZE=yes
  STUB_DERIVED=$work/changed.tsv
  STUB_SCORE='gate table=recorded router=product cells=44 routed=0 handed_back=30 derivation=same held=ok result=pass'
  check recorded-table-with-its-freeze "$recorded_repo" PASS "policy rows=44 same=yes"
  STUB_WANT_FREEZE=yes
  STUB_DERIVED=$work/first.tsv
  STUB_SCORE='gate table=recorded router=product cells=44 routed=0 handed_back=30 derivation=same held=ok result=pass'
  check recorded-table-derives-otherwise "$recorded_repo" FAIL "FAIL $name (derive)"
  STUB_WANT_FREEZE=yes
  STUB_DERIVED=$work/changed.tsv
  check recorded-table-scored-as-generated "$recorded_repo" FAIL "FAIL $name (score)"

  STUB_SCORE='gate table=generated router=product cells=44 routed=23 handed_back=7 derivation=differs held=ok result=fail'
  STUB_SCORE_EXIT=1
  check score-fails "$base" FAIL "derivation=differs held=ok result=fail"
  STUB_SCORE='gate table=generated router=product cells=44 routed=23 handed_back=7 derivation=same held=ok result=pass'
  STUB_SCORE_EXIT=1
  check score-exit-1 "$base" FAIL "FAIL $name (score)"
  STUB_SCORE='gate table=generated router=derived cells=44 routed=23 handed_back=7 derivation=same held=ok result=pass'
  check score-of-another-router "$base" FAIL "FAIL $name (score)"
  STUB_SCORE='gate table=generated router=product cells=44 routed=23 handed_back=7 derivation=same held=ok result=pass extra=1'
  check verdict-with-another-key "$base" FAIL "FAIL $name (score)"
  STUB_SCORE='gate fault=router category=email kind=write'
  STUB_SCORE_EXIT=1
  check router-panicked "$base" FAIL "gate fault=router category=email kind=write"

  printf '# Bar\n\nLINE_DEV_MIN=5 and LINE_HELD_MIN=2.\n' >"$base/$bar" || exit 2
  check bar-dev-five "$base" FAIL "bar dev_min=5 held_min=2 ok=no"
  printf '# Bar\n\nLINE_DEV_MIN=4 and two held-out rows.\n' >"$base/$bar" || exit 2
  check bar-without-held "$base" FAIL "bar dev_min=4 held_min=- ok=no"
  printf '# Bar\n\nLINE_DEV_MIN=4, LINE_HELD_MIN=2 and LINE_DEV_MIN=40.\n' >"$base/$bar" || exit 2
  check bar-two-values "$base" FAIL "bar dev_min=- held_min=2 ok=no"
  printf '# Bar\n\nLINE_DEV_MIN=4x and LINE_HELD_MIN=2.\n' >"$base/$bar" || exit 2
  check bar-value-not-a-number "$base" FAIL "FAIL $name (bar)"
  printf '# Bar\n\nMY_LINE_DEV_MIN=4 and LINE_HELD_MIN=2.\n' >"$base/$bar" || exit 2
  check bar-word-inside-another "$base" FAIL "bar dev_min=- held_min=2 ok=no"
  mv "$base/$bar" "$work/bar.md" || exit 2
  check bar-missing "$base" FAIL "bar dev_min=- held_min=- ok=no"
  printf '# Bar\n\nLINE_DEV_MIN=4 and LINE_HELD_MIN=2.\n' >"$base/$bar" || exit 2

  printf 'default_cap amount=2001 period=month status=provisional\n' >"$base/$cap" || exit 2
  check cap-amount "$base" FAIL "cap line=differs"
  printf '%s' "$cap_line" >"$base/$cap" || exit 2
  check cap-without-lf "$base" FAIL "FAIL $name (cap)"
  printf '%s\r\n' "$cap_line" >"$base/$cap" || exit 2
  check cap-crlf "$base" FAIL "FAIL $name (cap)"
  printf '%s\n' "$cap_line" >"$base/$cap" || exit 2

  setting=$(printf '%s%s' RUST FLAGS)
  encoded=$(printf 'CARGO_ENCODED_%s' "$setting")
  cfg_test=$(printf -- '--%s test' "$word_cfg")
  cfg_eq=$(printf -- '--%s=test' "$word_cfg")
  build_cfg=$(printf 'rustc-%s=test' "$word_cfg")
  flags_case config-toml FAIL .cargo/config.toml '[build]'
  flags_case config-legacy FAIL crates/x/.cargo/config 'jobs = 1'
  flags_case workflow-env FAIL .github/workflows/ci.yml "    env:
      $setting: -D warnings"
  flags_case workflow-encoded FAIL .github/workflows/ci.yml "      $encoded: x"
  flags_case script-cfg-test FAIL scripts/x.sh "cargo rustc -p halvo-route -- $cfg_test"
  flags_case script-cfg-equals FAIL scripts/x.sh "cargo rustc -p halvo-route -- $cfg_eq"
  flags_case toml-array FAIL ci/flags.toml "flags = [\"--$word_cfg\", \"test\"]"
  flags_case build-script FAIL crates/x/build.rs "fn main() { println!(\"cargo:$build_cfg\"); }"
  flags_case build-script-new-form FAIL crates/x/build.rs "fn main() { println!(\"cargo::$build_cfg\"); }"
  flags_case named-build-script FAIL crates/x/Cargo.toml "[package]
build = \"gen/make.rs\"" crates/x/gen/make.rs "fn main() { println!(\"cargo:$build_cfg\"); }"
  flags_case config-key-lower-case FAIL scripts/y.sh "cargo build --config 'build.$word_flags=[\"-Dx\"]'"
  flags_case interpreter-line FAIL tools/run "#!/bin/sh
$setting=x cargo build"
  flags_case interpreter-line-with-a-space FAIL tools/go "#! /bin/sh
$setting=x cargo build"
  flags_case composite-action FAIL .github/actions/build/action.yml "runs:
  using: composite
  steps:
    - run: cargo build
      shell: bash
      env:
        $setting: -D warnings"
  drawn=$(mktemp -u "$work/XXXXXX") || exit 2
  flags_case github-other-file FAIL ".github/${work##*.}/${drawn##*/}" "$setting=-D warnings"
  flags_case hook FAIL .githooks/pre-commit "export $setting=x"
  flags_case package-json FAIL package.json "{\"scripts\":{\"b\":\"$setting=x tauri build\"}}"
  flags_case power-shell FAIL scripts/w.ps1 "\$env:$setting = 'x'"
  flags_case python FAIL tools/p.py "os.environ[\"$setting\"] = \"x\""
  flags_case batch FAIL tools/b.bat "set $setting=x"
  flags_case javascript FAIL tools/j.js "process.env.$setting = 'x'"
  flags_case typescript FAIL tools/t.ts "process.env.$setting = 'x'"
  flags_case makefile FAIL Makefile "export $setting = x"
  flags_case cfg-value-in-single-quotes FAIL scripts/q.sh "cargo rustc -p halvo-route -- $(printf -- "--%s 'test'" "$word_cfg")"
  flags_case yaml-escape FAIL .github/workflows/ci.yml "    env:
      $(printf '"%s\\x46%s": x' RUST LAGS)"
  flags_case toml-unicode-escape FAIL ci/c.toml "$(printf '"%s\\u0066%s" = [1]' rust lags)"
  flags_case rust-escape FAIL crates/x/build.rs "$(printf 'fn main() { println!("cargo:rustc-%s=\\x74est"); }' "$word_cfg")"
  flags_case rust-braced-escape FAIL crates/x/build.rs "$(printf 'fn main() { println!("cargo:rustc-%s=\\u{74}est"); }' "$word_cfg")"
  flags_case octal-escape FAIL scripts/o.sh "$(printf 'export %s\\106%s=x' RUST LAGS)"
  flags_case line-continuation FAIL scripts/c.sh "$(printf 'cargo rustc -- --%s \\\n  test' "$word_cfg")"
  flags_case split-quotes FAIL scripts/s.sh "$(printf 'export "%s""%s=x"' RUST FLAGS)"
  flags_case single-quoted-build-path FAIL crates/x/Cargo.toml "[package]
build = 'gen/make.rs'" crates/x/gen/make.rs "fn main() { println!(\"cargo:$build_cfg\"); }"
  flags_case name-with-a-backslash FAIL 'scripts/a\b.sh' "export $setting=x"
  nul=$work/flags/nul-byte
  make_repo "$nul"
  mkdir -p "$nul/tools" || exit 2
  printf '// a\000b process.env.%s = 1\n' "$setting" >"$nul/tools/n.js" || exit 2
  git -C "$nul" add -- tools/n.js || exit 2
  check nul-byte "$nul" FAIL "FAIL $name (flags)"
  flags_case this-check-as-it-is PASS "scripts/r6/ci.d/$name.sh" "$(cat "$self")"
  flags_case other-words PASS scripts/z.sh "cargo rustc -- --${word_cfg} testing" crates/z/build.rs "fn main() { println!(\"cargo:rustc-${word_cfg}=test_hosted\"); }"
  for label in pinned-bytes-at-another-path pinned-name-in-another-directory pinned-path-with-one-byte-more \
    pinned-bytes-in-the-work-tree-alone pinned-bytes-in-the-commit-alone pinned-path-as-a-symbolic-link \
    pinned-blob-under-the-mode-of-a-link pinned-path-in-another-letter-case pinned-beside-a-second-script \
    pinned-beside-an-escaped-second-script pinned-beside-a-second-script-with-a-nul; do
    pinned_case "$label" FAIL
  done
  pinned_case pinned-bytes-at-the-path PASS
  report=$work/flags/pin-report
  make_repo "$report"
  mkdir -p "$report/scripts/r6/ci.d" || exit 2
  cp "$pinned_path" "$report/$pinned_path" || exit 2
  printf '\n' >>"$report/$pinned_path" || exit 2
  git -C "$report" add -A || exit 2
  git -C "$report" commit -q -m fixture || exit 2
  found_blob=$(git -C "$report" rev-parse "HEAD:$pinned_path") || exit 2
  check failure-line-names-the-ids "$report" FAIL "pin path=$pinned_path want=$pinned_blob head=100644:blob:$found_blob work=$found_blob"
  flags_case words-in-a-document PASS docs/notes.md "Never set $setting or $cfg_test."
  flags_case untracked-config PASS
  mkdir -p "$work/flags/untracked-config/.cargo" || exit 2
  printf '[build]\n' >"$work/flags/untracked-config/.cargo/config.toml" || exit 2
  check untracked-config-file "$work/flags/untracked-config" PASS "PASS $name"

  printf 'default_cap amount=2001 period=month status=provisional\n' >"$base/$cap" || exit 2
  STUB_DERIVE_EXIT=2
  check two-causes-name-the-first "$base" FAIL "FAIL $name (derive)"
  printf '%s\n' "$cap_line" >"$base/$cap" || exit 2
  STUB_DERIVE_EXIT=2
  STUB_SCORE_EXIT=1
  check derive-and-score-name-the-derive "$base" FAIL "FAIL $name (derive)"
  printf '# Bar\n\nLINE_DEV_MIN=5 and LINE_HELD_MIN=2.\n' >"$base/$bar" || exit 2
  STUB_SCORE_EXIT=1
  check score-and-bar-name-the-score "$base" FAIL "FAIL $name (score)"
  printf '# Bar\n\nLINE_DEV_MIN=4 and LINE_HELD_MIN=2.\n' >"$base/$bar" || exit 2
  both=$work/flags/flags-and-cap
  make_repo "$both"
  mkdir -p "$both/.cargo" || exit 2
  printf '[build]\n' >"$both/.cargo/config.toml" || exit 2
  git -C "$both" add -- .cargo/config.toml || exit 2
  printf 'default_cap amount=2001 period=month status=provisional\n' >"$both/$cap" || exit 2
  check flags-and-cap-name-the-cap "$both" FAIL "FAIL $name (cap)"

  usage=$(sh "$self" --all 2>&1)
  code=$?
  if [ "$code:$usage" = "1:FAIL $name (usage)" ]; then
    printf 'case %s/unknown-argument want=FAIL got=FAIL\n' "$name"
    rejected=$((rejected + 1))
  else
    printf 'case %s/unknown-argument want=FAIL got=exit-%s\n' "$name" "$code"
    wrong="$wrong unknown-argument"
  fi

  printf '%s\n' "$cap_line" >"$work/cap/line.txt" || exit 2
  printf 'default_cap amount=02000 period=month status=provisional\n' >"$work/cap/zero.txt" || exit 2
  printf '%s\r\n' "$cap_line" >"$work/cap/crlf.txt" || exit 2
  : >"$work/cap/empty.txt" || exit 2
  cap_build committed "$cap" 'cap amount=2000 period=month status=provisional'
  cap_build contract-line "$work/cap/line.txt" 'cap amount=2000 period=month status=provisional'
  cap_build leading-zero "$work/cap/zero.txt" 'cap none'
  cap_build crlf "$work/cap/crlf.txt" 'cap none'
  cap_build empty "$work/cap/empty.txt" 'cap none'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  '') measure ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
