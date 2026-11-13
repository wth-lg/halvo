#!/bin/sh
# jobs: macos windows
# rule: Read from the commit at their paths as regular files reached through trees alone, with no link, no twin in another letter case and no name outside ASCII at those places, the ruling file is the one line ruling=none and no row of the policy or of the recorded table names a hosted tier, the lines of cargo tree for the normal dependencies of the run's three crates name none of a closed list of 11 packages, the egress lists hold no entry, cargo test of halvo-route prints the ok lines of no_grant_in_release and pinned_files_hold_their_hashes under the target tests/no_grant_in_release.rs in the debug and the release profile, a program of another crate built with --cfg test set from outside stops with E0624 where it names Ruling::test_hosted while its control compiles and answers ruling, and no cargo command leaves the tree changed; 40-r6-policy holds this script by the blob id of its content, and the check holds none of these: code that runs in the job before it and writes what it reads (SG-30), a network package outside the list such as tungstenite, or std::net, a build or a development dependency, a forged ok line in the release profile with the pin test switched off (30-r6-table refuses it in the debug profile alone), a data file that release.rs reads from another path, a tracked cargo configuration (40-r6-policy refuses it), and the pair's text, which the blob id and the real path hold because the self-test compiles nothing; a job that turns warnings into errors fails it.
set -u
name=80-r6-absent
case $0 in
  /*) self=$0 ;;
  *) self=$(pwd)/$0 ;;
esac
tab=$(printf '\t')
entry_path=$PATH
policy_header="category${tab}kind${tab}pick${tab}level${tab}basis${tab}k${tab}n"
recorded_header="task${tab}category${tab}kind${tab}tier${tab}model${tab}machine${tab}seed${tab}split${tab}attempt${tab}pass${tab}steps${tab}wall_s${tab}tokens_in${tab}tokens_out${tab}level${tab}source"
net_packages='reqwest hyper ureq rustls ring aws-lc-rs native-tls openssl-sys tokio socket2 mio'
probe_source='use halvo_route::ruling::Ruling;

fn main() {
    let ruling = Ruling::test_hosted(&["127.0.0.1:1"]);
    println!("probe origins={}", ruling.origins().len());
}'
control_source='use halvo_route::leave::{grant, Consent, PartKind};
use halvo_route::tier::Tier;

fn main() {
    let ruling = halvo_route::release::ruling();
    for tier in [Tier::LowCostHosted, Tier::Frontier] {
        let answer = match grant(&ruling, tier, Tier::Frontier, &[PartKind::Text], "127.0.0.1:1", Consent::Recorded) {
            Ok(_) => "grant",
            Err(cause) => cause.as_str(),
        };
        println!("control tier={} answer={}", tier.as_str(), answer);
    }
    println!("control cfg_test={}", if cfg!(test) { "yes" } else { "no" });
}'
control_want='control tier=low_cost_hosted answer=ruling
control tier=frontier answer=ruling
control cfg_test=yes'

setup_fail() {
  printf 'FAIL %s (setup)\n' "$name"
  exit 1
}

enter_repo() {
  if [ -n "${CI_D_REPO:-}" ]; then
    repo=$CI_D_REPO
  else
    repo=$(git rev-parse --show-toplevel 2>/dev/null) || repo=
  fi
  if [ -z "$repo" ] || ! cd "$repo" 2>/dev/null; then
    setup_fail
  fi
}

tree_state() {
  git -c core.fsmonitor=false -c core.untrackedCache=false \
    -c 'alias.r6-tree-state=!git status --porcelain=v2 --branch --untracked-files=all --ignored=matching && git ls-files -v' r6-tree-state
}

tree_same() {
  tree_now=$(tree_state 2>&1) || tree_now="unreadable $tree_now"
  [ "$tree_now" = "$tree_before" ] && return 0
  R6_TREE_BEFORE=$tree_before R6_TREE_NOW=$tree_now awk -v when="$1" '
    function report(line,    field, count) {
      if (shown++ >= 5) return
      count = split(line, field, " ")
      printf "tree changed=%s %s %s\n", when, field[1], field[count]
    }
    BEGIN {
      n = split(ENVIRON["R6_TREE_BEFORE"], before, "\n")
      for (i = 1; i <= n; i++) was[before[i]] = 1
      m = split(ENVIRON["R6_TREE_NOW"], now, "\n")
      for (i = 1; i <= m; i++) is[now[i]] = 1
      for (i = 1; i <= m; i++) if (!(now[i] in was)) report(now[i])
      for (i = 1; i <= n; i++) if (!(before[i] in is)) report(before[i])
    }
  '
  printf 'FAIL %s (%s)\n' "$name" "$2"
  exit 1
}

read_commit() {
  commit=$(git rev-parse --verify --quiet 'HEAD^{commit}') || commit=
  tree_before=$(tree_state 2>&1) || tree_before=
  if [ -z "$commit" ] || [ -z "$tree_before" ]; then
    printf 'tree commit=unreadable\n'
    setup_fail
  fi
  changed=0
  while IFS= read -r state_line; do
    case ${state_line%% *} in
      '' | '#' | '?' | '!' | H) ;;
      *)
        printf 'tree changed=before %s %s\n' "${state_line%% *}" "${state_line##* }"
        changed=1
        ;;
    esac
  done <<EOF
$tree_before
EOF
  [ "$changed" -eq 0 ] || setup_fail
}

commit_unreadable() {
  printf 'tree commit=unreadable\n'
  setup_fail
}

data_plan() {
  awk -F "$tab" '
    BEGIN {
      subject["crates/halvo-route/data/ruling.txt"] = "ruling"
      subject["crates/halvo-route/data/policy.tsv"] = "picks"
      subject["docs/r6/outcomes/recorded.tsv"] = "rows"
      step["crates"] = step["crates/halvo-route"] = step["crates/halvo-route/data"] = "ruling picks"
      step["docs"] = step["docs/r6"] = step["docs/r6/outcomes"] = "rows"
      step["scripts"] = step["scripts/r6"] = step["scripts/r6/egress"] = "entries"
      watched[""] = "ruling picks rows entries"
      for (place in step) watched[place] = step[place]
      egress = "scripts/r6/egress/"
    }
    function odd(subjects,    n, i, word) {
      n = split(subjects, word, " ")
      for (i = 1; i <= n; i++) print word[i], "odd", "-"
    }
    function quoted(name,    place, rest, part) {
      for (place in watched) {
        if (place == "") rest = name
        else if (index(name, place "/") == 1) rest = substr(name, length(place) + 2)
        else continue
        part = rest
        if (place != "scripts/r6/egress") sub(/\/.*/, "", part)
        if (index(part, "\\") > 0) odd(watched[place])
      }
    }
    {
      split($1, field, " ")
      regular = (field[1] == "100644" || field[1] == "100755") && field[2] == "blob"
      path = $2
      if (path ~ /^"/) {
        path = substr(path, 2, length(path) - 2)
        quoted(path)
        regular = 0
      }
      if (path in subject) {
        if (regular) print subject[path], "blob", field[3]
        else odd(subject[path])
        next
      }
      if (index(path, egress) == 1) {
        if (regular) print "entries", "blob", field[3]
        else odd("entries")
        next
      }
      for (data in subject) if (index(path, data "/") == 1) odd(subject[data])
      folded = tolower(path)
      if (folded in subject) odd(subject[folded])
      if (folded in step) odd(step[folded])
      if (index(folded, egress) == 1) odd("entries")
    }
  ' "$1"
}

count_data() {
  committed=$scratch/commit
  git -c core.quotepath=on ls-tree -r "$commit" >"$scratch/listing.txt" 2>/dev/null || commit_unreadable
  data_plan "$scratch/listing.txt" >"$scratch/plan.txt" || setup_fail
  ruling_odd=0
  picks_odd=0
  rows_odd=0
  entries_odd=0
  ruling_blob=
  picks_blob=
  rows_blob=
  listed=0
  while read -r data_subject data_state data_object; do
    case $data_subject:$data_state in
      entries:blob)
        listed=$((listed + 1))
        git cat-file blob "$data_object" >"$committed/egress/$listed" 2>/dev/null || commit_unreadable
        ;;
      ruling:blob) ruling_blob=$data_object ;;
      picks:blob) picks_blob=$data_object ;;
      rows:blob) rows_blob=$data_object ;;
      ruling:*) ruling_odd=1 ;;
      picks:*) picks_odd=1 ;;
      rows:*) rows_odd=1 ;;
      entries:*) entries_odd=$((entries_odd + 1)) ;;
    esac
  done <"$scratch/plan.txt"
  ruling=other
  if [ -n "$ruling_blob" ]; then
    git cat-file blob "$ruling_blob" >"$committed/ruling.txt" 2>/dev/null || commit_unreadable
    [ "$ruling_odd" = 1 ] || ruling=$(ruling_of "$committed/ruling.txt")
  fi
  picks=1
  if [ -n "$picks_blob" ]; then
    git cat-file blob "$picks_blob" >"$committed/policy.tsv" 2>/dev/null || commit_unreadable
    picks=$(hosted_rows "$committed/policy.tsv" "$policy_header" 3 'none hand_back on_device' 1)
  fi
  [ "$picks_odd" = 0 ] || [ "$picks" != 0 ] || picks=1
  rows=0
  if [ -n "$rows_blob" ]; then
    git cat-file blob "$rows_blob" >"$committed/recorded.tsv" 2>/dev/null || commit_unreadable
    rows=$(hosted_rows "$committed/recorded.tsv" "$recorded_header" 4 on_device 0)
  fi
  [ "$rows_odd" = 0 ] || [ "$rows" != 0 ] || rows=1
  entries=0
  [ "$listed" -eq 0 ] || entries=$(egress_entries "$committed/egress")
  entries=$((entries + entries_odd))
}

