# S9 as a throwaway standard account: the hidden app and the walker both run as that account,
# as the daemon and the app would.
$ErrorActionPreference = 'Continue'
$pub = Join-Path $env:PUBLIC 'r1spike9'
New-Item -ItemType Directory -Force -Path $pub | Out-Null
icacls $pub /grant 'Users:(OI)(CI)M' | Out-Null
Copy-Item 'spike\r1-win-b\target\release\r1probe.exe', 'target\release\halvo.exe' $pub -Force
$probe = Join-Path $pub 'r1probe.exe'
$chars = [char[]](48..57 + 65..90 + 97..122)
$pw = (-join (1..24 | ForEach-Object { $chars | Get-Random })) + 'aZ9!'
$secure = ConvertTo-SecureString $pw -AsPlainText -Force
New-LocalUser -Name 'r1spk9' -Password $secure -AccountNeverExpires | Out-Null
Add-LocalGroupMember -Group 'Users' -Member 'r1spk9'
$cred = New-Object System.Management.Automation.PSCredential('r1spk9', $secure)
$out = Join-Path $pub 'out-s9-standard.txt'
$p = Start-Process -FilePath $probe -ArgumentList @('app-run', '--exe', (Join-Path $pub 'halvo.exe'), '--runs', '3', '--idle-secs', '15', '--walk', '--label', 'standard', '--out', $out) -Credential $cred -LoadUserProfile -WorkingDirectory $pub -WindowStyle Hidden -PassThru
$done = $p.WaitForExit(900000)
"S9 as_user exited=$done"
if (Test-Path $out) { Get-Content $out }
$w = Join-Path $pub 'out-s9-whoami.txt'
$q = Start-Process -FilePath $probe -ArgumentList @('whoami', '--out', $w) -Credential $cred -LoadUserProfile -WorkingDirectory $pub -WindowStyle Hidden -PassThru
$null = $q.WaitForExit(60000)
if (Test-Path $w) { Get-Content $w }
Remove-LocalUser -Name 'r1spk9'
"S9 account_removed=$(-not (Get-LocalUser -Name 'r1spk9' -ErrorAction SilentlyContinue))"
