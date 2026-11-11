# S5 on windows-2025: two throwaway standard accounts made in this VM, the Run value and the
# child's lifetime as the first account, the pipe DACL across both, the startup toggle as the
# runner's own account. Prints counts, sizes, times and booleans only.
$ErrorActionPreference = 'Continue'
$pub = Join-Path $env:PUBLIC 'r1spike'
New-Item -ItemType Directory -Force -Path $pub | Out-Null
icacls $pub /grant 'Users:(OI)(CI)M' | Out-Null
Copy-Item 'spike\r1-win-b\target\release\r1probe.exe', 'spike\r1-win-b\target\release\r1starter.exe' $pub -Force
$probe = Join-Path $pub 'r1probe.exe'
$starter = Join-Path $pub 'r1starter.exe'

function New-SpikeUser($name) {
  $chars = [char[]](48..57 + 65..90 + 97..122)
  $pw = (-join (1..24 | ForEach-Object { $chars | Get-Random })) + 'aZ9!'
  $secure = ConvertTo-SecureString $pw -AsPlainText -Force
  New-LocalUser -Name $name -Password $secure -AccountNeverExpires | Out-Null
  Add-LocalGroupMember -Group 'Users' -Member $name
  New-Object System.Management.Automation.PSCredential($name, $secure)
}

function Invoke-AsUser($cred, [string[]]$argList, $label, $timeoutSec = 600, [switch]$NoWait) {
  $out = Join-Path $pub "out-$label.txt"
  $all = @($argList) + @('--out', $out)
  try {
    $p = Start-Process -FilePath $probe -ArgumentList $all -Credential $cred -LoadUserProfile -WorkingDirectory $pub -WindowStyle Hidden -PassThru
  } catch {
    "S5 as_user label=$label started=false"
    return $null
  }
  if ($NoWait) { return $p }
  $done = $p.WaitForExit($timeoutSec * 1000)
  "S5 as_user label=$label started=true exited=$done"
  if (-not $done) { Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue }
  if (Test-Path $out) { Get-Content $out }
  return $null
}

$a = New-SpikeUser 'r1spka'
$b = New-SpikeUser 'r1spkb'
"S5 accounts_created=2"

"=== S5 runner account"
& $probe whoami
& $probe session-end-settings

"=== S5 account a: whoami, Run value, lifetime"
Invoke-AsUser $a @('whoami') 'a-whoami' 60
Invoke-AsUser $a @('runvalue', '--dir', $pub, '--starter', $starter, '--child', $probe) 'a-runvalue' 120
Invoke-AsUser $a @('lifetime', '--dir', $pub, '--starter', $starter, '--child', $probe, '--reps', '10') 'a-lifetime' 900

"=== S5 pipe"
Remove-Item (Join-Path $pub 'pipe-a.ready') -ErrorAction SilentlyContinue
$srv = Invoke-AsUser $a @('pipe-server', '--dir', $pub, '--which', 'a', '--clients', '3', '--secs', '60') 'a-pipe-server' -NoWait
$w = 0
while (-not (Test-Path (Join-Path $pub 'pipe-a.ready')) -and $w -lt 60) { Start-Sleep -Milliseconds 500; $w++ }
"S5 pipe_server_ready=$(Test-Path (Join-Path $pub 'pipe-a.ready'))"
Invoke-AsUser $b @('pipe-client', '--dir', $pub, '--which', 'a', '--label', 'second_account') 'b-pipe-client' 60
& $probe pipe-client --dir $pub --which a --label runner_admin
Invoke-AsUser $a @('pipe-client', '--dir', $pub, '--which', 'a', '--label', 'same_account') 'a-pipe-client' 60
Invoke-AsUser $a @('pipe-server', '--dir', $pub, '--which', 'a', '--clients', '0', '--secs', '1', '--name', (Get-Content (Join-Path $pub 'pipe-a.txt'))) 'a-pipe-second-instance' 60
Start-Sleep -Seconds 2
if ($srv -and -not $srv.HasExited) { Stop-Process -Id $srv.Id -Force -ErrorAction SilentlyContinue; "S5 pipe_server_stopped_by_job=true" }
if (Test-Path (Join-Path $pub 'out-a-pipe-server.txt')) { Get-Content (Join-Path $pub 'out-a-pipe-server.txt') }

"=== S5 pipe squat: the second account takes the first account's name before its server starts"
$sqname = (Get-Content (Join-Path $pub 'pipe-a.txt')) + '-sq'
Set-Content -Path (Join-Path $pub 'pipe-sq.txt') -Value $sqname
$sq = Invoke-AsUser $b @('pipe-server', '--dir', $pub, '--which', 'sqb', '--clients', '1', '--secs', '60', '--sddl', 'everyone', '--name', $sqname) 'b-pipe-squat' -NoWait
$w = 0
while (-not (Test-Path (Join-Path $pub 'pipe-sqb.ready')) -and $w -lt 60) { Start-Sleep -Milliseconds 500; $w++ }
Invoke-AsUser $a @('pipe-server', '--dir', $pub, '--which', 'sqa', '--clients', '0', '--secs', '1', '--name', $sqname) 'a-pipe-after-squat' 60
Invoke-AsUser $a @('pipe-client', '--dir', $pub, '--which', 'sqb', '--label', 'owner_to_squatter') 'a-pipe-client-squat' 60
Start-Sleep -Seconds 2
if ($sq -and -not $sq.HasExited) { Stop-Process -Id $sq.Id -Force -ErrorAction SilentlyContinue }
if (Test-Path (Join-Path $pub 'out-b-pipe-squat.txt')) { Get-Content (Join-Path $pub 'out-b-pipe-squat.txt') }

"=== S5 startup toggle in Settings (runner account)"
$value = "`"$starter`" --background --child `"$probe`" --marker `"$pub\m-toggle.txt`" --ready `"$pub\r-toggle.txt`" --form handle"
& $probe runvalue-set --name R1SpikeHalvo --value $value
& $probe startup-state --name R1SpikeHalvo --label before_toggle
& $probe startup-toggle --name R1SpikeHalvo --needles 'r1spike,r1starter'
Start-Sleep -Seconds 2
& $probe startup-state --name R1SpikeHalvo --label after_toggle
& $probe runvalue-set --name R1SpikeHalvo --delete
& $probe startup-state --name R1SpikeHalvo --label after_delete

"=== S5 clean-up"
Remove-LocalUser -Name 'r1spka'
Remove-LocalUser -Name 'r1spkb'
"S5 accounts_removed=$(-not (Get-LocalUser -Name 'r1spka','r1spkb' -ErrorAction SilentlyContinue))"