holds_nul() {
  od -An -v -tx1 "$1" >"$scratch/bytes.txt" 2>/dev/null || return 0
  while IFS= read -r bytes; do
    case " $bytes " in
      *' 00 '*) return 0 ;;
    esac
  done <"$scratch/bytes.txt"
  return 1
}

ruling_of() {
  printf 'ruling=none\n' >"$scratch/ruling-none"
  if cmp -s "$1" "$scratch/ruling-none"; then
    printf 'none\n'
  else
    printf 'other\n'
  fi
}

hosted_rows() {
  if [ ! -e "$1" ] && [ ! -h "$1" ]; then
    printf '%s\n' "$5"
    return
  fi
  if [ ! -f "$1" ] || [ ! -r "$1" ]; then
    printf '1\n'
    return
  fi
  count=$(awk -F "$tab" -v header="$2" -v column="$3" -v allowed="$4" '
    BEGIN {
      n = split(allowed, word, " ")
      for (i = 1; i <= n; i++) ok[word[i]] = 1
    }
    { sub(/\r$/, "") }
    NR == 1 && $0 == header { next }
    {
      hosted = !($column in ok)
      for (i = 1; i <= NF; i++) {
        cell = $i
        sub(/\r$/, "", cell)
        if (cell == "low_cost_hosted" || cell == "frontier") hosted = 1
      }
      count += hosted
    }
    END { print count + 0 }
  ' "$1") || count=1
  if holds_nul "$1" && [ "$count" -eq 0 ]; then
    count=1
  fi
  printf '%s\n' "$count"
}

egress_entries() {
  if [ ! -e "$1" ] && [ ! -h "$1" ]; then
    printf '0\n'
    return
  fi
  find "$1" ! -type d >"$scratch/egress-files.txt" 2>/dev/null || printf '%s\n' "$1/unreadable" >>"$scratch/egress-files.txt"
  nul=0
  while IFS= read -r path; do
    if holds_nul "$path"; then
      nul=1
    fi
  done <"$scratch/egress-files.txt"
  count=$(awk '
    {
      path = $0
      while ((got = (getline line < path)) > 0) {
        sub(/\r$/, "", line)
        if (line != "[fixed]" && line != "[agent]") count++
      }
      if (got < 0) count++
      close(path)
    }
    END { print count + 0 }
  ' "$scratch/egress-files.txt") || count=1
  if [ "$nul" -eq 1 ] && [ "$count" -eq 0 ]; then
    count=1
  fi
  printf '%s\n' "$count"
}

net_crates() {
  awk -v packages="$net_packages" '
    BEGIN {
      n = split(packages, word, " ")
      for (i = 1; i <= n; i++) net[word[i]] = 1
    }
    {
      sub(/\r$/, "")
      for (i = 2; i <= NF; i++) {
        if ($i !~ /^v[0-9]/) continue
        package = tolower($(i - 1))
        gsub(/_/, "-", package)
        if (package in net) count++
        break
      }
    }
    END { print count + 0 }
  ' "$1"
}

grant_lines_hold() {
  awk -v files="$#" '
    function target_of(line,    paren, bin) {
      sub(/^ *Running /, "", line)
      gsub(/\\/, "/", line)
      paren = index(line, " (")
      if (paren == 0 || line !~ /\)$/) return ""
      bin = substr(line, paren + 2, length(line) - paren - 2)
      sub(/.*\//, "", bin)
      sub(/\.exe$/, "", bin)
      sub(/-[0-9a-f]+$/, "", bin)
      return bin " " substr(line, 1, paren - 1)
    }
    function finish() {
      if (!(lines == 1 && grant_ok == 1 && pins_ok == 1)) bad = 1
    }
    FNR == 1 {
      if (seen++) finish()
      target = ""
      lines = grant_ok = pins_ok = 0
    }
    { sub(/\r$/, "") }
    /^ *Doc-tests / { target = ""; next }
    /^ *Running / {
      target = target_of($0)
      if (target == "no_grant_in_release tests/no_grant_in_release.rs") lines++
      next
    }
    target != "no_grant_in_release tests/no_grant_in_release.rs" { next }
    $0 == "test no_grant_in_release ... ok" { grant_ok++ }
    $0 == "test pinned_files_hold_their_hashes ... ok" { pins_ok++ }
    END {
      if (seen) finish()
      exit (bad || seen != files)
    }
  ' "$@"
}

probe_stops() {
  awk -v line="$2" '
    { sub(/\r$/, ""); gsub(/\\/, "/") }
    /^error: (could not compile |aborting due to )/ { next }
    /^error(\[E[0-9]+\])?: / { elsewhere++; next }
    /^[^ ].*:[0-9]+:[0-9]+: error(\[E[0-9]+\])?: / {
      if ($0 ~ ("^src/bin/probe\\.rs:" line ":[0-9]+: error\\[E0624\\]: ")) at_line++
      else elsewhere++
    }
    END { exit !(at_line >= 1 && elsewhere == 0) }
  ' "$1"
}

write_pair() {
  route=$repo/crates/halvo-route
  if command -v cygpath >/dev/null 2>&1; then
    route=$(cygpath -m "$route") || return 1
  fi
  printf '[package]\nname = "r6-absent-pair"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n[workspace]\n\n[dependencies]\nhalvo-route = { path = '"'"'%s'"'"' }\n' \
    "$route" >"$pair/Cargo.toml" || return 1
  printf '%s\n' "$probe_source" >"$pair/src/bin/probe.rs" || return 1
  printf '%s\n' "$control_source" >"$pair/src/bin/control.rs" || return 1
}

build_pair() {
  if [ "${CARGO_ENCODED_RUSTFLAGS+set}" = set ]; then
    separator=$(printf '\037')
    CARGO_ENCODED_RUSTFLAGS="${CARGO_ENCODED_RUSTFLAGS:+$CARGO_ENCODED_RUSTFLAGS$separator}--cfg${separator}test"
    export CARGO_ENCODED_RUSTFLAGS
    set --
  elif [ "${RUSTFLAGS+set}" = set ]; then
    RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }--cfg test"
    export RUSTFLAGS
    set --
  else
    set -- --config 'build.rustflags=["--cfg", "test"]'
  fi
  cargo generate-lockfile --offline --manifest-path "$pair/Cargo.toml" >"$artifacts/pair-lock.txt" 2>&1 </dev/null
  cargo run --locked --offline --manifest-path "$pair/Cargo.toml" --target-dir "$scratch/pair-target" --bin control "$@" \
    >"$artifacts/control.out" 2>"$artifacts/control.err" </dev/null
  printf '%s\n' "$?" >"$scratch/control.code"
  cargo build --locked --offline --manifest-path "$pair/Cargo.toml" --target-dir "$scratch/pair-target" --message-format short \
    --bin probe "$@" >"$artifacts/probe.txt" 2>&1 </dev/null
  printf '%s\n' "$?" >"$scratch/probe.code"
}

test_value() {
  pair=$scratch/pair
  misses=0
  if write_pair; then
    (build_pair)
    probe_code=
    control_code=
    { read -r probe_code <"$scratch/probe.code"; } 2>/dev/null
    { read -r control_code <"$scratch/control.code"; } 2>/dev/null
  else
    probe_code=
    control_code=
  fi
  tree_same after-pair test_value
  judge_pair
}

judge_pair() {
  misses=0
  probe_line=
  number=0
  while IFS= read -r source_line; do
    number=$((number + 1))
    case $source_line in
      *test_hosted*)
        probe_line=$number
        break
        ;;
    esac
  done <<EOF
$probe_source
EOF
  if [ -z "$probe_line" ] || [ "$probe_code" = 0 ] || [ -z "$probe_code" ] || ! probe_stops "$artifacts/probe.txt" "$probe_line"; then
    misses=$((misses + 1))
  fi
  printf '%s\n' "$control_want" >"$scratch/control.want"
  if [ "$control_code" != 0 ] || ! cmp -s "$artifacts/control.out" "$scratch/control.want"; then
    misses=$((misses + 1))
  fi
  value_count=$misses
}

