cells: taskpolicy.macos-15 taskpolicy.this-mac control.macos-15 control.windows-2025 child-process.this-mac exec.this-mac mapped-read.this-mac mapped-written.this-mac purgeable.this-mac reusable.this-mac kernel-message.this-mac socket-buffer.this-mac child-process.macos-15 exec.macos-15 mapped-read.macos-15 mapped-written.macos-15 purgeable.macos-15 reusable.macos-15 kernel-message.macos-15 socket-buffer.macos-15 child-process.windows-2025 mapped-file.windows-2025 section.windows-2025 broker-process.windows-2025 control-reduced-token.windows-2025 control-standard-user.windows-2025
hosted-run branch=scratch/r2-u02-1 commit=1f407ce7d03c6dc1b30dfe351caa12e84bb3c164 run=43011382012 attempt=1
hosted-run branch=scratch/r2-u02-3 commit=0c1cd6e42456837a89682ff0c441f995e2a3a5d6 run=43013466434 attempt=1
hosted-run branch=scratch/r2-u02-fix-1 commit=3ced50cb72cd4a0197bfc7f0e22a8344045d1331 run=43044047984 attempt=1
hosted-run branch=scratch/r2-u02-fix-2 commit=af1f28650dfc25ce37232265b9bd749bafa17670 run=43047611808 attempt=1
cell taskpolicy.macos-15 runs=1 enforced=no argument=256
cell taskpolicy.this-mac runs=1 enforced=yes argument=256
cell control.macos-15 runs=1 outcome=stopped stop_ms=171 peak_bytes=286524736 control=ok
cell control.windows-2025 runs=1 outcome=refused stop_ms=- peak_bytes=267571200 control=ok
limit child-process.this-mac runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1311080 capped=no
limit exec.this-mac runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1311080 capped=no
limit mapped-read.this-mac runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=2425288 capped=no
limit mapped-written.this-mac runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1360328 capped=no
limit purgeable.this-mac runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1360328 capped=no
limit reusable.this-mac runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=2408904 capped=no
limit kernel-message.this-mac runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=3391848 capped=no
limit socket-buffer.this-mac runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1376616 capped=no
limit child-process.macos-15 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1065664 capped=no
limit exec.macos-15 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1147584 capped=no
limit mapped-read.macos-15 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=2163712 capped=no
limit mapped-written.macos-15 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1065920 capped=no
limit purgeable.macos-15 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1082368 capped=no
limit reusable.macos-15 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=2229184 capped=no
limit kernel-message.macos-15 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=3097280 capped=no
limit socket-buffer.macos-15 runs=1 held_bytes=131530752 limit_bytes=67108864 peak_bytes=1262336 capped=no
limit child-process.windows-2025 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=34295808 capped=no
limit mapped-file.windows-2025 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=942080 capped=no
limit section.windows-2025 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=937984 capped=no
limit broker-process.windows-2025 runs=1 held_bytes=134217728 limit_bytes=67108864 peak_bytes=1298432 capped=no
cell control-reduced-token.windows-2025 runs=1 result=cap-unsupported error=1314
cell control-standard-user.windows-2025 runs=1 result=cap-unsupported error=1314
verdict taskpolicy: not-enforced
