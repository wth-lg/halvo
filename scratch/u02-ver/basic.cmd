@echo off
"%~dp0..\..\target\debug\measure-peak.exe" --limit 268435456 --runs 1 -- cmd.exe /c exit 0 > "%~dp0basic.out" 2>&1
echo measure-exit=%ERRORLEVEL% >> "%~dp0basic.out"
"%~dp0..\..\target\debug\measure-peak.exe" --limit 268435456 --control >> "%~dp0basic.out" 2>&1
echo control-exit=%ERRORLEVEL% >> "%~dp0basic.out"
whoami /priv >> "%~dp0basic.out"
whoami /groups | findstr /i "Mandatory Administrators" >> "%~dp0basic.out"