crates_of() {
  crates=$(net_crates "$artifacts/tree.txt")
  [ "$tree_code" -eq 0 ] || [ "$crates" -gt 0 ] || crates=1
}

grants() {
  failed=0
  started=$(date +%s)
  cargo test --locked -p halvo-route >"$artifacts/test-debug.txt" 2>&1 </dev/null || failed=1
  middle=$(date +%s)
  tree_same after-debug grants
  cargo test --locked -p halvo-route --release >"$artifacts/test-release.txt" 2>&1 </dev/null || failed=1
  ended=$(date +%s)
  printf 'debug %s\nrelease %s\n' "$((middle - started))" "$((ended - middle))" >>"$artifacts/seconds.txt"
  tree_same after-release grants
  grant_count=1
  if [ "$failed" -eq 0 ] && grant_lines_hold "$artifacts/test-debug.txt" "$artifacts/test-release.txt"; then
    grant_count=0
  fi
}

measure() {
  enter_repo
  artifacts=${CI_D_ARTIFACTS:-target/ci-artifacts/$name}
  if ! scratch=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX" 2>/dev/null) ||
    ! mkdir -p "$artifacts" "$scratch/commit/egress" "$scratch/pair/src/bin" 2>/dev/null; then
    setup_fail
  fi
  : >"$artifacts/seconds.txt" || setup_fail
  CARGO_TERM_QUIET=false
  CARGO_TERM_VERBOSE=false
  CARGO_TERM_COLOR=never
  export CARGO_TERM_QUIET CARGO_TERM_VERBOSE CARGO_TERM_COLOR

  read_commit
  count_data

  cargo tree --locked -e normal --target all --all-features -p halvo-route -p halvo-route-gate -p halvo-route-wire \
    >"$artifacts/tree.txt" 2>"$artifacts/tree.err" </dev/null
  tree_code=$?
  tree_same after-tree crates
  crates_of

  grants
  test_value

  printf 'absent ruling=%s hosted_picks=%s hosted_rows=%s net_crates=%s entries=%s grants=%s test_value=%s\n' \
    "$ruling" "$picks" "$rows" "$crates" "$entries" "$grant_count" "$value_count"
  cause=
  [ "$ruling" = none ] || cause=ruling
  [ "$picks" = 0 ] || cause=${cause:-picks}
  [ "$rows" = 0 ] || cause=${cause:-rows}
  [ "$crates" = 0 ] || cause=${cause:-crates}
  [ "$entries" = 0 ] || cause=${cause:-entries}
  [ "$grant_count" = 0 ] || cause=${cause:-grants}
  [ "$value_count" = 0 ] || cause=${cause:-test_value}
  if [ -n "$cause" ]; then
    printf 'FAIL %s (%s)\n' "$name" "$cause"
    exit 1
  fi
  printf 'PASS %s\n' "$name"
}

verdict() {
  printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
  if [ "$3" != "$2" ]; then
    wrong="$wrong $1"
  elif [ "$4" = failing ]; then
    rejected=$((rejected + 1))
  fi
}

reset_stubs() {
  STUB_COMMIT=$work/commit
  STUB_NO_COMMIT=
  STUB_CHANGE_AT=
  STUB_CHANGE_LINE=
  STUB_CHANGE_FILE=status
  STUB_TREE_OUT=$work/stub/tree.out
  STUB_TREE_EXIT=0
  STUB_DEBUG_OUT=$work/stub/debug.out
  STUB_DEBUG_EXIT=0
  STUB_RELEASE_OUT=$work/stub/release.out
  STUB_RELEASE_EXIT=0
  STUB_PROBE_OUT=$work/stub/probe.out
  STUB_PROBE_EXIT=101
  STUB_CONTROL_OUT=$work/stub/control.out
  STUB_CONTROL_EXIT=0
  STUB_JOB_FLAG=
  STUB_GITLINK=
  CASE_RUSTFLAGS=
  CASE_ENCODED=
  CASE_QUIET=
  : >"$work/tree/status" || fixture_fail
  : >"$work/tree/flags" || fixture_fail
}

check() {
  case_name=$1
  want=$2
  shift 2
  out=$(
    CI_D_REPO=$work/repo
    CI_D_ARTIFACTS=$work/artifacts/$case_name
    STUB_TREE=$work/tree
    export CI_D_REPO CI_D_ARTIFACTS STUB_TREE STUB_COMMIT STUB_NO_COMMIT STUB_CHANGE_AT STUB_CHANGE_LINE STUB_CHANGE_FILE
    export STUB_TREE_OUT STUB_TREE_EXIT STUB_DEBUG_OUT STUB_DEBUG_EXIT STUB_RELEASE_OUT STUB_RELEASE_EXIT
    export STUB_PROBE_OUT STUB_PROBE_EXIT STUB_CONTROL_OUT STUB_CONTROL_EXIT STUB_JOB_FLAG STUB_GITLINK
    unset RUSTFLAGS CARGO_ENCODED_RUSTFLAGS
    if [ -n "$CASE_RUSTFLAGS" ]; then
      RUSTFLAGS=$CASE_RUSTFLAGS
      export RUSTFLAGS
    fi
    if [ -n "$CASE_ENCODED" ]; then
      CARGO_ENCODED_RUSTFLAGS=$CASE_ENCODED
      export CARGO_ENCODED_RUSTFLAGS
    fi
    if [ -n "$CASE_QUIET" ]; then
      CARGO_TERM_QUIET=$CASE_QUIET
      export CARGO_TERM_QUIET
    fi
    sh "$self" 2>&1
  )
  code=$?
  last=${out##*"$newline"}
  case $code:$last in
    "0:PASS $name") got=PASS ;;
    "1:FAIL $name ("*")") got=FAIL ;;
    *) got=exit-$code ;;
  esac
  for text in "$@"; do
    case $out in
      *"$text"*) ;;
      *) got=$got-without-the-text ;;
    esac
  done
  if [ "$want" = FAIL ]; then
    verdict "$case_name" "$want" "$got" failing
  else
    verdict "$case_name" "$want" "$got" passing
  fi
  reset_stubs
}

unit() {
  unit_name=$1
  unit_want=$2
  unit_kind=$3
  shift 3
  unit_got=$("$@") || unit_got=exit-status
  verdict "$unit_name" "$unit_want" "$unit_got" "$unit_kind"
}

direct() {
  if [ "$1:$2" = "1:FAIL $name ($3)" ]; then
    verdict "$4" FAIL FAIL failing
  else
    verdict "$4" FAIL "exit-$1" failing
  fi
}

write_stubs() {
  cat >"$work/bin/git" <<'EOF'
#!/bin/sh
head=1111111111111111111111111111111111111111
put() {
  while IFS= read -r line || [ -n "$line" ]; do
    printf '%s\n' "$line"
  done <"$1"
}
case $1:${2:-} in
  cat-file:blob)
    put "$STUB_COMMIT/${3#obj:}"
    exit
    ;;
  -c:core.quotepath=on)
    [ "$*" = "-c core.quotepath=on ls-tree -r $head" ] || exit 97
    [ -z "$STUB_GITLINK" ] || printf '160000 commit 2222222222222222222222222222222222222222\tscripts/r6/egress/lists\n'
    if [ ! -f "$STUB_COMMIT.listing" ]; then
      cd "$STUB_COMMIT" || exit 2
      find crates docs scripts -type f >"$STUB_COMMIT.files" || exit 2
      while IFS= read -r path; do
        printf '100644 blob obj:%s\t%s\n' "$path" "$path"
      done <"$STUB_COMMIT.files" >"$STUB_COMMIT.listing"
    fi
    put "$STUB_COMMIT.listing"
    exit 0
    ;;
esac
case $* in
  'rev-parse --verify --quiet HEAD^{commit}')
    [ -z "$STUB_NO_COMMIT" ] || exit 1
    printf '%s\n' "$head"
    ;;
  *' r6-tree-state')
    case $* in
      "-c core.fsmonitor=false -c core.untrackedCache=false -c alias.r6-tree-state=!git status --porcelain=v2 --branch --untracked-files=all --ignored=matching && git ls-files -v r6-tree-state") ;;
      *) exit 97 ;;
    esac
    printf '# branch.oid %s\n' "$head"
    put "$STUB_TREE/status"
    printf 'H crates/halvo-route/data/ruling.txt\n'
    put "$STUB_TREE/flags"
    ;;
  *)
    printf 'stub git: got the arguments: %s\n' "$*" >&2
    exit 97
    ;;
