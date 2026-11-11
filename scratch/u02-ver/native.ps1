param([long]$Bytes)
$info = [GC]::GetGCMemoryInfo()
"gc total_available_memory_bytes=$($info.TotalAvailableMemoryBytes)"
try {
  $p = [System.Runtime.InteropServices.Marshal]::AllocHGlobal([IntPtr]$Bytes)
  for ($i = 0L; $i -lt $Bytes; $i += 65536) { [System.Runtime.InteropServices.Marshal]::WriteByte($p, $i, 1) }
  "native touched_bytes=$Bytes"
  exit 0
} catch {
  "native refused: $($_.Exception.GetType().FullName)"
  exit 3
}
