#!/bin/bash
# Makes a hosted macOS runner trust the S6 test CA. It changes the machine's trust settings,
# so it refuses to run anywhere but a hosted runner job.
set -u
ca=${1:?path of the CA certificate}
leaf=${2:?path of the leaf certificate}

if [ "${GITHUB_ACTIONS:-}" != true ]; then
  echo "trust-ca: refusing to change trust settings outside a hosted runner job" >&2
  exit 2
fi

# Since macOS 11 a change of trust settings asks for confirmation in a dialog, even as root.
# A runner has nobody to answer it, so the right is opened for the one command and put back.
add_trust() {
  domain=$1
  shift
  sudo security authorizationdb write "com.apple.trust-settings.$domain" allow
  "$@"
  rc=$?
  sudo security authorizationdb remove "com.apple.trust-settings.$domain"
  echo "trust-ca: add-trusted-cert in the $domain domain ended with $rc"
  return "$rc"
}

add_trust admin sudo security add-trusted-cert -d -r trustRoot \
  -k /Library/Keychains/System.keychain "$ca" ||
  add_trust user security add-trusted-cert -r trustRoot \
    -k "$HOME/Library/Keychains/login.keychain-db" "$ca" ||
  exit 1

# New trust settings reach the system verifier a moment after the change. The timed runs
# start once it accepts the leaf, or after ten seconds with a warning.
tries=0
until security verify-cert -c "$leaf" -p ssl -n localhost -q >/dev/null 2>&1; do
  tries=$((tries + 1))
  if [ "$tries" -ge 20 ]; then
    echo "trust-ca: warning: security verify-cert still rejects the leaf after 10 s"
    exit 0
  fi
  sleep 0.5
done
echo "trust-ca: security verify-cert accepts the leaf after $tries retries"
