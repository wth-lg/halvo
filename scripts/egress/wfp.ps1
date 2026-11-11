param(
    [Parameter(Mandatory = $true)][ValidateSet('rights', 'setup', 'collect', 'teardown')][string]$Action,
    [string]$Dir = '',
    [string]$Token = '',
    [int]$Deny = 1,
    [long]$CheckPid = 0,
    [long]$Root = 0,
    [string]$Start = '',
    [string]$Listed = ''
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2
Add-Type -AssemblyName System.Numerics

$FilteringPlatformConnection = '{0CCE9226-69AE-11D9-BED3-505054503030}'
$ProcessCreation = '{0CCE922B-69AE-11D9-BED3-505054503030}'
$DnsChannel = 'Microsoft-Windows-DNS-Client/Operational'
$Utf8 = New-Object System.Text.UTF8Encoding $false

function Test-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    (New-Object Security.Principal.WindowsPrincipal $identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Get-Group { "$Token-w$CheckPid" }

function Read-Entries {
    $entries = @()
    foreach ($line in [IO.File]::ReadAllLines((Join-Path $Dir 'entries'))) {
        $parts = $line.Split(' ')
        if ($parts.Count -eq 5) {
            $entries += [pscustomobject]@{ Kind = $parts[2]; Host = $parts[3]; Port = [int]$parts[4] }
        }
    }
    return $entries
}

function Get-Bare([string]$hostText) { $hostText.Trim('[', ']') }

function ConvertTo-Number([string]$address) {
    $bytes = [Net.IPAddress]::Parse($address).GetAddressBytes()
    [Array]::Reverse($bytes)
    $bytes += [byte]0
    New-Object System.Numerics.BigInteger (, [byte[]]$bytes)
}

function ConvertFrom-Number([System.Numerics.BigInteger]$number, [int]$size) {
    $bytes = $number.ToByteArray()
    $out = New-Object byte[] $size
    for ($i = 0; $i -lt [Math]::Min($bytes.Length, $size); $i++) { $out[$size - 1 - $i] = $bytes[$i] }
    (New-Object Net.IPAddress (, $out)).ToString()
}

function Get-Complement([string[]]$addresses, [int]$size) {
    $max = [System.Numerics.BigInteger]::Pow(2, 8 * $size) - 1
    $numbers = @($addresses | ForEach-Object { ConvertTo-Number $_ } | Sort-Object)
    $ranges = @()
    $low = [System.Numerics.BigInteger]::Zero
    foreach ($n in $numbers) {
        if ($n -gt $low) { $ranges += ('{0}-{1}' -f (ConvertFrom-Number $low $size), (ConvertFrom-Number ($n - 1) $size)) }
        $low = $n + 1
    }
    if ($low -le $max) { $ranges += ('{0}-{1}' -f (ConvertFrom-Number $low $size), (ConvertFrom-Number $max $size)) }
    return $ranges
}

function Get-PortComplement([int[]]$ports) {
    $ranges = @()
    $low = 1
    foreach ($p in ($ports | Sort-Object -Unique)) {
        if ($p -gt $low) { $ranges += "$low-$($p - 1)" }
        $low = $p + 1
    }
    if ($low -le 65535) { $ranges += "$low-65535" }
    return $ranges
}

function Remove-Left {
    $rules = @(Get-NetFirewallRule -DisplayName 'halvo-egress-*' -ErrorAction SilentlyContinue)
    $groups = @($rules | ForEach-Object { $_.Group } | Sort-Object -Unique)
    foreach ($group in $groups) {
        if ($group -notmatch '^halvo-egress-\d+-\d+-(\d+)-w(\d+)$') { continue }
        $epoch = [long]$Matches[1]
        $owner = [int]$Matches[2]
        $process = Get-Process -Id $owner -ErrorAction SilentlyContinue
        $alive = $false
        if ($null -ne $process) {
            $started = ([DateTimeOffset]$process.StartTime).ToUnixTimeSeconds()
            $alive = $started -le $epoch
        }
        if (-not $alive) { Remove-NetFirewallRule -Group $group -ErrorAction SilentlyContinue }
    }
}

function Add-Rules {
    $group = Get-Group
    $entries = @(Read-Entries)
    $programs = @([IO.File]::ReadAllLines((Join-Path $Dir 'programs')) | Where-Object { $_ -ne '' } | Sort-Object -Unique)
    $v4 = @($entries | Where-Object { $_.Kind -eq 'v4' } | ForEach-Object { Get-Bare $_.Host } | Sort-Object -Unique)
    $v6 = @($entries | Where-Object { $_.Kind -eq 'v6' } | ForEach-Object { Get-Bare $_.Host } | Sort-Object -Unique)
    $specs = @()
    if ($v4.Count -eq 0 -and $v6.Count -eq 0) {
        $specs += @{ RemoteAddress = 'Any' }
    } else {
        $others = @(Get-Complement $v4 4) + @(Get-Complement $v6 16)
        $specs += @{ RemoteAddress = $others }
        foreach ($address in ($v4 + $v6)) {
            $ports = @($entries | Where-Object { (Get-Bare $_.Host) -eq $address } | ForEach-Object { $_.Port })
            $rest = @(Get-PortComplement $ports)
            if ($rest.Count -gt 0) {
                $specs += @{ RemoteAddress = $address; Protocol = 'TCP'; RemotePort = $rest }
                $specs += @{ RemoteAddress = $address; Protocol = 'UDP'; RemotePort = $rest }
            }
        }
    }
    $count = 0
    foreach ($program in $programs) {
        foreach ($spec in $specs) {
            $count++
            $null = New-NetFirewallRule -DisplayName "$group $count" -Group $group -Direction Outbound -Action Block -Program $program -Profile Any -Enabled True @spec
        }
    }
    $count
}

function Get-Field($xml, [string]$name) {
    foreach ($node in $xml.Event.EventData.Data) { if ($node.Name -eq $name) { return [string]$node.'#text' } }
    ''
}

function Get-Instant($record) {
    ($record.TimeCreated.ToUniversalTime().Ticks - 621355968000000000) * 100
}

function Format-Address([string]$address) {
    $ip = $null
    if (-not [Net.IPAddress]::TryParse(($address -replace '%.*$', ''), [ref]$ip)) { return '-' }
    if ($ip.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetworkV6) { return '[' + ($ip.ToString() -replace '%.*$', '') + ']' }
    $ip.ToString()
}

function Get-Events([hashtable]$filter) {
    try { @(Get-WinEvent -FilterHashtable $filter -ErrorAction Stop) } catch {
        if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') { @() } else { throw }
    }
}

function Write-Log {
    $start = [DateTime]::Parse($Start, $null, [Globalization.DateTimeStyles]::RoundtripKind)
    Start-Sleep -Seconds 2
    $since = $start.ToLocalTime().AddSeconds(-1)
    $tree = @{}
    $images = @{}
    $tree[[long]$Root] = $true
    $created = @(Get-Events @{ LogName = 'Security'; Id = 4688; StartTime = $since })
    foreach ($record in ($created | Sort-Object RecordId)) {
        $xml = [xml]$record.ToXml()
        $child = [Convert]::ToInt64((Get-Field $xml 'NewProcessId'), 16)
        $parent = [Convert]::ToInt64((Get-Field $xml 'ProcessId'), 16)
        $image = [IO.Path]::GetFileName((Get-Field $xml 'NewProcessName'))
        if ($child -eq $Root) { $images[$child] = $image }
        if ($tree.ContainsKey($parent)) {
            $tree[$child] = $true
            $images[$child] = $image
        }
    }
    if (-not $images.ContainsKey([long]$Root)) { throw 'the process-creation audit holds no event for the runner' }
    $allowed = @{}
    foreach ($pair in $Listed.Split(';')) { if ($pair -ne '' -and $pair -ne 'none') { $allowed[$pair] = $true } }
    $lines = @()
    $connections = @(Get-Events @{ LogName = 'Security'; Id = @(5156, 5157); StartTime = $since })
    $seen = @{ '5156' = 0; '5157' = 0; 'tree5156' = 0; 'tree5157' = 0 }
    foreach ($record in $connections) {
        $xml = [xml]$record.ToXml()
        $seen["$($record.Id)"]++
        $processId = [long](Get-Field $xml 'ProcessID')
        if (-not $tree.ContainsKey($processId)) { continue }
        $seen["tree$($record.Id)"]++
        if ((Get-Field $xml 'Direction') -ne '%%14593') { continue }
        $proto = switch (Get-Field $xml 'Protocol') { '6' { 'tcp' } '17' { 'udp' } default { '-' } }
        $address = Format-Address (Get-Field $xml 'DestAddress')
        $port = Get-Field $xml 'DestPort'
        if ($record.Id -eq 5156) {
            if ($Deny -ne 1) { continue }
            if ($proto -ne '-' -and $allowed.ContainsKey("$address $port")) { continue }
        }
        $image = [IO.Path]::GetFileName((Get-Field $xml 'Application'))
        if ($image -eq '') { $image = '-' }
        $lines += [pscustomobject]@{ At = (Get-Instant $record); Order = $record.RecordId; Text = "proto=$proto dest=${address}:$port pid=$processId image=$image" }
    }
    $dns = @()
    if ($Deny -eq 1) { $dns = @(Get-Events @{ LogName = $DnsChannel; Id = 3006; StartTime = $since }) }
    $seen['3006'] = $dns.Count
    $seen['tree3006'] = 0
    foreach ($record in $dns) {
        $processId = [long]$record.ProcessId
        if (-not $tree.ContainsKey($processId)) { continue }
        $seen['tree3006']++
        $image = '-'
        if ($images.ContainsKey($processId)) { $image = $images[$processId] }
        $lines += [pscustomobject]@{ At = (Get-Instant $record); Order = $record.RecordId; Text = "proto=dns dest=resolver pid=$processId image=$image" }
    }
    $text = ''
    foreach ($line in ($lines | Sort-Object At, Order)) { $text += "denied at=$($line.At) $($line.Text)`n" }
    [IO.File]::WriteAllText((Join-Path $Dir 'denied.log'), $text, $Utf8)
    $counts = "created=$($created.Count) tree=$($tree.Count) events5156=$($seen['5156']) events5157=$($seen['5157']) tree5156=$($seen['tree5156']) tree5157=$($seen['tree5157']) events3006=$($seen['3006']) tree3006=$($seen['tree3006'])`n"
    [IO.File]::WriteAllText((Join-Path $Dir 'counts'), $counts, $Utf8)
}

switch ($Action) {
    'rights' {
        if (-not (Test-Admin)) { Write-Output 'wfp needs an elevated shell'; exit 1 }
        if ($null -eq (Get-Command New-NetFirewallRule -ErrorAction SilentlyContinue)) { Write-Output 'the NetSecurity module is missing'; exit 1 }
        if ($null -eq (Get-Command auditpol.exe -ErrorAction SilentlyContinue)) { Write-Output 'auditpol is missing'; exit 1 }
        exit 0
    }
    'setup' {
        try {
            Remove-Left
            $null = & auditpol.exe /set "/subcategory:$FilteringPlatformConnection" /success:enable /failure:enable
            if ($LASTEXITCODE -ne 0) { throw 'auditpol could not enable the Filtering Platform Connection audit' }
            $null = & auditpol.exe /set "/subcategory:$ProcessCreation" /success:enable
            if ($LASTEXITCODE -ne 0) { throw 'auditpol could not enable the Process Creation audit' }
            $null = & wevtutil.exe sl $DnsChannel /e:true
            if ($LASTEXITCODE -ne 0) { throw 'wevtutil could not enable the DNS client channel' }
            $off = @(Get-NetFirewallProfile | Where-Object { -not $_.Enabled })
            if ($off.Count -gt 0) {
                Set-NetFirewallProfile -Profile Domain, Public, Private -Enabled True
                [IO.File]::WriteAllText((Join-Path $Dir 'firewall-enabled'), "yes`n", $Utf8)
            }
            $rules = 0
            if ($Deny -eq 1) { $rules = Add-Rules }
            [IO.File]::WriteAllText((Join-Path $Dir 'rules'), "$rules`n", $Utf8)
            $now = [DateTime]::UtcNow.ToString('o')
            [IO.File]::WriteAllText((Join-Path $Dir 'start'), $now + "`n", $Utf8)
            Write-Output "start=$now"
        } catch {
            Remove-NetFirewallRule -Group (Get-Group) -ErrorAction SilentlyContinue
            Write-Output "set-up failed: $($_.Exception.Message)"
            exit 1
        }
        exit 0
    }
    'collect' {
        try { Write-Log } catch { Write-Output "collect failed: $($_.Exception.Message)"; exit 1 }
        exit 0
    }
    'teardown' {
        Remove-NetFirewallRule -Group (Get-Group) -ErrorAction SilentlyContinue
        if (Test-Path (Join-Path $Dir 'firewall-enabled')) {
            $left = @(Get-NetFirewallRule -DisplayName 'halvo-egress-*' -ErrorAction SilentlyContinue)
            if ($left.Count -eq 0) { Set-NetFirewallProfile -Profile Domain, Public, Private -Enabled False }
        }
        exit 0
    }
}
