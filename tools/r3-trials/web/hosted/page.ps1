param(
    [Parameter(Mandatory = $true)][string]$Browser,
    [Parameter(Mandatory = $true)][string]$ProfileDir,
    [Parameter(Mandatory = $true)][string]$Url,
    [Parameter(Mandatory = $true)][string]$Pdf,
    [int]$Wait = 60
)
$ErrorActionPreference = 'Stop'
$arguments = @('--headless=new', "--user-data-dir=`"$ProfileDir`"", '--allow-chrome-scheme-url',
    '--no-pdf-header-footer', '--timeout=4000', "--print-to-pdf=`"$Pdf`"", $Url)
$root = Start-Process -FilePath $Browser -ArgumentList $arguments -PassThru -WindowStyle Hidden
$floor = $root.StartTime.AddSeconds(-1)
$null = $root.WaitForExit($Wait * 1000)
$ended = 0
for ($pass = 0; $pass -lt 10; $pass++) {
    $all = @(Get-CimInstance Win32_Process)
    $tree = @{ ([int]$root.Id) = $true }
    do {
        $grew = $false
        foreach ($q in $all) {
            if (-not $tree.ContainsKey([int]$q.ProcessId) -and $tree.ContainsKey([int]$q.ParentProcessId) -and $q.CreationDate -ge $floor) {
                $tree[[int]$q.ProcessId] = $true
                $grew = $true
            }
        }
    } while ($grew)
    $found = @($all | Where-Object { $tree.ContainsKey([int]$_.ProcessId) -and $_.CreationDate -ge $floor })
    if ($found.Count -eq 0) { break }
    foreach ($q in $found) {
        $again = Get-CimInstance Win32_Process -Filter "ProcessId = $($q.ProcessId)"
        if ($null -ne $again -and $again.CreationDate -eq $q.CreationDate) {
            Stop-Process -Id $q.ProcessId -Force -ErrorAction SilentlyContinue
            $ended++
        }
    }
    Start-Sleep -Milliseconds 500
}
"page_processes_ended=$ended"
