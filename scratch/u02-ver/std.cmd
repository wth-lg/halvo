@echo off
whoami > "%~dp0std.out" 2>&1
"%~dp0mp-head.exe" --limit 268435456 --control >> "%~dp0std.out" 2>&1
echo head-control-exit=%ERRORLEVEL% >> "%~dp0std.out"
"%~dp0mp-nows.exe" --limit 268435456 --control >> "%~dp0std.out" 2>&1
echo nows-control-exit=%ERRORLEVEL% >> "%~dp0std.out"
whoami /priv | findstr /i "Se" >> "%~dp0std.out"
whoami /groups | findstr /i "Administrators Mandatory" >> "%~dp0std.out"
