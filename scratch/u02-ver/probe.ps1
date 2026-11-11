$ErrorActionPreference = 'Continue'
$root = (Get-Location).Path
$dir = Join-Path $root 'scratch\u02-ver'
$mp = Join-Path $root 'target\debug\measure-peak.exe'
cargo build --locked -p halvo-cap --bins 2>&1 | Select-Object -Last 1
$pw = (Get-Command pwsh).Source
"probe-env pwsh=$($PSVersionTable.PSVersion) os=$([Environment]::OSVersion.VersionString)"

"=== V1 pagefile-backed section of 536870912 bytes, touched, under --limit 268435456"
& $mp --limit 268435456 --runs 1 -- $pw -NoProfile -File (Join-Path $dir 'section.ps1') 536870912 2>&1 | ForEach-Object { "V1 $_" }
"V1 measure-peak exit=$LASTEXITCODE"

"=== V2 native HGlobal of 536870912 bytes, and the .NET GC limit, under --limit 268435456"
& $mp --limit 268435456 --runs 1 -- $pw -NoProfile -File (Join-Path $dir 'native.ps1') 536870912 2>&1 | ForEach-Object { "V2 $_" }
"V2 measure-peak exit=$LASTEXITCODE"

"=== V3 the child when measure-peak is killed"
$o3 = Join-Path $env:RUNNER_TEMP 'v3.out'
$e3 = Join-Path $env:RUNNER_TEMP 'v3.err'
$p = Start-Process -FilePath $mp -ArgumentList @('--limit', '268435456', '--runs', '1', '--', 'C:\Windows\System32\ping.exe', '-n', '60', '127.0.0.1') -PassThru -RedirectStandardOutput $o3 -RedirectStandardError $e3
Start-Sleep 5
$all = Get-CimInstance Win32_Process
$kids = @($all | Where-Object { $_.ParentProcessId -eq $p.Id })
"V3 measure-peak pid=$($p.Id) has_exited=$($p.HasExited) children=" + (($kids | ForEach-Object { "$($_.ProcessId):$($_.Name)" }) -join ',')
Stop-Process -Id $p.Id -Force
Start-Sleep 3
foreach ($k in $kids) {
  $alive = @(Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -eq $k.ProcessId -and $_.CreationDate -eq $k.CreationDate })
  "V3 child $($k.ProcessId) $($k.Name) alive_after_parent_killed=$($alive.Count -gt 0)"
}
Get-Content $o3 -ErrorAction SilentlyContinue | ForEach-Object { "V3 out: $_" }

"=== V4 a variant without JOB_OBJECT_LIMIT_WORKINGSET, for the privilege question"
$src = Join-Path $root 'crates\halvo-cap\src\windows.rs'
$text = Get-Content -Raw $src
$variant = $text -replace '\r?\n\s*\| JOB_OBJECT_LIMIT_WORKINGSET', ''
"V4 flag removed: $($variant -ne $text)"
Set-Content -NoNewline -Path $src -Value $variant
$env:CARGO_TARGET_DIR = Join-Path $root 'target-nows'
cargo build --locked -p halvo-cap --bin measure-peak 2>&1 | Select-Object -Last 1
Remove-Item Env:CARGO_TARGET_DIR
git checkout -- crates/halvo-cap/src/windows.rs
"V4 restored: $((git status --porcelain crates/halvo-cap/src/windows.rs) -eq $null)"
$pub = 'C:\Users\Public\u02'
New-Item -ItemType Directory -Force -Path $pub | Out-Null
Copy-Item $mp (Join-Path $pub 'mp-head.exe')
Copy-Item (Join-Path $root 'target-nows\debug\measure-peak.exe') (Join-Path $pub 'mp-nows.exe')
Copy-Item (Join-Path $dir 'basic2.cmd') (Join-Path $pub 'basic2.cmd')
runas /trustlevel:0x20000 (Join-Path $pub 'basic2.cmd') 2>&1 | ForEach-Object { "V4 runas: $_" }
Start-Sleep 12
Get-Content (Join-Path $pub 'basic2.out') -ErrorAction SilentlyContinue | ForEach-Object { "V4 $_" }

"=== V5 a real standard user (net user, not an administrator)"
$pass = 'U02a!' + [guid]::NewGuid().ToString('N').Substring(0, 14)
net user u02probe $pass /add 2>&1 | Out-Null
"V5 user added exit=$LASTEXITCODE"
$cred = New-Object System.Management.Automation.PSCredential('u02probe', (ConvertTo-SecureString $pass -AsPlainText -Force))
Copy-Item (Join-Path $dir 'std.cmd') (Join-Path $pub 'std.cmd')
icacls $pub /grant 'Users:(OI)(CI)M' 2>&1 | Out-Null
try {
  Start-Process -FilePath 'C:\Windows\System32\cmd.exe' -ArgumentList @('/c', (Join-Path $pub 'std.cmd')) -Credential $cred -WorkingDirectory $pub -Wait -LoadUserProfile
  "V5 started as u02probe"
} catch { "V5 start failed: $($_.Exception.Message)" }
Get-Content (Join-Path $pub 'std.out') -ErrorAction SilentlyContinue | ForEach-Object { "V5 $_" }
"=== probes done"
exit 0
