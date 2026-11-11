param([long]$Bytes)
$ErrorActionPreference = 'Stop'
try {
  $m = [System.IO.MemoryMappedFiles.MemoryMappedFile]::CreateNew([NullString]::Value, $Bytes)
  $v = $m.CreateViewAccessor(0, $Bytes)
  for ($i = 0L; $i -lt $Bytes; $i += 4096) { $v.Write($i, [byte]1) }
  $p = Get-Process -Id $PID
  "section touched_bytes=$Bytes working_set=$($p.WorkingSet64) private=$($p.PrivateMemorySize64) peak_working_set=$($p.PeakWorkingSet64)"
  Start-Sleep -Milliseconds 800
  $p.Refresh()
  "section after_800ms working_set=$($p.WorkingSet64) private=$($p.PrivateMemorySize64)"
  exit 0
} catch {
  "section failed: $($_.Exception.GetType().FullName): $($_.Exception.Message)"
  exit 4
}
