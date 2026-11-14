# usage: powershell -File tools/r4-trials/window/sample.ps1 -Id <pid>
# Reads, on Windows, the figures of the process <pid> and of its tree as the reader of the trial app
# finds it (tools/r4-trials/app/system/windows.ps1 -Mode tree), and prints the lines of sample.sh:
# key_bytes the summed working set (Get-Process WorkingSet64), second_bytes the summed private bytes
# (Get-Process PrivateMemorySize64), cpu_ms per process (Get-Process TotalProcessorTime), cpu_pct the
# mean use of all processors over the 10 s before the read (Get-Counter), ncpu the processor count.
param([int]$Id = 0)
$ErrorActionPreference = 'Stop'

if ($Id -le 0) {
  [Console]::Error.WriteLine('REFUSED usage: sample.ps1 -Id <pid> takes a process id above 0')
  exit 2
}
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$reader = Join-Path $here '..\app\system\windows.ps1'

$counter = Get-Counter -Counter '\Processor(_Total)\% Processor Time' -SampleInterval 1 -MaxSamples 10
$values = @($counter | ForEach-Object { $_.CounterSamples[0].CookedValue })
$pct = [int][Math]::Round(($values | Measure-Object -Average).Average)

$rows = $null
for ($try = 1; $try -le 3; $try++) {
  $tree = @(& $reader -Mode tree -Id $Id)
  $head = [regex]::Match([string]$tree[0], '^tree count=(\d+) webview=\d+$')
  if (-not $head.Success) { throw 'the tree reader gave no count' }
  $count = [int]$head.Groups[1].Value
  if ($count -eq 0) {
    Write-Output "sample pid=$Id found=no"
    exit 1
  }
  $found = New-Object System.Collections.ArrayList
  foreach ($line in $tree[1..($tree.Count - 1)]) {
    $m = [regex]::Match([string]$line, '^pid=(\d+) start=(\d+)$')
    if (-not $m.Success) { continue }
    $p = Get-Process -Id ([int]$m.Groups[1].Value) -ErrorAction SilentlyContinue
    if ($p) {
      [void]$found.Add([pscustomobject]@{
        Pid = [int]$m.Groups[1].Value
        Start = $m.Groups[2].Value
        Working = [int64]$p.WorkingSet64
        Private = [int64]$p.PrivateMemorySize64
        Cpu = [int64][Math]::Round($p.TotalProcessorTime.TotalMilliseconds)
      })
    }
  }
  if ($found.Count -eq $count) {
    $rows = $found
    break
  }
  Start-Sleep -Seconds 1
}
if ($null -eq $rows) {
  Write-Output "sample pid=$Id found=partial"
  exit 1
}

$key = [int64]0
$second = [int64]0
foreach ($r in $rows) {
  $key += $r.Working
  $second += $r.Private
}
Write-Output ("sample pid={0} key_bytes={1} second_bytes={2} helpers={3} load_x100=- cpu_pct={4} ncpu={5}" -f $Id, $key, $second, ($count - 1), $pct, [Environment]::ProcessorCount)
foreach ($r in $rows) {
  Write-Output ("proc pid={0} start={1} cpu_ms={2}" -f $r.Pid, $r.Start, $r.Cpu)
}
exit 0
