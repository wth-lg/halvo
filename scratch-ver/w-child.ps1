$ErrorActionPreference = 'Continue'
$lines = & C:\Windows\System32\curl.exe -sS -m 5 -o NUL http://192.0.2.1:443/ 2>&1
"curl-exit=$LASTEXITCODE"
$lines | ForEach-Object { "curl: $_" }
exit 0
