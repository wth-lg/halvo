$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$repo = Split-Path -Parent $here
$out = Join-Path $repo 'target\ver\w7.txt'
$runs = @(Get-ChildItem (Join-Path $repo 'target\egress') -Directory | Where-Object { $_.Name -match '^\d+$' } | Sort-Object { [int]$_.Name })
$dir = $runs[-1].FullName
[IO.File]::WriteAllText((Join-Path $dir 'run\status'), "0`n")
$me = Get-CimInstance Win32_Process -Filter "ProcessId=$PID"
$parent = Get-CimInstance Win32_Process -Filter "ProcessId=$($me.ParentProcessId)"
"tree: run dir $($runs[-1].Name), my parent is $($parent.Name)"
Stop-Process -Id $parent.ProcessId -Force
Start-Sleep -Seconds 25
[IO.File]::AppendAllText($out, "rules at the connect=" + @(Get-NetFirewallRule -DisplayName 'halvo-egress-*' -ErrorAction SilentlyContinue).Count + "`n")
& (Join-Path $here 'connect.ps1') 192.0.2.1 443 $out
exit 0
