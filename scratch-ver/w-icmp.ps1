$ErrorActionPreference = 'Continue'
$lines = & C:\Windows\System32\PING.EXE -n 2 -w 1000 192.0.2.1
"ping-exit=$LASTEXITCODE"
$lines | ForEach-Object { "ping: $_" }
exit 0