esac
EOF
  cat >"$work/bin/cargo" <<'EOF'
#!/bin/sh
kind=other
put() {
  while IFS= read -r line || [ -n "$line" ]; do
    printf '%s\n' "$line"
  done <"$1"
}
case $* in
  'tree --locked -e normal --target all --all-features -p halvo-route -p halvo-route-gate -p halvo-route-wire') kind=tree ;;
  'test --locked -p halvo-route') kind=debug ;;
  'test --locked -p halvo-route --release') kind=release ;;
  'generate-lockfile --offline --manifest-path '*'/pair/Cargo.toml') kind=lock ;;
  'run --locked --offline --manifest-path '*'/pair/Cargo.toml --target-dir '*' --bin control'*) kind=control ;;
  'build --locked --offline --manifest-path '*'/pair/Cargo.toml --target-dir '*' --message-format short --bin probe'*) kind=probe ;;
esac
if [ -n "$STUB_CHANGE_AT" ] && [ "$kind" = "$STUB_CHANGE_AT" ]; then
  printf '%s\n' "$STUB_CHANGE_LINE" >>"$STUB_TREE/${STUB_CHANGE_FILE:-status}"
fi
case $kind in
  tree)
    put "$STUB_TREE_OUT"
    exit "$STUB_TREE_EXIT"
    ;;
  debug | release)
    [ "${CARGO_TERM_QUIET:-false}" = true ] && exit 0
    if [ "$kind" = debug ]; then
      put "$STUB_DEBUG_OUT"
      exit "$STUB_DEBUG_EXIT"
    fi
    put "$STUB_RELEASE_OUT"
    exit "$STUB_RELEASE_EXIT"
    ;;
  lock) exit 0 ;;
  probe | control)
    if [ "${CARGO_ENCODED_RUSTFLAGS+set}" = set ]; then
      flags=$(printf '%s' "$CARGO_ENCODED_RUSTFLAGS" | tr '\037' ' ')
    elif [ "${RUSTFLAGS+set}" = set ]; then
      flags=$RUSTFLAGS
    else
      flags=
      case $* in
        *' --config build.rustflags=["--cfg", "test"]') flags='--cfg test' ;;
      esac
    fi
    reached=yes
    case " $flags " in
      *' --cfg test '*) ;;
      *) reached=no ;;
    esac
    if [ -n "$STUB_JOB_FLAG" ]; then
      case " $flags " in
        *" $STUB_JOB_FLAG "*) ;;
        *) reached=no ;;
      esac
    fi
    if [ "$kind" = probe ]; then
      if [ "$reached" = no ]; then
        printf 'src/bin/probe.rs:4:26: error[E0599]: no function or associated item named test_hosted found for enum `Ruling`\n'
        exit 101
      fi
      put "$STUB_PROBE_OUT"
      exit "$STUB_PROBE_EXIT"
    fi
    if [ "$reached" = no ]; then
      printf 'control tier=low_cost_hosted answer=ruling\ncontrol tier=frontier answer=ruling\ncontrol cfg_test=no\n'
      exit 0
    fi
    put "$STUB_CONTROL_OUT"
    exit "$STUB_CONTROL_EXIT"
    ;;
  *)
    printf 'stub cargo: got the arguments: %s\n' "$*"
    exit 97
    ;;
esac
EOF
  chmod +x "$work/bin/git" "$work/bin/cargo" || fixture_fail
}

write_outputs() {
  stub=$work/stub
  cat >"$stub/debug.out" <<'EOF'
   Compiling halvo-route v0.0.0 (crates/halvo-route)
    Finished test profile [unoptimized + debuginfo] target(s) in 1.00s
     Running unittests src/lib.rs (target/debug/deps/halvo_route-0123456789abcdef)

running 1 test
test ruling::tests::ruling_test_value ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/no_grant_in_release.rs (target/debug/deps/no_grant_in_release-0123456789abcdef)

running 3 tests
test lib_rs_is_the_forbid_line_and_the_module_lines ... ok
test no_grant_in_release ... ok
test pinned_files_hold_their_hashes ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

     Running tests/pick_rules.rs (target/debug/deps/pick_rules-0123456789abcdef)

running 1 test
test pick_rule ... ok

test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s

   Doc-tests halvo_route

running 0 tests
EOF
  sed 's|target/debug/|target/release/|; s|test profile \[unoptimized + debuginfo\]|release profile [optimized]|' "$stub/debug.out" >"$stub/release.out" || fixture_fail
  printf 'halvo-route v0.0.0 (crates/halvo-route)\n\nhalvo-route-wire v0.0.0 (crates/halvo-route-wire)\n\342\224\234\342\224\200\342\224\200 serde v1.0.229\n\342\224\224\342\224\200\342\224\200 serde_json v1.0.154\n' >"$stub/tree.out" || fixture_fail
  printf '   Compiling r6-absent-pair v0.0.0 (pair)\nsrc/bin/probe.rs:4:26: error[E0624]: associated function test_hosted is private: private associated function\nerror: could not compile r6-absent-pair (bin "probe") due to 1 previous error\n' >"$stub/probe.out" || fixture_fail
  printf 'control tier=low_cost_hosted answer=ruling\ncontrol tier=frontier answer=ruling\ncontrol cfg_test=yes\n' >"$stub/control.out" || fixture_fail
  printf 'control tier=low_cost_hosted answer=ruling\ncontrol tier=frontier answer=grant\ncontrol cfg_test=yes\n' >"$stub/control-frontier-grant.out" || fixture_fail
  printf 'halvo-route-wire v0.0.0 (crates/halvo-route-wire)\n\342\224\224\342\224\200\342\224\200 reqwest v0.12.0\n' >"$stub/tree-reqwest.out" || fixture_fail
  awk '$0 != "test no_grant_in_release ... ok"' "$stub/debug.out" >"$stub/debug-no-grant.out" || fixture_fail
  awk '$0 != "test no_grant_in_release ... ok"' "$stub/release.out" >"$stub/release-no-grant.out" || fixture_fail
  printf '' >"$stub/empty.out" || fixture_fail
  printf 'src/bin/probe.rs:4:26: error[E0624]: associated function test_hosted is private\n' >"$stub/probe-text.out" || fixture_fail
  printf 'control tier=low_cost_hosted answer=grant\ncontrol tier=frontier answer=grant\ncontrol cfg_test=yes\n' >"$stub/control-grant.out" || fixture_fail
}

make_commit() {
  mkdir -p "$1/crates/halvo-route/data" "$1/scripts/r6" "$1/docs/r6" || fixture_fail
  printf 'ruling=none\n' >"$1/crates/halvo-route/data/ruling.txt" || fixture_fail
  cp crates/halvo-route/data/policy.tsv "$1/crates/halvo-route/data/policy.tsv" || fixture_fail
}

fixture_fail() {
  printf 'FAIL self-test:%s (cannot build a fixture)\n' "$name"
  exit 1
}

data_tree() {
  awk -F "$tab" '
    {
      split($1, field, " ")
      n = split($2, part, "/")
      parent = "."
      for (i = 1; i < n; i++) {
        child = (parent == "." ? part[i] : parent "/" part[i])
        if (!(child in seen)) {
          seen[child] = 1
          print i, child, parent, part[i] > dirs
        }
        parent = child
      }
      print parent "\t" field[1] " " (field[1] == "160000" ? "commit" : "blob") " " field[2] "\t" part[n]
    }
  ' dirs="$work/d/dirs-$label" "$1" >"$work/d/children-$label" || return 1
  sort -rn "$work/d/dirs-$label" >"$work/d/order-$label" || return 1
  printf '0 . - -\n' >>"$work/d/order-$label" || return 1
  while read -r _ dir parent base; do
    : >"$work/d/records-$label" || return 1
    while IFS= read -r child; do
      [ "${child%%"$tab"*}" = "$dir" ] || continue
      printf '%s\000' "${child#*"$tab"}" >>"$work/d/records-$label" || return 1
    done <"$work/d/children-$label"
    built=$(git -C "$data" mktree -z <"$work/d/records-$label") || return 1
    [ "$dir" = . ] || printf '%s\t040000 tree %s\t%s\n' "$parent" "$built" "$base" >>"$work/d/children-$label" || return 1
  done <"$work/d/order-$label"
  printf '%s\n' "$built"
}

