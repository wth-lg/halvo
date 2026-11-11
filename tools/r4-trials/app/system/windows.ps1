# The reader of the system for Windows, run by system.mjs.
# usage: windows.ps1 -Mode window -Id <pid> -Title <title> | -Mode tree -Id <pid> | -Mode alive -Pairs <pid>:<start>,...
param([string]$Mode, [int]$Id = 0, [string]$Title = 'Trial', [string]$Pairs = '')
$ErrorActionPreference = 'Stop'

function Refuse([string]$detail) {
  [Console]::Error.WriteLine("REFUSED usage: $detail")
  exit 2
}

function Window-State([int]$owner, [string]$title) {
  Add-Type @"
using System; using System.Runtime.InteropServices; using System.Text;
public static class R4Windows {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
}
"@
  $found = New-Object System.Collections.ArrayList
  $callback = [R4Windows+EnumProc] {
    param($h, $l)
    $p = [uint32]0
    [void][R4Windows]::GetWindowThreadProcessId($h, [ref]$p)
    if ($p -eq $owner) { [void]$found.Add($h) }
    return $true
  }
  [void][R4Windows]::EnumWindows($callback, [IntPtr]::Zero)
  $matched = 0
  $visible = 0
  foreach ($h in $found) {
    $text = New-Object System.Text.StringBuilder 512
    [void][R4Windows]::GetWindowText($h, $text, 512)
    if ($text.ToString() -eq $title) {
      $matched++
      if ([R4Windows]::IsWindowVisible($h)) { $visible++ }
    }
  }
  if ($visible -gt 0) { $state = 'onscreen' } elseif ($matched -gt 0) { $state = 'hidden' } else { $state = 'absent' }
  Write-Output "window=$state matched_by=title"
}

function Tree([int]$root) {
  $all = @(Get-CimInstance Win32_Process | Select-Object ProcessId, ParentProcessId, Name, CreationDate)
  $byId = @{}
  foreach ($p in $all) { $byId[[int]$p.ProcessId] = $p }
  if (-not $byId.ContainsKey($root)) {
    Write-Output 'tree count=0 webview=0'
    return
  }
  $rootStart = $byId[$root].CreationDate
  $members = New-Object System.Collections.Generic.List[int]
  $members.Add($root)
  $grew = $true
  while ($grew) {
    $grew = $false
    foreach ($p in $all) {
      $child = [int]$p.ProcessId
      if ($members.Contains([int]$p.ParentProcessId) -and -not $members.Contains($child) -and $p.CreationDate -ge $rootStart) {
        $members.Add($child)
        $grew = $true
      }
    }
  }
  $webviews = @($members | Where-Object { $byId[$_].Name -like 'msedgewebview2*' }).Count
  Write-Output ("tree count={0} webview={1}" -f $members.Count, $webviews)
  foreach ($m in ($members | Sort-Object)) {
    Write-Output ("pid={0} start={1}" -f $m, $byId[$m].CreationDate.ToUniversalTime().Ticks)
  }
}

function Alive([string]$list) {
  $count = 0
  foreach ($pair in ($list -split ',')) {
    if ($pair -eq '') { continue }
    $parts = $pair -split ':'
    if ($parts.Count -ne 2) { Refuse 'alive takes <pid>:<start>,...' }
    $p = Get-CimInstance Win32_Process -Filter ("ProcessId={0}" -f [int]$parts[0])
    if ($p -and ($p.CreationDate.ToUniversalTime().Ticks -eq [int64]$parts[1])) { $count++ }
  }
  Write-Output "alive=$count"
}

switch ($Mode) {
  'window' { Window-State $Id $Title }
  'tree' { Tree $Id }
  'alive' { Alive $Pairs }
  default { Refuse 'window, tree or alive' }
}
