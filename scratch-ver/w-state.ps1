$ErrorActionPreference = 'Continue'
$fpc = '{0CCE9226-69AE-11D9-BED3-505054503030}'
$pc = '{0CCE922B-69AE-11D9-BED3-505054503030}'
foreach ($guid in @($fpc, $pc)) {
    $row = @(& auditpol.exe /get "/subcategory:$guid" /r | Where-Object { $_ -ne '' } | ConvertFrom-Csv)
    if ($row.Count -gt 0) { "audit $($row[0].Subcategory) = $($row[0].'Inclusion Setting')" } else { "audit $guid unreadable" }
}
$dns = & wevtutil.exe gl Microsoft-Windows-DNS-Client/Operational
"dns-channel " + (($dns | Where-Object { $_ -match '^(enabled|\s+maxSize|\s+retention):' }) -join ' ')
$sec = & wevtutil.exe gl Security
"security-log " + (($sec | Where-Object { $_ -match '^\s+(maxSize|retention|autoBackup):' }) -join ' ')
Get-NetFirewallProfile | ForEach-Object { "profile $($_.Name) enabled=$($_.Enabled) outbound=$($_.DefaultOutboundAction)" }
"halvo-rules=" + @(Get-NetFirewallRule -DisplayName 'halvo-egress-*' -ErrorAction SilentlyContinue).Count