data_case() {
  label=$1
  want=$2
  printf '%b\n' "$3" >"$work/d/text-$label" || fixture_fail
  : >"$work/d/lines-$label" || fixture_fail
  while IFS= read -r entry; do
    printf '%s\000' "$entry" >>"$work/d/lines-$label" || fixture_fail
  done <"$work/d/text-$label"
  if GIT_INDEX_FILE=$work/d/index-$label git -C "$data" update-index -z --add --index-info <"$work/d/lines-$label" 2>"$work/d/refused-$label" &&
    [ ! -s "$work/d/refused-$label" ]; then
    tree=$(GIT_INDEX_FILE=$work/d/index-$label git -C "$data" write-tree) || fixture_fail
  else
    IFS= read -r refused <"$work/d/refused-$label" || refused=
    printf 'case %s/data-%s git refused the index: %s\n' "$name" "$label" "$refused"
    tree=$(data_tree "$work/d/text-$label") || fixture_fail
  fi
  out=$(
    cd "$data" || exit 2
    commit=$tree
    scratch=$work/d/scan-$label
    mkdir -p "$scratch/commit/egress" || exit 2
    if [ -n "${4:-}" ]; then
      R6_FAIL_ON=$4
      PATH=$work/failgit:$entry_path
      export R6_FAIL_ON PATH
    fi
    count_data
    case $label in
      (quoted-*)
        while IFS= read -r listed_line; do
          case $listed_line in
            (*"$tab\""*) printf 'listing %s\n' "$listed_line" ;;
          esac
        done <"$scratch/listing.txt"
        ;;
    esac
    cause=
    [ "$ruling" = none ] || cause=ruling
    [ "$picks" = 0 ] || cause=${cause:-picks}
    [ "$rows" = 0 ] || cause=${cause:-rows}
    [ "$entries" = 0 ] || cause=${cause:-entries}
    printf 'data ruling=%s hosted_picks=%s hosted_rows=%s entries=%s\n' "$ruling" "$picks" "$rows" "$entries"
    printf '%s\n' "${cause:-PASS}"
  )
  got=${out##*"$newline"}
  case $got in
    "FAIL $name (setup)") got=setup ;;
  esac
  case $label in
    quoted-*)
      while IFS= read -r shown; do
        case $shown in
          listing*) printf 'case %s/data-%s %s\n' "$name" "$label" "$shown" ;;
        esac
      done <<EOF
$out
EOF
      ;;
  esac
  if [ "$want" = PASS ]; then
    verdict "data-$label" "$want" "$got" passing
  else
    verdict "data-$label" "$want" "$got" failing
  fi
}

data_units() {
  data=$work/d/repo
  mkdir -p "$data" "$work/failgit" || fixture_fail
  git -C "$data" init -q || fixture_fail
  R6_REAL_GIT=$(command -v git) || fixture_fail
  export R6_REAL_GIT
  cat >"$work/failgit/git" <<'EOF' || fixture_fail
#!/bin/sh
case " $* " in
  *" $R6_FAIL_ON"*) exit 128 ;;
esac
exec "$R6_REAL_GIT" "$@"
EOF
  chmod +x "$work/failgit/git" || fixture_fail
  d=$work/d
  printf 'ruling=none\n' >"$d/ruling" || fixture_fail
  cp crates/halvo-route/data/policy.tsv "$d/policy" || fixture_fail
  printf '%s\n' "$recorded_header" >"$d/header" || fixture_fail
  row="bill${tab}utility_bill_payment${tab}compile${tab}TIER${tab}m1${tab}mac${tab}1${tab}dev${tab}1${tab}yes${tab}3${tab}10${tab}5${tab}5${tab}low${tab}s1"
  printf '%s\n%s\n' "$recorded_header" "$row" | sed 's/TIER/frontier/' >"$d/frontier" || fixture_fail
  printf '%s\n%s\n' "$recorded_header" "$row" | sed 's/TIER/on_device/' >"$d/device" || fixture_fail
  printf '[fixed]\n127.0.0.1:1\n[agent]\n' >"$d/entry" || fixture_fail
  printf '[fixed]\n[agent]\n' >"$d/headings" || fixture_fail
  printf '%s' "$recorded_header" >"$d/header-link" || fixture_fail
  printf '[fixed]' >"$d/fixed-link" || fixture_fail
  printf '../elsewhere' >"$d/elsewhere-link" || fixture_fail
  printf '../other' >"$d/other-link" || fixture_fail
  printf 'ruling=none' >"$d/ruling-link" || fixture_fail
  printf 'policy-target.tsv' >"$d/policy-link" || fixture_fail
  git -C "$data" hash-object -w "$d/ruling" "$d/policy" "$d/header" "$d/frontier" "$d/device" "$d/entry" "$d/headings" \
    "$d/header-link" "$d/fixed-link" "$d/elsewhere-link" "$d/other-link" "$d/ruling-link" "$d/policy-link" >"$d/objects" || fixture_fail
  {
    read -r o_ruling && read -r o_policy && read -r o_header && read -r o_frontier && read -r o_device && read -r o_entry &&
      read -r o_headings && read -r o_header_link && read -r o_fixed_link && read -r o_elsewhere_link && read -r o_other_link &&
      read -r o_ruling_link && read -r o_policy_link
  } <"$d/objects" || fixture_fail
  t='\t'
  ruling_line="100644 $o_ruling${t}crates/halvo-route/data/ruling.txt"
  policy_line="100644 $o_policy${t}crates/halvo-route/data/policy.tsv"
  base="$ruling_line\n$policy_line"
  outcomes=docs/r6/outcomes
  egress_dir=scripts/r6/egress
  data_case base PASS "$base"
  data_case c1-recorded-row-at-its-path rows "$base\n100644 $o_frontier${t}$outcomes/recorded.tsv"
  data_case c2-egress-entry-at-its-path entries "$base\n100644 $o_entry${t}$egress_dir/gate.txt"
  data_case d1-recorded-as-a-link rows "$base\n120000 $o_header_link${t}$outcomes/recorded.tsv"
  data_case c-outcomes-as-a-link rows "$base\n120000 $o_elsewhere_link${t}$outcomes\n100644 $o_frontier${t}docs/r6/elsewhere/recorded.tsv"
  data_case d4-recorded-in-another-letter-case rows "$base\n100644 $o_frontier${t}$outcomes/Recorded.tsv"
  data_case h43-header-in-another-letter-case rows "$base\n100644 $o_header${t}$outcomes/Recorded.tsv"
  data_case c2-egress-as-a-link entries "$base\n120000 $o_fixed_link${t}$egress_dir\n100644 $o_entry${t}scripts/r6/[fixed]/gate.txt"
  data_case h13-egress-as-a-link-alone entries "$base\n120000 $o_fixed_link${t}$egress_dir"
  data_case c3-scripts-r6-as-a-link entries "$base\n120000 $o_other_link${t}scripts/r6\n100644 $o_entry${t}scripts/other/egress/gate.txt"
  data_case d5-egress-in-another-letter-case entries "$base\n100644 $o_entry${t}scripts/r6/Egress/gate.txt"
  data_case h41-headings-in-another-letter-case entries "$base\n100644 $o_headings${t}scripts/r6/Egress/extra.txt"
  data_case quoted-a-egress-name-with-a-double-quote entries "$base\n100644 $o_entry${t}$egress_dir/list\"2.txt"
  data_case quoted-b-egress-name-with-a-backslash entries "$base\n100644 $o_entry${t}$egress_dir/back\\\\slash.txt"
  data_case quoted-c-egress-in-another-letter-case entries "$base\n100644 $o_entry${t}scripts/r6/Egress/a\"b.txt"
  data_case quoted-d-only-list-starts-with-a-double-quote entries "$base\n100644 $o_entry${t}$egress_dir/\"gate.txt"
  data_case quoted-e-headings-alone-under-a-quoted-name entries "$base\n100644 $o_headings${t}$egress_dir/list\"2.txt"
  data_case name-outside-ascii-in-outcomes rows "$base\n100644 $o_header${t}$outcomes/record\0303\0251d.tsv"
  data_case name-outside-ascii-beside-docs ruling "$base\n100644 $o_header${t}d\0303\0266cs/r6/outcomes/recorded.tsv"
  data_case ruling-as-a-link ruling "120000 $o_ruling_link${t}crates/halvo-route/data/ruling.txt\n$policy_line"
  data_case ruling-twin-in-another-letter-case ruling "$base\n100644 $o_ruling${t}crates/halvo-route/data/Ruling.txt"
  data_case policy-twin-in-another-letter-case picks "$base\n100644 $o_policy${t}crates/halvo-route/data/Policy.tsv"
  data_case policy-as-a-link picks "$ruling_line\n120000 $o_policy_link${t}crates/halvo-route/data/policy.tsv\n100644 $o_policy${t}crates/halvo-route/data/policy-target.tsv"
  data_case no-policy picks "$ruling_line"
  data_case outcomes-as-a-submodule rows "$base\n160000 2222222222222222222222222222222222222222${t}$outcomes"
  data_case egress-as-a-file entries "$base\n100644 $o_headings${t}$egress_dir"
  data_case recorded-path-as-a-tree rows "$base\n100644 $o_device${t}$outcomes/recorded.tsv/rows.tsv"
  data_case recorded-executable-on-the-device PASS "$base\n100755 $o_device${t}$outcomes/recorded.tsv"
  data_case listing-fails setup "$base" ls-tree
  data_case policy-read-fails setup "$base" "$o_policy"
  data_case ruling-read-fails setup "$base" "$o_ruling"
  data_case egress-read-fails setup "$base\n100644 $o_entry${t}$egress_dir/gate.txt" "$o_entry"
  data_case recorded-read-fails setup "$base\n100644 $o_device${t}$outcomes/recorded.tsv" "$o_device"
}

