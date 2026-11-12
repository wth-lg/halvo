#!/bin/sh
# jobs: macos windows
# rule: Every line of the covered table in docs/verify/store-shred.md names a required mutant row whose verdict is a formal check.
set -u
cd "$CI_D_REPO" || exit 2
exec sh docs/verify/check-covered.sh --as 30-formal-covered "$@"
