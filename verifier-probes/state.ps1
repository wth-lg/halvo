param([string]$Label = '')
$ErrorActionPreference = 'Continue'
$connection = '{0CCE9226-69AE-11D9-BED3-505054503030}'
$creation = '{0CCE922B-69AE-11D9-BED3-505054503030}'
foreach ($pair in @(@('filtering-platform-connection', $connection), @('process-creation', $creation))) {
    $text = (& auditpol.exe /get "/subcategory:$($pair[1])" | Where-Object { $_ -match '\S' } | Select-Object -Last 1)
    Write-Output ("state {0} audit {1}: {2}" -f $Label, $pair[0], (($text -replace '\s+', ' ').Trim()))
}
$channel = (& wevtutil.exe gl 'Microsoft-Windows-DNS-Client/Operational' | Where-Object { $_ -match '^enabled' })
Write-Output ("state {0} dns-client-channel {1}" -f $Label, ($channel -replace '\s+', ' '))
$rules = @(Get-NetFirewallRule -DisplayName 'halvo-egress-*' -ErrorAction SilentlyContinue)
$groups = @($rules | ForEach-Object { $_.Group } | Sort-Object -Unique)
Write-Output ("state {0} firewall-rules-of-the-check {1} in {2} group(s)" -f $Label, $rules.Count, $groups.Count)
foreach ($rule in ($rules | Select-Object -First 6)) {
    $app = ($rule | Get-NetFirewallApplicationFilter).Program
    Write-Output ("state {0} rule program={1} action={2}" -f $Label, [IO.Path]::GetFileName($app), $rule.Action)
}
Get-NetFirewallProfile | ForEach-Object { Write-Output ("state {0} firewall-profile {1} enabled={2}" -f $Label, $_.Name, $_.Enabled) }