ruling_units() {
  u=$work/u
  for fixture in other-line:'ruling=hosted\n' without-its-lf:'ruling=none' line-twice:'ruling=none\nruling=none\n' \
    carriage-return:'ruling=none\r\n' leading-space:' ruling=none\n'; do
    printf '%b' "${fixture#*:}" >"$u/ruling-${fixture%%:*}" || fixture_fail
    unit "ruling-${fixture%%:*}" other failing ruling_of "$u/ruling-${fixture%%:*}"
  done
  unit ruling-missing other failing ruling_of "$u/no-such-file"
  mkdir "$u/ruling-directory" || fixture_fail
  unit ruling-directory other failing ruling_of "$u/ruling-directory"
  printf 'ruling=none\n' >"$u/ruling-none" || fixture_fail
  unit ruling-none none passing ruling_of "$u/ruling-none"
}

row_units() {
  u=$work/u
  policy=crates/halvo-route/data/policy.tsv
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $3 = "frontier"; $4 = "high" } { print }' "$policy" >"$u/policy-frontier" || fixture_fail
  awk -F "$tab" -v OFS="$tab" 'NR == 3 { $3 = "low_cost_hosted"; $4 = "low" } { print }' "$policy" >"$u/policy-low-cost" || fixture_fail
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $3 = "Frontier" } { print }' "$policy" >"$u/policy-unknown-word" || fixture_fail
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $4 = "frontier" } { print }' "$policy" >"$u/policy-hosted-level" || fixture_fail
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $5 = "low_cost_hosted" } { print }' "$policy" >"$u/policy-low-cost-in-the-basis-cell" || fixture_fail
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $7 = $7 "\r"; $3 = "frontier\r" } { print }' "$policy" >"$u/policy-hosted-cr" || fixture_fail
  awk '{ printf "%s\r\n", $0 }' "$policy" >"$u/policy-crlf" || fixture_fail
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $3 = "frontier" } { printf "%s\r\n", $0 }' "$policy" >"$u/policy-crlf-frontier" || fixture_fail
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $3 = "on_device"; $4 = "low" } { print }' "$policy" >"$u/policy-on-device" || fixture_fail
  awk -F "$tab" -v OFS="$tab" 'NR == 2 { $3 = "low_cost_hosted"; $4 = "low"; print }' "$policy" >"$u/policy-without-its-header" || fixture_fail
  cp "$policy" "$u/policy-nul-byte" || fixture_fail
  printf 'other\tcompile\tnone\000\t-\tnot_seed\t0\t0\n' >>"$u/policy-nul-byte" || fixture_fail
  for fixture in frontier low-cost unknown-word hosted-level low-cost-in-the-basis-cell hosted-cr crlf-frontier without-its-header nul-byte; do
    unit "policy-$fixture" 1 failing hosted_rows "$u/policy-$fixture" "$policy_header" 3 'none hand_back on_device' 1
  done
  unit policy-missing 1 failing hosted_rows "$u/no-such-file" "$policy_header" 3 'none hand_back on_device' 1
  mkdir "$u/policy-directory" || fixture_fail
  unit policy-directory 1 failing hosted_rows "$u/policy-directory" "$policy_header" 3 'none hand_back on_device' 0
  unit policy-on-device 0 passing hosted_rows "$u/policy-on-device" "$policy_header" 3 'none hand_back on_device' 1
  unit policy-crlf 0 passing hosted_rows "$u/policy-crlf" "$policy_header" 3 'none hand_back on_device' 1
  unit policy-committed 0 passing hosted_rows "$policy" "$policy_header" 3 'none hand_back on_device' 1

  row="bill${tab}utility_bill_payment${tab}compile${tab}on_device${tab}m1${tab}mac${tab}1${tab}dev${tab}1${tab}yes${tab}3${tab}10${tab}5${tab}5${tab}low${tab}s1"
  printf '%s\n%s\n' "$recorded_header" "$row" >"$u/recorded-on-device" || fixture_fail
  printf '%s\n%s\n' "$recorded_header" "$row" | sed "2s/${tab}on_device${tab}/${tab}frontier${tab}/" >"$u/recorded-frontier" || fixture_fail
  printf '%s\n%s\n%s\n' "$recorded_header" "$row" "$row" | sed "3s/${tab}on_device${tab}/${tab}low_cost_hosted${tab}/" >"$u/recorded-low-cost" || fixture_fail
  unit recorded-frontier 1 failing hosted_rows "$u/recorded-frontier" "$recorded_header" 4 on_device 0
  unit recorded-low-cost 1 failing hosted_rows "$u/recorded-low-cost" "$recorded_header" 4 on_device 0
  mkdir "$u/recorded-directory" || fixture_fail
  unit recorded-directory 1 failing hosted_rows "$u/recorded-directory" "$recorded_header" 4 on_device 0
  unit recorded-on-device 0 passing hosted_rows "$u/recorded-on-device" "$recorded_header" 4 on_device 0
  unit recorded-missing 0 passing hosted_rows "$u/no-such-file" "$recorded_header" 4 on_device 0
}

entry_units() {
  u=$work/u
  for fixture in entry:'[fixed]\n127.0.0.1:1\n[agent]\n' blank-line:'[fixed]\n\n[agent]\n' \
    last-line-without-lf:'[fixed]\n[agent]\nexample.invalid:443' carriage-returns:'[fixed]\r\n127.0.0.1:1\r\n[agent]\r\n' headings-with-carriage-returns:'[fixed]\r\n[agent]\r\n'; do
    mkdir -p "$u/egress-${fixture%%:*}" || fixture_fail
    printf '%b' "${fixture#*:}" >"$u/egress-${fixture%%:*}/gate.txt" || fixture_fail
  done
  unit egress-entry 1 failing egress_entries "$u/egress-entry"
  unit egress-blank-line 1 failing egress_entries "$u/egress-blank-line"
  unit egress-last-line-without-lf 1 failing egress_entries "$u/egress-last-line-without-lf"
  unit egress-carriage-returns 1 failing egress_entries "$u/egress-carriage-returns"
  unit egress-headings-with-carriage-returns 0 passing egress_entries "$u/egress-headings-with-carriage-returns"
  mkdir -p "$u/egress-nested/agent" || fixture_fail
  printf '[fixed]\n[agent]\n' >"$u/egress-nested/gate.txt" || fixture_fail
  printf '[fixed]\n[agent]\nexample.invalid:443\n' >"$u/egress-nested/agent/list.txt" || fixture_fail
  unit egress-entry-in-a-subdirectory 1 failing egress_entries "$u/egress-nested"
  printf '[fixed]\n[agent]\n127.0.0.1:1\n' >"$u/egress-as-a-file" || fixture_fail
  unit egress-path-is-a-file 1 failing egress_entries "$u/egress-as-a-file"
  mkdir -p "$u/egress-headings" || fixture_fail
  printf '[fixed]\n[agent]\n' >"$u/egress-headings/gate.txt" || fixture_fail
  unit egress-headings-only 0 passing egress_entries "$u/egress-headings"
  unit egress-missing 0 passing egress_entries "$u/no-such-directory"
  mkdir -p "$u/egress-nul" "$u/egress-unreadable" || fixture_fail
  printf '[fixed]\000example.invalid:443\n[agent]\n' >"$u/egress-nul/gate.txt" || fixture_fail
  unit egress-nul-byte 1 failing egress_entries "$u/egress-nul"
  printf '[fixed]\n[agent]\n' >"$u/egress-unreadable/gate.txt" || fixture_fail
  chmod 000 "$u/egress-unreadable/gate.txt" || fixture_fail
  if [ -r "$u/egress-unreadable/gate.txt" ]; then
    printf 'case %s/egress-unreadable-file skipped: this system reads a file of mode 000\n' "$name"
  else
    unit egress-unreadable-file 1 failing egress_entries "$u/egress-unreadable"
  fi
}

