@echo off
rem Glide for Windows install (UAC prompt once). Options: -NoUiAccess -NoAutoStart -DryRun
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0install.ps1" %*
pause
