#!/bin/sh
set -eu
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
cd "$ROOT"

say() { printf 'prerequisites: %s\n' "$1"; }
die() {
  printf 'prerequisites: error: %s\n' "$1" >&2
  exit 1
}

CARGO_HOME=${CARGO_HOME:-$HOME/.cargo}
PATH="$CARGO_HOME/bin:$PATH"

check_required_tools() {
  command -v git > /dev/null 2>&1 || die "git is missing. Install it: this script reads the checkout and its lock files through git."
  command -v curl > /dev/null 2>&1 || die "curl is missing. Install it: scripts/fetch-tools.sh downloads the pinned tools with it."
  command -v cargo > /dev/null 2>&1 || die "cargo is missing. Install Rust, for example with rustup: this script runs cargo fetch --locked for every committed Cargo.lock."
  command -v lsof > /dev/null 2>&1 || die "lsof is missing. Install it: scripts/stack.sh asks it which process listens on a port."
  node_wanted=$(sed -n 's/.*"node": "[^0-9"]*\([0-9][0-9]*\).*/\1/p' package.json)
  [ -n "$node_wanted" ] || die "package.json states no Node version under engines.node, where this script reads it."
  command -v node > /dev/null 2>&1 || die "node is missing. Install Node $node_wanted or later, as package.json asks."
  [ "$(node -p 'process.versions.node.split(".")[0]')" -ge "$node_wanted" ] ||
    die "node $(node --version) is too old. Install Node $node_wanted or later, as package.json asks."
}
check_required_tools

if [ "$(git rev-parse --is-inside-work-tree 2> /dev/null)" = true ] && [ -z "$(git rev-parse --show-cdup)" ]; then
  if [ "$(git config --get core.hooksPath || true)" = .githooks ]; then
    say "git hooks path present: .githooks"
  else
    say "setting git core.hooksPath to .githooks"
    git config core.hooksPath .githooks
  fi
fi

pinned=$(sed -n 's/.*"packageManager": "pnpm@\([0-9.]*\).*/\1/p' package.json)
command -v pnpm > /dev/null 2>&1 ||
  die "pnpm is missing. Install version $pinned for your user: npm install --global --prefix \"\$HOME/.local\" pnpm@$pinned, then put \$HOME/.local/bin on PATH."
say "pnpm $(pnpm --version) present; package.json pins $pinned"
say "installing the packages of pnpm-lock.yaml with install scripts off"
pnpm install --frozen-lockfile --ignore-scripts

git ls-files 'spikes/*/pnpm-lock.yaml' | while IFS= read -r lock; do
  say "installing the packages of $lock with install scripts off"
  (cd "$(dirname "$lock")" && pnpm install --frozen-lockfile --ignore-scripts --ignore-workspace)
done

sh scripts/fetch-tools.sh
node scripts/fetch-licenses.mjs

say "installing the Chromium build that the pinned Playwright names"
node_modules/.bin/playwright install chromium

if [ -z "$(git ls-files '*Cargo.lock')" ]; then
  say "no Cargo.lock is committed: nothing for cargo to fetch"
else
  git ls-files '*Cargo.lock' | while IFS= read -r lock; do
    say "cargo fetch --locked for $lock"
    (cd "$(dirname "$lock")" && cargo fetch --locked)
  done
fi
say "done"
