#!/bin/sh
# MS-1 step 1: time the apply-only phase of scripts/r1-gate/mutation-test.sh in
# two forms on one runner, the type-check form first, each from a fresh clone.
# Prints counts, seconds and runner lines only.
set -u
# shellcheck disable=SC2086
repo=$(pwd -P)
base=${RUNNER_TEMP:-}
[ -n "$base" ] || base=${TMPDIR:-/tmp}
root=$(mktemp -d "${base%/}/ms1.XXXXXX") || exit 2
head=$(git -C "$repo" rev-parse HEAD) || exit 2
if [ -d "$repo/target/r1-gate" ]; then cached=yes; else cached=no; fi
printf 'ms1 target-r1-gate-present=%s cores=%s\n' "$cached" "$(sysctl -n hw.ncpu 2>/dev/null || echo unknown)"

git -C "$repo" ls-files -z -- '*mutants.rows' | tr '\0' '\n' > "$root/rowsfiles" || exit 2
: > "$root/kinds"
while IFS= read -r rf; do
  dir=${rf%/*}; [ "$dir" != "$rf" ] || dir=.
  awk -F '\t' -v dir="$dir" 'NF == 6 {
      f = ($5 == "transitional" || $5 == "control") ? $2 : dir "/" $2
      sub(/^\.\//, "", f)
      k = (f ~ /^(crates|src-tauri)\// || f == "Cargo.toml" || f == "Cargo.lock") ? "cargo" : "other"
      if (k == "cargo" && (f ~ /^crates\/halvo-sqlcipher\// || f ~ /(^|\/)build\.rs$/)) k = "cargo-kept-build"
      print $1 "\t" k }' "$repo/$rf" >> "$root/kinds" || exit 2
done < "$root/rowsfiles"
printf 'ms1 rows=%s cargo=%s kept-build=%s other=%s\n' \
  "$(wc -l < "$root/kinds" | tr -d ' ')" \
  "$(awk -F '\t' '$2 == "cargo"' "$root/kinds" | wc -l | tr -d ' ')" \
  "$(awk -F '\t' '$2 == "cargo-kept-build"' "$root/kinds" | wc -l | tr -d ' ')" \
  "$(awk -F '\t' '$2 == "other"' "$root/kinds" | wc -l | tr -d ' ')"

make_clone() {
  clone=$root/$1
  git clone -q --shared --no-checkout "$repo" "$clone" > /dev/null 2>&1 || return 1
  git -C "$clone" checkout -q --detach "$head" > /dev/null 2>&1 || return 1
  if [ "$1" = check ]; then
    perl -0pi -e '$n = s/cargo build -q --locked -p halvo-capture --bin halvo-capture/cargo "\${MS1_FORM:-check}" -q --locked -p halvo-capture --bin halvo-capture/g; die "lib.sh: $n\n" unless $n == 1' "$clone/scripts/r1-gate/lib.sh" || return 1
    perl -0pi -e '$n = s/  if needs_cargo "\$1"; then tree_build "\$work\/build.out"; return; fi\n/  if needs_cargo "\$1"; then\n    case \$1 in crates\/halvo-sqlcipher\/*|build.rs|*\/build.rs) MS1_FORM=build; export MS1_FORM ;; esac\n    tree_build "\$work\/build.out"; ms1_status=\$?; unset MS1_FORM; return "\$ms1_status"\n  fi\n/g; die "mutation-test.sh: $n\n" unless $n == 1' "$clone/scripts/r1-gate/mutation-test.sh" || return 1
    git -C "$clone" add scripts/r1-gate/lib.sh scripts/r1-gate/mutation-test.sh || return 1
    tree=$(git -C "$clone" write-tree) || return 1
    commit=$(printf 'tree %s\nparent %s\nauthor ms1 <ms1@example.invalid> 1 +0000\ncommitter ms1 <ms1@example.invalid> 1 +0000\n\nms1 check form\n' "$tree" "$head" | git -C "$clone" hash-object -t commit -w --stdin) || return 1
    git -C "$clone" update-ref HEAD "$commit" || return 1
    printf 'ms1 form=check patched-lines=%s\n' "$(git -C "$clone" diff --numstat "$head" HEAD | awk '{ n += $1 } END { print n + 0 }')"
  fi
  [ -z "$(git -C "$clone" status --porcelain)" ] || return 1
  if [ "$cached" = yes ]; then
    mkdir -p "$clone/target" || return 1
    cp -Rpc "$repo/target/r1-gate" "$clone/target/r1-gate" 2> /dev/null || cp -Rp "$repo/target/r1-gate" "$clone/target/r1-gate" || return 1
  fi
}

run_form() {
  form=$1
  make_clone "$form" || { printf 'ms1 form=%s clone-failed\n' "$form"; return 1; }
  mkdir -p "$root/tmp-$form" || return 1
  started=$(date +%s)
  TMPDIR=$root/tmp-$form sh "$root/$form/scripts/r1-gate/mutation-test.sh" --apply-only --budget 999999 --remove-scratch ${MS1_ONLY:-} > "$root/$form.out" 2>&1
  code=$?
  ended=$(date +%s)
  sed "s/^/ms1 $form | /" "$root/$form.out"
  awk -F '\t' -v form="$form" -v phase="$((ended - started))" -v code="$code" '
    FILENAME == ARGV[1] { kind[$1] = $2; next }
    /^[A-Z-]+ mutation:[^ ]+ \(/ { split($0, q, " "); w = q[2]; sub(/^mutation:/, "", w); word[w] = q[1] }
    /^seconds mutation:/ { split($0, p, " "); n = p[2]; sub(/^mutation:/, "", n); k = (n in kind) ? kind[n] : "unknown"
      if (word[n] != "PASS" && word[n] != "FAIL") { unapplied++; next }
      s = p[3] + 0
      rows++; sum[k] += s; cnt[k]++
      if (k != "other" && first == "") { first = n; firsts = s }
      else if (k == "cargo") { rest[++r] = s } }
    /^PASS mutation:/ { pass++ } /^FAIL / { fail++ } /^OPEN mutation:/ { open++ } /^NOT-RUN mutation:/ { notrun++ }
    END {
      m = 0
      if (r > 0) { for (i = 1; i <= r; i++) for (j = i + 1; j <= r; j++) if (rest[j] < rest[i]) { t = rest[i]; rest[i] = rest[j]; rest[j] = t }
        m = (r % 2) ? rest[(r + 1) / 2] : (rest[r / 2] + rest[r / 2 + 1]) / 2 }
      printf "ms1 summary form=%s exit=%s phase_s=%s timed_rows=%d unapplied_rows=%d pass=%d fail=%d open=%d notrun=%d\n", form, code, phase, rows, unapplied, pass, fail, open, notrun
      printf "ms1 summary form=%s cargo_rows=%d cargo_s=%d kept_build_rows=%d kept_build_s=%d other_rows=%d other_s=%d\n", form, cnt["cargo"], sum["cargo"], cnt["cargo-kept-build"], sum["cargo-kept-build"], cnt["other"], sum["other"]
      printf "ms1 summary form=%s first_cargo_row=%s first_cargo_row_s=%d later_cargo_rows=%d later_cargo_median_s=%s\n", form, first, firsts, r, m
    }' "$root/kinds" "$root/$form.out"
  grep -h '^PASS mutation:mutant-does-not-compile \|^FAIL mutation:mutant-does-not-compile ' "$root/$form.out" | sed "s/^/ms1 control form=$form | /"
  grep -h '^mutation rows: ' "$root/$form.out" | sed "s/^/ms1 count form=$form | /"
  return "$code"
}

run_form check; check_code=$?
run_form build; build_code=$?
printf 'ms1 done check_exit=%s build_exit=%s\n' "$check_code" "$build_code"
[ "$check_code" -eq 0 ] && [ "$build_code" -eq 0 ]
