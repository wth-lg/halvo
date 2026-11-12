#!/bin/sh
set -u
ROOT=${HALVO_ROOT:-$(cd "$(dirname "$0")/../../.." && pwd -P)}
HERE="$ROOT/gate/fixtures/outbound"

usage() {
  cat >&2 <<'EOF'
One control of the outbound check: a process of one kind makes exactly one attempt to reach a
host outside loopback. The host is 192.0.2.1, an address reserved for documentation that routes
nowhere. Run it through scripts/outbound-hosts.sh, which denies the attempt before it leaves.

  sh gate/fixtures/outbound/attempt.sh KIND            one attempt outside loopback
  sh gate/fixtures/outbound/attempt.sh KIND loopback   the same program aimed at 127.0.0.1

KIND is node (the Node runtime), workerd (the Worker runtime binary), stripe-mock (the mock's Go
binary), browser (the pinned Chromium) or native (a C program that this run builds).
mach-lookup is a second C program. It asks launchd for the Mach service of the system's DNS,
which the profile denies with a record and without the kill. It has no loopback form.
stripe-mock has no loopback form: it opens no connection of its own, so its attempt is to
listen on the address, and on loopback it would serve until stopped. The browser is the one
that scripts/outbound-hosts.sh browser starts, so run that kind through that mode: a page on
loopback asks for http://192.0.2.1/, or for nothing in the loopback form.
EOF
  exit 64
}

kind=${1:-}
where=${2:-outside}
case "$where" in
  outside) host=192.0.2.1 ;;
  loopback) host=127.0.0.1 ;;
  *) kind= ;;
esac

case "$kind:$where" in
  node:*)
    exec node -e 'require("node:net").connect(9, process.argv[1]).on("error", (e) => console.log("node connect:", e.code))' "$host"
    ;;
  workerd:*)
    set -- "$ROOT"/node_modules/.pnpm/@cloudflare+workerd-darwin-*/node_modules/@cloudflare/workerd-darwin-*/bin/workerd
    exec "$1" test "$HERE/attempt-worker-$where.capnp"
    ;;
  stripe-mock:outside)
    exec "$ROOT/.tools/stripe-mock/stripe-mock" -http-addr "$host:9"
    ;;
  browser:outside)
    exec node "$HERE/attempt-browser.mjs" tcp
    ;;
  browser:loopback)
    exec node "$HERE/attempt-browser.mjs" loopback
    ;;
  native:* | mach-lookup:outside)
    mkdir -p "$ROOT/.tmp"
    build=$(mktemp -d "$ROOT/.tmp/outbound-native.XXXXXX") || exit 71
    case "$kind" in
      native) program=attempt.c ;;
      mach-lookup) program=mach-lookup.c ;;
    esac
    cc -DBUILD_THAT_NEVER_RAN="\"$(basename "$build")\"" -o "$build/attempt" "$HERE/$program" || exit 71
    exec "$build/attempt" "$host"
    ;;
  *) usage ;;
esac
