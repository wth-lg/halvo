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

# Run 42764934594 (macos-15, macOS 15.8.2): as root this add ends with 0 and shows no
# dialog. The usual advice is to open the right com.apple.trust-settings.admin first. That
# run tried it: macOS 15 refused the write (NO (-60005)) and the add worked without it.
# So the authorization database is left alone.
sudo -n security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain "$ca"
rc=$?
echo "trust-ca: sudo security add-trusted-cert -d (admin trust domain) ended with $rc"
[ "$rc" -eq 0 ] || exit 1

# New trust settings reach the system verifier a moment after the change. The timed runs
# start once it accepts the leaf. If it still rejects the leaf after ten seconds, this
# runner does not trust the test CA, and exit 1 makes the driver print DOES NOT COUNT.
tries=0
until security verify-cert -c "$leaf" -p ssl -n localhost -q >/dev/null 2>&1; do
  tries=$((tries + 1))
  if [ "$tries" -ge 20 ]; then
    echo "trust-ca: security verify-cert still rejects the leaf after 10 s"
    security verify-cert -c "$leaf" -p ssl -n localhost 2>&1 | sed 's/^/trust-ca: verify-cert: /'
    exit 1
  fi
  sleep 0.5
done
echo "trust-ca: security verify-cert accepts the leaf after $tries retries"
