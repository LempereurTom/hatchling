@echo off
rem Double-click to uninstall Hatchling (also in Settings, Apps).
rem Runs a copy, since uninstalling removes this folder.
copy /y "%~dp0uninstall.ps1" "%TEMP%\hatchling-uninstall.ps1" >nul
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%TEMP%\hatchling-uninstall.ps1"
echo.
pause
