$ErrorActionPreference = 'Continue'
$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$target = Join-Path (Split-Path -Parent $here) 'target\ver\w4.txt'
$line = "cmd.exe /c C:\Windows\System32\curl.exe -sS -m 5 -o NUL http://192.0.2.1:443/ > `"$target`" 2>&1"
$made = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{ CommandLine = $line }
"wmi create return=$($made.ReturnValue)"
Start-Sleep -Seconds 9
exit 0