crate_units() {
  u=$work/u
  printf '%s\r\n' 'reqwest v0.12.0' >"$u/tree-all" || fixture_fail
  printf '%s\n' '|-- hyper v1.0.0' "$(printf '\342\224\234\342\224\200\342\224\200') ureq v2.0.0" '|   `-- rustls v0.23.0 (*)' \
    '    ring v0.17.0' 'aws_lc_rs v1.0.0' 'native-tls v0.2.0' 'openssl-sys v0.9.0' 'Tokio v1.0.0' 'socket2 v0.5.0' \
    'mio v1.0.0 (registry+https://example.invalid)' >>"$u/tree-all" || fixture_fail
  unit tree-every-listed-package 11 failing net_crates "$u/tree-all"
  printf '%s\n' 'halvo-route-wire v0.0.0 (crates/reqwest)' '|-- tokio-util v0.7.0' '|-- hyper-util v0.1.0' \
    '|-- rustls-pemfile v2.0.0' '|-- ringbuf v0.4.0' '`-- mio-extras v2.0.0' >"$u/tree-near" || fixture_fail
  unit tree-near-names 0 passing net_crates "$u/tree-near"
  unit tree-committed-form 0 passing net_crates "$work/stub/tree.out"
}

grant_units() {
  u=$work/u
  base=$work/stub/debug.out
  grant_case() {
    if grant_lines_hold "$u/grants-$1"; then
      verdict "grants-$1" "$2" hold "$3"
    else
      verdict "grants-$1" "$2" miss "$3"
    fi
  }
  awk '$0 != "test pinned_files_hold_their_hashes ... ok"' "$base" >"$u/grants-without-the-pins-ok" || fixture_fail
  awk '$0 != "test no_grant_in_release ... ok"' "$base" >"$u/grants-without-the-grant-ok" || fixture_fail
  awk '/^test (no_grant_in_release|pinned_files_hold_their_hashes) \.\.\. ok$/ { held = held $0 "\n"; next } { print } $0 == "test pick_rule ... ok" { printf "%s", held }' "$base" >"$u/grants-under-the-next-target" || fixture_fail
  awk '!/Running tests\/no_grant_in_release\.rs /' "$base" >"$u/grants-without-the-target-line" || fixture_fail
  awk '{ print } /Running tests\/no_grant_in_release\.rs / { print }' "$base" >"$u/grants-target-line-twice" || fixture_fail
  awk '{ sub(/deps\/no_grant_in_release-/, "deps/pick_rules-") } { print }' "$base" >"$u/grants-other-binary" || fixture_fail
  awk '{ sub(/Running tests\/no_grant_in_release\.rs /, "Running tests/forge.rs ") } { print }' "$base" >"$u/grants-other-source" || fixture_fail
  awk '$0 == "test no_grant_in_release ... ok" { print "   Doc-tests halvo_route" } { print }' "$base" >"$u/grants-after-a-doc-tests-line" || fixture_fail
  awk '$0 == "test pinned_files_hold_their_hashes ... ok" { $0 = "test pinned_files_hold_their_hashes - should panic ... ok" } { print }' "$base" >"$u/grants-should-panic" || fixture_fail
  awk '{ print } $0 == "test no_grant_in_release ... ok" { print }' "$base" >"$u/grants-ok-line-twice" || fixture_fail
  awk '/Running tests\/no_grant_in_release\.rs / { sub(/\)$/, "") } { print }' "$base" >"$u/grants-target-line-without-its-paren" || fixture_fail
  for fixture in without-the-pins-ok without-the-grant-ok under-the-next-target without-the-target-line target-line-twice \
    other-binary other-source after-a-doc-tests-line should-panic ok-line-twice target-line-without-its-paren; do
    grant_case "$fixture" miss failing
  done
  awk '/^ *Running / { gsub(/\//, "\\"); sub(/\)$/, ".exe)") } { printf "%s\r\n", $0 }' "$base" >"$u/grants-windows-form" || fixture_fail
  grant_case windows-form hold passing
  cp "$work/stub/release.out" "$u/grants-release-form" || fixture_fail
  grant_case release-form hold passing
  if grant_lines_hold "$base" "$work/stub/empty.out"; then
    verdict grants-second-profile-empty miss hold failing
  else
    verdict grants-second-profile-empty miss miss failing
  fi
}

probe_units() {
  u=$work/u
  probe_case() {
    if probe_stops "$u/probe-$1" 4; then
      verdict "probe-$1" "$2" stops "$3"
    else
      verdict "probe-$1" "$2" holds-no-stop "$3"
    fi
  }
  e0624='error[E0624]: associated function test_hosted is private'
  printf '' >"$u/probe-no-error" || fixture_fail
  printf 'src/bin/probe.rs:4:26: error[E0599]: no function or associated item named test_hosted found\n' >"$u/probe-e0599" || fixture_fail
  printf 'src/bin/probe.rs:5:26: %s\n' "$e0624" >"$u/probe-another-line" || fixture_fail
  printf 'src/bin/probe.rs:4:26: %s\ncrates/halvo-route/src/leave.rs:586:5: error[E0432]: unresolved import sha2\n' "$e0624" >"$u/probe-and-an-error-in-the-library" || fixture_fail
  printf 'src/bin/probe.rs:4:26: %s\nerror[E0432]: unresolved import sha2\n  --> crates/halvo-route/src/leave.rs:586:5\n' "$e0624" >"$u/probe-and-a-long-form-error" || fixture_fail
  printf 'other/src/bin/probe.rs:4:26: %s\n' "$e0624" >"$u/probe-another-file" || fixture_fail
  printf 'src/bin/probe.rs:4:26: error: associated function test_hosted is private\n' >"$u/probe-error-without-a-code" || fixture_fail
  for fixture in no-error e0599 another-line and-an-error-in-the-library and-a-long-form-error another-file error-without-a-code; do
    probe_case "$fixture" holds-no-stop failing
  done
  cp "$work/stub/probe.out" "$u/probe-committed-form" || fixture_fail
  probe_case committed-form stops passing
  printf 'src\\bin\\probe.rs:4:26: %s\r\nerror: aborting due to 1 previous error\r\n' "$e0624" >"$u/probe-windows-form" || fixture_fail
  probe_case windows-form stops passing
}

judge_case() {
  label=$1
  want=$2
  artifacts=$work/u/judge-$label
  mkdir "$artifacts" || fixture_fail
  probe_code=$3
  control_code=$5
  put_file "$work/stub/$4" "$artifacts/probe.txt"
  put_file "$work/stub/$6" "$artifacts/control.out"
  judge_pair
  verdict "judge-$label" "$want" "$value_count" failing
}

put_file() {
  while IFS= read -r copied || [ -n "$copied" ]; do
    printf '%s\n' "$copied"
  done <"$1" >"$2" || fixture_fail
}

judge_units() {
  judge_case probe-exits-0-with-the-error-text 1 0 probe-text.out 0 control.out
  judge_case control-answers-grant 1 101 probe.out 0 control-grant.out
  judge_case control-exits-1 1 101 probe.out 1 control.out
  judge_case probe-and-control-miss 2 0 empty.out 101 control.out
  artifacts=$work/u/judge-tree-fails
  mkdir "$artifacts" || fixture_fail
  : >"$artifacts/tree.txt" || fixture_fail
  tree_code=101
  crates_of
  verdict judge-tree-fails 1 "$crates" failing
  tree_code=0
  put_file "$work/stub/tree.out" "$artifacts/tree.txt"
  crates_of
  verdict judge-tree-clean 0 "$crates" passing
}

