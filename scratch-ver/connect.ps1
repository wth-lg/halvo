param([string]$HostName, [int]$PortNumber, [string]$OutFile = '')
$client = New-Object System.Net.Sockets.TcpClient
$watch = [Diagnostics.Stopwatch]::StartNew()
$outcome = ''
try {
    $task = $client.ConnectAsync($HostName, $PortNumber)
    if ($task.Wait(5000)) { $outcome = 'connected' } else { $outcome = 'timeout (no answer in 5 s: the SYN was sent)' }
} catch {
    $inner = $_.Exception
    while ($null -ne $inner.InnerException) { $inner = $inner.InnerException }
    if ($inner -is [System.Net.Sockets.SocketException]) { $outcome = "socket-error=$($inner.NativeErrorCode) $($inner.SocketErrorCode)" } else { $outcome = "error $($inner.Message)" }
}
$text = "connect ${HostName}:$PortNumber $outcome after $([int]$watch.Elapsed.TotalSeconds)s"
if ($OutFile -ne '') { [IO.File]::AppendAllText($OutFile, $text + "`n") }
$text
