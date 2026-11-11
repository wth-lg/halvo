cells: taskpolicy.macos-15 taskpolicy.this-mac control.macos-15 control.windows-2025
hosted-run branch=scratch/r2-u02-1 commit=1f407ce7d03c6dc1b30dfe351caa12e84bb3c164 run=43011382012 attempt=1
hosted-run branch=scratch/r2-u02-3 commit=0c1cd6e42456837a89682ff0c441f995e2a3a5d6 run=43013466434 attempt=1
cell taskpolicy.macos-15 runs=1 enforced=no argument=256
cell taskpolicy.this-mac runs=1 enforced=yes argument=256
cell control.macos-15 runs=1 outcome=stopped stop_ms=171 peak_bytes=286524736 control=ok
cell control.windows-2025 runs=1 outcome=refused stop_ms=- peak_bytes=267571200 control=ok
verdict taskpolicy: not-enforced
