#!/bin/sh
# The step under a second account of ruling PR-31 item 4 (capture run, control-2d): a record and no gate.
# It runs once, on a GitHub-hosted macOS runner of a scratch push, and lives on that scratch commit alone.
#   sh second-account-step.sh              measure, on a GitHub-hosted runner alone
#   sh second-account-step.sh --self-test  the two refusal cases, with a stand-in sudo first on PATH
# It prints booleans, modes and counts alone, and exits 0 whatever it found.
set +e

say() {
  printf 'second-account: %s\n' "$*"
}

refused() {
  if [ "${GITHUB_ACTIONS:-}" != true ]; then
    say "refused: GITHUB_ACTIONS is not true; this step runs on a GitHub-hosted runner alone"
    return 0
  fi
  if [ "${RUNNER_ENVIRONMENT:-}" != github-hosted ]; then
    say "refused: RUNNER_ENVIRONMENT is not github-hosted; this step runs on a GitHub-hosted runner alone"
    return 0
  fi
  return 1
}

self_test() {
  base=${TMPDIR:-/tmp}
  root=$(mktemp -d "${base%/}/second-account-self-test.XXXXXX") || {
    echo "second-account self-test: FAIL (no scratch directory)"
    exit 1
  }
  mkdir "$root/bin"
  cat >"$root/bin/sudo" <<'EOF'
#!/bin/sh
printf '%s\n' "$*" >>"${0%/bin/sudo}/sudo.log"
exit 1
EOF
  chmod 0755 "$root/bin/sudo"
  : >"$root/sudo.log"
  failures=0
  if [ "$(PATH="$root/bin:$PATH" command -v sudo)" != "$root/bin/sudo" ]; then
    echo "FAIL control: the stand-in sudo is not first on PATH"
    failures=$((failures + 1))
  fi
  got=$(env -u GITHUB_ACTIONS RUNNER_ENVIRONMENT=github-hosted PATH="$root/bin:$PATH" sh "$0")
  code=$?
  case "$code:$got" in
    "0:second-account: refused: GITHUB_ACTIONS is not true"*) echo "PASS case 1: GITHUB_ACTIONS unset" ;;
    *) echo "FAIL case 1: GITHUB_ACTIONS unset gave exit $code"; failures=$((failures + 1)) ;;
  esac
  got=$(env -u RUNNER_ENVIRONMENT GITHUB_ACTIONS=true PATH="$root/bin:$PATH" sh "$0")
  code=$?
  case "$code:$got" in
    "0:second-account: refused: RUNNER_ENVIRONMENT is not github-hosted"*) echo "PASS case 2: RUNNER_ENVIRONMENT unset" ;;
    *) echo "FAIL case 2: RUNNER_ENVIRONMENT unset gave exit $code"; failures=$((failures + 1)) ;;
  esac
  calls=$(wc -l <"$root/sudo.log" | tr -d ' ')
  if [ "$calls" != 0 ]; then
    echo "FAIL: the stand-in sudo was called $calls times"
    failures=$((failures + 1))
  fi
  if [ "$failures" = 0 ]; then
    echo "second-account self-test: PASS (2 refusal cases; the stand-in sudo was called 0 times)"
    exit 0
  fi
  echo "second-account self-test: FAIL ($failures)"
  exit 1
}

mode_of() {
  if [ -e "$1" ] || [ -L "$1" ]; then
    stat -f %Lp "$1"
  else
    echo absent
  fi
}

can() {
  # can <list|read> <path>: whether the second account can list the folder or read the file
  case $1 in
    list) sudo -n -u "$user" /bin/ls "$2" >/dev/null 2>&1 ;;
    read) sudo -n -u "$user" /bin/cat "$2" >/dev/null 2>&1 ;;
  esac && echo yes || echo no
}

entries() {
  /bin/ls -led "$1" 2>/dev/null | grep -c 'everyone allow'
}

record_reads() {
  phase=$1
  say "$phase own: list=$(can list "$own") read_test_data=$(can read "$own/test-data")"
  if [ "$made_link" = yes ]; then
    say "$phase linked through the link: list=$(can list "$link") read_test_data=$(can read "$link/test-data")"
    say "$phase linked at the target: list=$(can list "$target") read_test_data=$(can read "$target/test-data")"
  fi
}

record_modes() {
  phase=$1
  say "$phase mode own=$(mode_of "$own") own_parent=$(mode_of "$scratch/own") scratch=$(mode_of "$scratch") tmp=$(mode_of /private/tmp)"
  say "$phase entries own=$(entries "$own")"
  if [ "$made_link" = yes ]; then
    say "$phase mode target=$(mode_of "$target") target_parent=$(mode_of "$scratch/linked") application_support=$(mode_of "$HOME/Library/Application Support") library=$(mode_of "$HOME/Library") home=$(mode_of "$HOME")"
    say "$phase entries target=$(entries "$target")"
  fi
}

