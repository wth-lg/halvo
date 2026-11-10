#!/bin/bash
# Stands in for the helper in the driver's own test. It reads no key store: its item is
# a file, and "the machine trusts the CA" is a marker file that the test's trust command
# writes. FAKE_HELPER_INSECURE makes it skip verification, to play a control that passes.
set -u
state=${FAKE_HELPER_STATE:?}

case "${1:-}" in
  store)
    printf '%s' "$2" > "$state/item"
    exit 0
    ;;
  delete)
    exit 0
    ;;
  run)
    url=$2
    run_id=$3
    item=$(cat "$state/item") || exit 20
    if [ -n "${FAKE_HELPER_INSECURE:-}" ]; then
      verify=--insecure
    elif [ -e "$state/trusted" ]; then
      verify="--cacert $state/ca.pem"
    else
      verify=
    fi
    # shellcheck disable=SC2086
    code=$(curl -sS -o /dev/null -w '%{http_code}' $verify \
      -H "x-run-id: $run_id" -H "authorization: Bearer $item" "$url" 2>&1) || {
      echo "request failed: $code"
      exit 21
    }
    [ "$code" = 200 ] || exit 22
    echo "phases_us read=1000 client=2000 request=3000 total=6000"
    exit 0
    ;;
esac
exit 2
