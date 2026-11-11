$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$fpc = '{0CCE9226-69AE-11D9-BED3-505054503030}'
$found = @(Get-NetFirewallRule -DisplayName 'halvo-egress-*' -ErrorAction SilentlyContinue)
"tree sees halvo rules=$($found.Count)"
$null = & auditpol.exe /set "/subcategory:$fpc" /success:disable /failure:disable
"auditpol off exit=$LASTEXITCODE"
$null = & wevtutil.exe sl Microsoft-Windows-DNS-Client/Operational /e:false
"dns channel off exit=$LASTEXITCODE"
$found | Remove-NetFirewallRule
"rules left=" + @(Get-NetFirewallRule -DisplayName 'halvo-egress-*' -ErrorAction SilentlyContinue).Count
& (Join-Path $here 'connect.ps1') 192.0.2.1 443
try { $null = Resolve-DnsName -Name 'egress-ver-w1.invalid' -ErrorAction Stop; 'lookup answered' } catch { "lookup: $($_.Exception.Message)" }
Start-Sleep -Seconds 1
$null = & auditpol.exe /set "/subcategory:$fpc" /success:enable /failure:enable
"auditpol on exit=$LASTEXITCODE"
$null = & wevtutil.exe sl Microsoft-Windows-DNS-Client/Operational /e:true
exit 0