check() {
  phase=$1
  shift
  CARGO_TARGET_DIR="$scratch/target" cargo run --quiet --manifest-path "$scratch/probe/Cargo.toml" -- "$phase" "$@"
  say "$phase check exit=$?"
}

cleanup() {
  if [ "$made_link" = yes ]; then
    unlink "$link"
    made_link=no
    say "link at the runner's data folder removed=$([ -L "$link" ] && echo no || echo yes)"
  fi
  if [ "$made_user" = yes ]; then
    sudo -n sysadminctl -deleteUser "$user" >/dev/null 2>&1
    made_user=no
    say "account removed=$(id "$user" >/dev/null 2>&1 && echo no || echo yes) home_removed=$([ -e "/Users/$user" ] && echo no || echo yes)"
  fi
}

measure() {
  if refused; then
    exit 0
  fi
  if ! sudo -n true >/dev/null 2>&1; then
    say "sudo -n refused; nothing was made or measured"
    exit 0
  fi
  repo=$(pwd)
  made_user=no
  made_link=no
  trap cleanup EXIT
  trap 'cleanup; trap - EXIT; exit 0' INT TERM HUP
  scratch=$(mktemp -d /private/tmp/halvo-second-account.XXXXXX) || exit 0
  chmod 0711 "$scratch"
  user=halvo2nd$(od -An -N3 -tu4 /dev/urandom | tr -d ' ')
  password=$(LC_ALL=C tr -dc 'A-Za-z0-9' </dev/urandom | head -c 24)
  if sudo -n sysadminctl -addUser "$user" -password "$password" >/dev/null 2>&1 && id "$user" >/dev/null 2>&1; then
    made_user=yes
  fi
  unset password
  say "account made=$made_user"
  [ "$made_user" = yes ] || exit 0

  own=$scratch/own/Halvo
  mkdir -p "$own"
  chmod 0711 "$scratch/own"
  chmod 0755 "$own"
  printf 'halvo test data, no captured content\n' >"$own/test-data"
  target=$scratch/linked/Halvo
  link="$HOME/Library/Application Support/Halvo"
  mkdir -p "$target"
  chmod 0711 "$scratch/linked"
  chmod 0700 "$target"
  printf 'halvo test data, no captured content\n' >"$target/test-data"
  if [ -e "$link" ] || [ -L "$link" ]; then
    say "the runner's data folder stands already; the linked case is not made"
  else
    mkdir -p "$HOME/Library/Application Support"
    ln -s "$target" "$link" && made_link=yes
  fi
  say "linked case made=$made_link"

  mkdir -p "$scratch/probe/src"
  cp "$repo/Cargo.lock" "$scratch/probe/Cargo.lock"
  cat >"$scratch/probe/Cargo.toml" <<EOF
[package]
name = "second-account-probe"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
halvo-control = { path = "$repo/crates/halvo-control" }

[workspace]
EOF
  cat >"$scratch/probe/src/main.rs" <<'EOF'
use halvo_control::paths::{check_data_dir, DataDir, DataDirCheck};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let phase = &args[0];
    for pair in args[1..].chunks(2) {
        let answer = match DataDir::from_flag(&pair[1]) {
            Ok(dir) => match check_data_dir(&dir, None) {
                Ok(DataDirCheck::Missing) => "missing".to_string(),
                Ok(DataDirCheck::Own { set_to_0700 }) => format!("own set_to_0700={set_to_0700}"),
                Ok(DataDirCheck::Linked { .. }) => "linked".to_string(),
                Err(fault) => format!("fault {fault:?}"),
            },
            Err(error) => format!("path refused {error:?}"),
        };
        println!("second-account: {phase} check {}: {answer}", pair[0]);
    }
}
EOF

  record_modes before-check
  if [ "$made_link" = yes ]; then
    check first own "$own" linked "$link"
  else
    check first own "$own"
  fi
  record_modes after-check
  record_reads before-entry

  chmod +a "everyone allow list,search,readattr,file_inherit" "$own"
  say "entry added own=$?"
  printf 'written by the runner after the entry\n' >"$own/after-entry"
  if [ "$made_link" = yes ]; then
    chmod +a "everyone allow list,search,readattr,file_inherit" "$target"
    say "entry added target=$?"
    printf 'written by the runner after the entry\n' >"$target/after-entry"
  fi
  record_modes after-entry
  record_reads after-entry
  say "after-entry own: read_file_written_after=$(can read "$own/after-entry")"
  if [ "$made_link" = yes ]; then
    say "after-entry linked at the target: read_file_written_after=$(can read "$target/after-entry")"
    check second own "$own" linked "$link"
  else
    check second own "$own"
  fi
  cleanup
  trap - EXIT
  exit 0
}

if [ "${1:-}" = --self-test ]; then
  self_test
fi
measure
