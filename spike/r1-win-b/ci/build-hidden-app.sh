#!/bin/sh
# The app at this commit, built with its one window created at launch and hidden (RD-37).
set -u
cfg='{"app":{"windows":[{"label":"main","title":"Halvo","width":620,"height":720,"minWidth":480,"minHeight":480,"resizable":true,"visible":false}]}}'
npm run tauri build -- --no-bundle --config "$cfg" >"$RUNNER_TEMP/build-hidden.log" 2>&1
rc=$?
echo "PROBE build_hidden_app exit=$rc exe_bytes=$(wc -c <target/release/halvo.exe 2>/dev/null || echo 0)"
exit $rc
