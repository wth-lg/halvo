@echo off
"%~dp0mp-head.exe" --limit 268435456 --control > "%~dp0basic2.out" 2>&1
echo head-control-exit=%ERRORLEVEL% >> "%~dp0basic2.out"
"%~dp0mp-nows.exe" --limit 268435456 --control >> "%~dp0basic2.out" 2>&1
echo nows-control-exit=%ERRORLEVEL% >> "%~dp0basic2.out"
whoami /priv | findstr /i "Se" >> "%~dp0basic2.out"
