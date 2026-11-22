# shellcheck shell=sh disable=SC2034
# The two roots of 24-r7-wall, each a directory from the top of the repository
# in lower case: a tracked file under backend_root reads no tracked file
# outside backend_root and model_root. 24-r7-wall and 21-r7-backend-test source
# this file, and 21 runs the backend's tests on a pull request that changes a
# path under either root.
backend_root=backend
model_root=formal/r7-ledger
