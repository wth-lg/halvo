param([long]$Bytes)
try {
  $a = [byte[]]::new($Bytes)
  for ($i = 0L; $i -lt $Bytes; $i += 4096) { $a[$i] = 1 }
  $p = Get-Process -Id $PID
  "private touched_bytes=$Bytes working_set=$($p.WorkingSet64) private=$($p.PrivateMemorySize64)"
  exit 0
} catch {
  "private refused: $($_.Exception.GetType().FullName)"
  exit 3
}
