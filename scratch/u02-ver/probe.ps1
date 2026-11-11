$ErrorActionPreference = 'Continue'
$root = (Get-Location).Path
$dir = Join-Path $root 'scratch\u02-ver'
$mp = Join-Path $root 'target\debug\measure-peak.exe'
cargo build --locked -p halvo-cap --bins 2>&1 | Select-Object -Last 2
$pw = (Get-Command pwsh).Source
"probe-env pwsh=$($PSVersionTable.PSVersion) os=$([Environment]::OSVersion.VersionString)"

"=== W1 pagefile-backed section of 536870912 bytes, touched, under --limit 268435456"
& $mp --limit 268435456 --runs 1 -- $pw -NoProfile -File (Join-Path $dir 'section.ps1') 536870912 2>&1 | ForEach-Object { "$_" }
"W1 measure-peak exit=$LASTEXITCODE"

"=== W2 private array of 536870912 bytes, touched, under --limit 268435456"
& $mp --limit 268435456 --runs 1 -- $pw -NoProfile -File (Join-Path $dir 'private.ps1') 536870912 2>&1 | ForEach-Object { "$_" }
"W2 measure-peak exit=$LASTEXITCODE"

"=== W3 the child when measure-peak is killed"
$p = Start-Process -FilePath $mp -ArgumentList @('--limit', '268435456', '--runs', '1', '--', $pw, '-NoProfile', '-Command', 'Start-Sleep 60') -PassThru -NoNewWindow -RedirectStandardOutput (Join-Path $env:RUNNER_TEMP 'w3.out')
Start-Sleep 6
$kids = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$($p.Id)")
"W3 measure-peak pid=$($p.Id) children=" + (($kids | ForEach-Object { "$($_.ProcessId):$($_.Name):$($_.CreationDate.ToString('o'))" }) -join ',')
Stop-Process -Id $p.Id -Force
Start-Sleep 3
foreach ($k in $kids) {
  $alive = Get-CimInstance Win32_Process -Filter "ProcessId=$($k.ProcessId)" | Where-Object { $_.CreationDate -eq $k.CreationDate }
  "W3 child $($k.ProcessId) alive_after_parent_killed=$([bool]$alive)"
}

"=== W4 breakaway from the Job, and IsProcessInJob, from inside the capped child"
& $mp --limit 268435456 --runs 1 -- $pw -NoProfile -File (Join-Path $dir 'breakaway.ps1') 2>&1 | ForEach-Object { "$_" }
"W4 measure-peak exit=$LASTEXITCODE"

"=== W5 measure-peak under a basic-user token (runas /trustlevel:0x20000)"
$basicOut = Join-Path $dir 'basic.out'
runas /trustlevel:0x20000 (Join-Path $dir 'basic.cmd') 2>&1 | ForEach-Object { "W5 runas: $_" }
"W5 runas exit=$LASTEXITCODE"
Start-Sleep 10
if (Test-Path $basicOut) { Get-Content $basicOut | ForEach-Object { "W5 $_" } } else { "W5 no output file" }
"=== probes done"
exit 0