full_cases() {
  check all-hold PASS 'absent ruling=none hosted_picks=0 hosted_rows=0 net_crates=0 entries=0 grants=0 test_value=0'

  for variant in ruling policy recorded egress; do
    mkdir "$work/commit-$variant" || fixture_fail
    make_commit "$work/commit-$variant"
  done
  printf 'ruling=hosted\n' >"$work/commit-ruling/crates/halvo-route/data/ruling.txt" || fixture_fail
  STUB_COMMIT=$work/commit-ruling
  check ruling-file-with-another-line FAIL 'absent ruling=other hosted_picks=0 ' "FAIL $name (ruling)"
  cp "$work/u/policy-frontier" "$work/commit-policy/crates/halvo-route/data/policy.tsv" || fixture_fail
  STUB_COMMIT=$work/commit-policy
  check policy-pick-raised-to-frontier FAIL ' hosted_picks=1 hosted_rows=0 ' "FAIL $name (picks)"
  mkdir -p "$work/commit-recorded/docs/r6/outcomes" || fixture_fail
  cp "$work/u/recorded-frontier" "$work/commit-recorded/docs/r6/outcomes/recorded.tsv" || fixture_fail
  STUB_COMMIT=$work/commit-recorded
  check recorded-row-of-a-hosted-tier FAIL ' hosted_rows=1 net_crates=0 ' "FAIL $name (rows)"
  cp "$work/u/recorded-on-device" "$work/commit-recorded/docs/r6/outcomes/recorded.tsv" || fixture_fail
  STUB_COMMIT=$work/commit-recorded
  check recorded-rows-on-the-device PASS ' hosted_rows=0 net_crates=0 '
  mkdir "$work/commit-recorded-directory" || fixture_fail
  make_commit "$work/commit-recorded-directory"
  mkdir -p "$work/commit-recorded-directory/docs/r6/outcomes/recorded.tsv" || fixture_fail
  printf 'x\n' >"$work/commit-recorded-directory/docs/r6/outcomes/recorded.tsv/x" || fixture_fail
  STUB_COMMIT=$work/commit-recorded-directory
  check recorded-path-is-a-directory FAIL ' hosted_rows=1 net_crates=0 ' "FAIL $name (rows)"
  STUB_GITLINK=1
  check egress-entry-that-is-no-file FAIL ' entries=1 grants=0 ' "FAIL $name (entries)"

  mkdir -p "$work/commit-egress/scripts/r6/egress" || fixture_fail
  printf '[fixed]\n127.0.0.1:1\n[agent]\n' >"$work/commit-egress/scripts/r6/egress/gate.txt" || fixture_fail
  STUB_COMMIT=$work/commit-egress
  check egress-list-with-an-entry FAIL ' entries=1 grants=0 ' "FAIL $name (entries)"

  mkdir "$work/commit-no-policy" || fixture_fail
  make_commit "$work/commit-no-policy"
  mv "$work/commit-no-policy/crates/halvo-route/data/policy.tsv" "$work/commit-no-policy/policy.away" || fixture_fail
  STUB_COMMIT=$work/commit-no-policy
  check commit-without-a-policy FAIL ' hosted_picks=1 hosted_rows=0 ' "FAIL $name (picks)"
  mkdir "$work/commit-ruling-and-pick" || fixture_fail
  make_commit "$work/commit-ruling-and-pick"
  printf 'ruling=hosted\n' >"$work/commit-ruling-and-pick/crates/halvo-route/data/ruling.txt" || fixture_fail
  cp "$work/u/policy-frontier" "$work/commit-ruling-and-pick/crates/halvo-route/data/policy.tsv" || fixture_fail
  STUB_COMMIT=$work/commit-ruling-and-pick
  check ruling-and-pick-name-the-ruling FAIL 'absent ruling=other hosted_picks=1 ' "FAIL $name (ruling)"

  STUB_TREE_OUT=$work/stub/tree-reqwest.out
  check tree-names-a-network-package FAIL ' net_crates=1 entries=0 ' "FAIL $name (crates)"

  STUB_DEBUG_OUT=$work/stub/debug-no-grant.out
  check debug-without-the-grant-ok-line FAIL ' grants=1 test_value=0' "FAIL $name (grants)"
  STUB_RELEASE_OUT=$work/stub/release-no-grant.out
  check release-without-the-grant-ok-line FAIL ' grants=1 test_value=0' "FAIL $name (grants)"
  STUB_DEBUG_EXIT=101
  check debug-run-fails FAIL ' grants=1 test_value=0' "FAIL $name (grants)"
  STUB_RELEASE_EXIT=101
  check release-run-fails FAIL ' grants=1 test_value=0' "FAIL $name (grants)"

  STUB_PROBE_EXIT=0
  STUB_PROBE_OUT=$work/stub/empty.out
  check probe-compiles FAIL ' grants=0 test_value=1' "FAIL $name (test_value)"
  STUB_CONTROL_OUT=$work/stub/control-frontier-grant.out
  check control-answers-grant-for-frontier FAIL ' test_value=1' "FAIL $name (test_value)"
  CASE_RUSTFLAGS='--cfg r6_job'
  STUB_JOB_FLAG='--cfg r6_job'
  check job-rustflags-kept PASS ' test_value=0'
  CASE_ENCODED=$(printf -- '--cfg\037r6_job')
  STUB_JOB_FLAG='--cfg r6_job'
  check job-encoded-rustflags-kept PASS ' test_value=0'
  CASE_QUIET=true
  check job-sets-cargo-quiet PASS ' grants=0 test_value=0'

  for kind in tree:crates debug:grants release:grants control:test_value probe:test_value; do
    STUB_CHANGE_AT=${kind%%:*}
    STUB_CHANGE_LINE='? .cargo/config.toml'
    case ${kind%%:*} in
      control | probe) when=pair ;;
      *) when=${kind%%:*} ;;
    esac
    check "tree-changed-by-$STUB_CHANGE_AT" FAIL "tree changed=after-$when ? .cargo/config.toml" "FAIL $name (${kind#*:})"
  done
  printf 'h crates/halvo-route/data/ruling.txt\n' >"$work/tree/flags" || fixture_fail
  check assume-unchanged-file-before-the-run FAIL 'tree changed=before h crates/halvo-route/data/ruling.txt' "FAIL $name (setup)"
  printf 'S crates/halvo-route/data/policy.tsv\n' >"$work/tree/flags" || fixture_fail
  check skip-worktree-file-before-the-run FAIL 'tree changed=before S crates/halvo-route/data/policy.tsv' "FAIL $name (setup)"
  STUB_CHANGE_AT=debug
  STUB_CHANGE_FILE=flags
  STUB_CHANGE_LINE='S crates/halvo-route/src/leave.rs'
  check skip-worktree-set-by-the-debug-run FAIL 'tree changed=after-debug S crates/halvo-route/src/leave.rs' "FAIL $name (grants)"
  STUB_CHANGE_AT=tree
  STUB_CHANGE_FILE=flags
  STUB_CHANGE_LINE='h crates/halvo-route/data/policy.tsv'
  check assume-unchanged-set-by-cargo-tree FAIL 'tree changed=after-tree h crates/halvo-route/data/policy.tsv' "FAIL $name (crates)"
  printf '1 .M N... 100644 100644 100644 0000 0000 crates/halvo-route/data/ruling.txt\n' >"$work/tree/status" || fixture_fail
  check changed-file-before-the-run FAIL 'tree changed=before 1 crates/halvo-route/data/ruling.txt' "FAIL $name (setup)"
  printf '? notes.txt\n! target/\n' >"$work/tree/status" || fixture_fail
  check untracked-and-ignored-before-the-run PASS ' test_value=0'
  STUB_NO_COMMIT=1
  check no-commit FAIL 'tree commit=unreadable' "FAIL $name (setup)"
  STUB_COMMIT=$work/commit-ruling
  STUB_RELEASE_EXIT=101
  STUB_PROBE_EXIT=0
  check first-cause-named FAIL 'absent ruling=other ' ' grants=1 test_value=1' "FAIL $name (ruling)"

  usage=$(sh "$self" --all 2>&1)
  direct "$?" "$usage" usage unknown-argument
  unentered=$(CI_D_REPO=$work/no-such-directory sh "$self" 2>&1)
  direct "$?" "$unentered" setup cannot-enter-the-repository
  : >"$work/a-file" || fixture_fail
  unmade=$(CI_D_REPO=$work/repo CI_D_ARTIFACTS=$work/a-file/artifacts sh "$self" 2>&1)
  direct "$?" "$unmade" setup cannot-make-a-directory
}

self_test() {
  enter_repo
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || setup_fail
  newline='
'
  scratch=$work/u
  mkdir -p "$work/bin" "$work/repo" "$work/artifacts" "$work/stub" "$work/tree" "$work/u" "$work/commit" || setup_fail
  make_commit "$work/commit"
  rejected=0
  wrong=
  data_units
  write_stubs
  PATH=$work/bin:$entry_path
  export PATH
  write_outputs
  reset_stubs
  ruling_units
  row_units
  entry_units
  crate_units
  grant_units
  probe_units
  judge_units
  full_cases
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
  exit 0
}

case ${1:-} in
  --self-test) self_test ;;
  '')
    measure
    exit
    ;;
  *)
    printf 'FAIL %s (usage)\n' "$name"
    exit 1
    ;;
esac
