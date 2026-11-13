@echo off
rem Glide for Windows uninstall (UAC prompt once). Settings in %APPDATA%\Glide are kept.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1" %*
pause
