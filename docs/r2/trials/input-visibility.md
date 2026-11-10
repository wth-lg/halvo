cells: listen-event-access.macos-15 input.windows-2025.playwright input.windows-2025.enigo
hosted-run branch=scratch/r2-probes-1 commit=cdd5ec6e96f84371a4cd5545094a502fedf77cc1 run=42889376963 attempt=1
cell listen-event-access.macos-15 runs=1 preflight=true
cell input.windows-2025.playwright runs=3 keys=0 clicks=0 version=1.65.0 page_keys_count=20 page_clicks_count=5 no_device_keys_count=0 no_device_clicks_count=0 crash_reruns_count=0 start_reruns_count=0
cell input.windows-2025.enigo runs=3 keys=20 clicks=5 version=0.6.1 page_keys_count=20 page_clicks_count=5 no_device_keys_count=20 no_device_clicks_count=5 crash_reruns_count=0 start_reruns_count=0
file chrome-win64.zip set=playwright-1.65.0 address=https://cdn.playwright.dev/builds/cft/158.0.8127.18/win64/chrome-win64.zip size_bytes=215820284 sha256=b796e8a6231463d1a150ee679fd7cb0168b3728162d2d19bf7bbd867d60d55e2
file ffmpeg-win64.zip set=playwright-1.65.0 address=https://cdn.playwright.dev/dbazure/download/playwright/builds/ffmpeg/1013/ffmpeg-win64.zip size_bytes=1663577 sha256=0c4ee6c694e9bffea27128ab543ae573466c5245d7d16c054e40115d7d743874
file chrome-headless-shell-win64.zip set=playwright-1.65.0 address=https://cdn.playwright.dev/builds/cft/158.0.8127.18/win64/chrome-headless-shell-win64.zip size_bytes=129186142 sha256=fdd3721ad0299ddd419d170ce8ab89aecf8631d157b85215474b4a70c8dcd061
file winldd-win64.zip set=playwright-1.65.0 address=https://cdn.playwright.dev/dbazure/download/playwright/builds/winldd/1007/winldd-win64.zip size_bytes=128684 sha256=0069f0d11d4ad6df068a068c003d22fe7dbec192a47bba64b2e115e9c8ce41d8
command cells of a hosted run: sh tools/r2-probes/run-hosted.sh OUT
command this file: r2-probes trial input-visibility --artifact MACOS --artifact WINDOWS [--records THIS-MAC] --lines LINES --out FILE
verdict driver: enigo
