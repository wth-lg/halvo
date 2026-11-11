param(
    [Parameter(Mandatory = $true)][ValidateSet('rights', 'setup', 'collect', 'teardown', 'reap', 'state')][string]$Action,
    [string]$Dir = '',
    [string]$Token = '',
    [int]$Deny = 1,
    [long]$CheckPid = 0,
    [long]$Root = 0,
    [string]$Start = '',
    [int]$Rules = -1,
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

function Get-Setting {
    $parts = @()
    foreach ($pair in @(@('fpc', $FilteringPlatformConnection), @('pc', $ProcessCreation))) {
        $row = @(& auditpol.exe /get "/subcategory:$($pair[1])" /r | Where-Object { $_ -ne '' } | ConvertFrom-Csv)
        if ($row.Count -eq 0) { throw 'auditpol could not read an audit setting' }
        $parts += "$($pair[0])=$($row[0].'Inclusion Setting')"
    }
    $channel = (& wevtutil.exe gl $DnsChannel | Where-Object { $_ -match '^enabled:' }) -join ''
    $parts += "dns=$($channel -match 'true')"
    foreach ($one in @(Get-NetFirewallProfile | Sort-Object Name)) { $parts += "$($one.Name)=$($one.Enabled)" }
    $parts -join ';'
}

function Set-Setting([string]$saved) {
    $values = @{}
    foreach ($part in $saved.Split(';')) {
        $pair = $part.Split('=', 2)
        if ($pair.Count -eq 2) { $values[$pair[0]] = $pair[1] }
    }
    foreach ($pair in @(@('fpc', $FilteringPlatformConnection), @('pc', $ProcessCreation))) {
        if (-not $values.ContainsKey($pair[0])) { continue }
        $success = 'disable'
        $failure = 'disable'
        if ($values[$pair[0]] -like '*Success*') { $success = 'enable' }
        if ($values[$pair[0]] -like '*Failure*') { $failure = 'enable' }
        $null = & auditpol.exe /set "/subcategory:$($pair[1])" "/success:$success" "/failure:$failure"
    }
    if ($values.ContainsKey('dns')) { $null = & wevtutil.exe sl $DnsChannel "/e:$($values['dns'].ToLower())" }
    foreach ($name in @('Domain', 'Private', 'Public')) {
        if ($values.ContainsKey($name)) { Set-NetFirewallProfile -Profile $name -Enabled $values[$name] }
    }
}

function Get-Groups {
    @(Get-NetFirewallRule -DisplayName 'halvo-egress-*' -ErrorAction SilentlyContinue | ForEach-Object { $_.Group } | Sort-Object -Unique)
}

function Test-Live([string]$group) {
    if ($group -notmatch '^halvo-egress-\d+-\d+-(\d+)-w(\d+)$') { return $false }
    $epoch = [long]$Matches[1]
    $process = Get-Process -Id ([int]$Matches[2]) -ErrorAction SilentlyContinue
    if ($null -eq $process) { return $false }
    try { $started = ([DateTimeOffset]$process.StartTime).ToUnixTimeSeconds() } catch { return $true }
    $started -le $epoch
}

function Get-Saved([string]$group) {
    $marker = @(Get-NetFirewallRule -Group $group -ErrorAction SilentlyContinue | Where-Object { $_.DisplayName -eq "$group saved" })
    if ($marker.Count -eq 0) { return '' }
    [string]$marker[0].Description
}

function Remove-Left {
    $live = @()
    $restore = ''
    foreach ($group in Get-Groups) {
        if ($group -eq (Get-Group)) { continue }
        if (Test-Live $group) {
            $live += $group
            continue
        }
        $saved = Get-Saved $group
        if ($restore -eq '') { $restore = $saved }
        Remove-NetFirewallRule -Group $group -ErrorAction SilentlyContinue
    }
    if ($live.Count -eq 0 -and $restore -ne '') { Set-Setting $restore }
    $live
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
    $count++
    $null = New-NetFirewallRule -DisplayName "$group $count" -Group $group -Direction Outbound -Action Block -Protocol ICMPv4 -Profile Any -Enabled True
    $count++
    $null = New-NetFirewallRule -DisplayName "$group $count" -Group $group -Direction Outbound -Action Block -Protocol ICMPv6 -IcmpType 128 -Profile Any -Enabled True
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

function Test-Kept {
    $kept = @(Get-NetFirewallRule -Group (Get-Group) -ErrorAction SilentlyContinue).Count
    if ($kept -ne $Rules) { throw "the run's firewall rules were changed during the run ($kept of $Rules left)" }
    foreach ($pair in @(@($FilteringPlatformConnection, 'Success and Failure'), @($ProcessCreation, 'Success'))) {
        $row = @(& auditpol.exe /get "/subcategory:$($pair[0])" /r | Where-Object { $_ -ne '' } | ConvertFrom-Csv)
        if ($row.Count -eq 0 -or $row[0].'Inclusion Setting' -notlike "$($pair[1])*") { throw "an audit of the run was switched off during the run" }
    }
    if ($Deny -eq 1) {
        $channel = (& wevtutil.exe gl $DnsChannel | Where-Object { $_ -match '^enabled:' }) -join ''
        if ($channel -notmatch 'true') { throw 'the DNS client channel was switched off during the run' }
    }
}

function Write-Log {
    Test-Kept
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
        $protocol = Get-Field $xml 'Protocol'
        $sentBySystem = $Deny -eq 1 -and $record.Id -eq 5157 -and ($protocol -eq '1' -or $protocol -eq '58') -and ($processId -eq 4 -or $processId -eq 0)
        if (-not $tree.ContainsKey($processId) -and -not $sentBySystem) { continue }
        $seen["tree$($record.Id)"]++
        if ((Get-Field $xml 'Direction') -ne '%%14593') { continue }
        $proto = switch ($protocol) { '6' { 'tcp' } '17' { 'udp' } '1' { 'icmp' } '58' { 'icmp' } default { '-' } }
        $address = Format-Address (Get-Field $xml 'DestAddress')
        $port = Get-Field $xml 'DestPort'
        if ($proto -eq 'icmp') { $port = '-' }
        if ($record.Id -eq 5156) {
            if ($Deny -ne 1) { continue }
            if ($proto -ne '-' -and $allowed.ContainsKey("$address $port")) { continue }
        }
        $image = [IO.Path]::GetFileName((Get-Field $xml 'Application'))
        if ($image -eq '') { $image = '-' }
        $shown = "$processId"
        if ($sentBySystem) { $shown = '-'; $image = '-' }
        $lines += [pscustomobject]@{ At = (Get-Instant $record); Order = $record.RecordId; Text = "proto=$proto dest=${address}:$port pid=$shown image=$image" }
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
        $saved = ''
        try {
            $live = @(Remove-Left)
            foreach ($group in $live) { if ($saved -eq '') { $saved = Get-Saved $group } }
            if ($saved -eq '') { $saved = Get-Setting }
            $null = New-NetFirewallRule -DisplayName "$(Get-Group) saved" -Group (Get-Group) -Direction Outbound -Action Allow -Enabled False -Description $saved
            $null = & icacls.exe $Dir /inheritance:r /grant:r '*S-1-5-32-544:(OI)(CI)F' '*S-1-5-18:(OI)(CI)F'
            if ($LASTEXITCODE -ne 0) { throw 'icacls could not keep the tree from writing the run directory' }
            $null = & icacls.exe $Dir /setowner '*S-1-5-32-544'
            if ($LASTEXITCODE -ne 0) { throw 'icacls could not give the run directory to the Administrators group' }
            $null = & auditpol.exe /set "/subcategory:$FilteringPlatformConnection" /success:enable /failure:enable
            if ($LASTEXITCODE -ne 0) { throw 'auditpol could not enable the Filtering Platform Connection audit' }
            $null = & auditpol.exe /set "/subcategory:$ProcessCreation" /success:enable
            if ($LASTEXITCODE -ne 0) { throw 'auditpol could not enable the Process Creation audit' }
            $null = & wevtutil.exe sl $DnsChannel /e:true
            if ($LASTEXITCODE -ne 0) { throw 'wevtutil could not enable the DNS client channel' }
            Set-NetFirewallProfile -Profile Domain, Public, Private -Enabled True
            if ($Deny -eq 1) { $null = Add-Rules }
            $rules = @(Get-NetFirewallRule -Group (Get-Group) -ErrorAction Stop).Count
            [IO.File]::WriteAllText((Join-Path $Dir 'rules'), "$rules`n", $Utf8)
            $now = [DateTime]::UtcNow.ToString('o')
            [IO.File]::WriteAllText((Join-Path $Dir 'start'), $now + "`n", $Utf8)
            Write-Output "rules=$rules start=$now"
        } catch {
            $message = $_.Exception.Message
            Remove-NetFirewallRule -Group (Get-Group) -ErrorAction SilentlyContinue
            $others = @(Get-Groups | Where-Object { Test-Live $_ })
            if ($others.Count -eq 0 -and $saved -ne '') { Set-Setting $saved }
            Write-Output "set-up failed: $message"
            exit 1
        }
        exit 0
    }
    'collect' {
        try { Write-Log } catch { Write-Output "collect failed: $($_.Exception.Message)"; exit 1 }
        exit 0
    }
    'teardown' {
        $saved = Get-Saved (Get-Group)
        Remove-NetFirewallRule -Group (Get-Group) -ErrorAction SilentlyContinue
        $others = @(Get-Groups | Where-Object { Test-Live $_ })
        if ($others.Count -eq 0 -and $saved -ne '') { Set-Setting $saved }
        exit 0
    }
    'reap' {
        try { $null = Remove-Left } catch { Write-Output "reap failed: $($_.Exception.Message)"; exit 1 }
        exit 0
    }
    'state' {
        Write-Output (Get-Setting)
        exit 0
    }
}
