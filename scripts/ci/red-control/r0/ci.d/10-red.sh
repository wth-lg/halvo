#!/bin/sh
# jobs: macos windows
# rule: The red control of the merge gate: this check always fails, and the run of the driver that holds it must fail. The form --merge-gate of scripts/ci/run-ci-d.sh runs it first and goes on only when that run exits 1.
# lane: free
printf 'FAIL 10-red (planned)\n'
exit 1
