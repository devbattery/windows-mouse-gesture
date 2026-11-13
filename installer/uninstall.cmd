@echo off
rem Glide for Windows uninstall (UAC prompt once). Settings in %APPDATA%\Glide are kept.
rem One line on purpose: this file may live in the folder being deleted, and cmd reads a batch file line by line.
cd /d "%TEMP%" & powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1" %* & pause & exit /b
