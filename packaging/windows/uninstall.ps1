# Removes what install.ps1 added for the current user. Run from Settings >
# Apps, from Desinstaller.cmd, or directly.

$ErrorActionPreference = "Continue"
if (-not $env:LOCALAPPDATA) { throw "LOCALAPPDATA is not set; nothing was removed." }
$Dest = Join-Path $env:LOCALAPPDATA "Programs\Hatchling"
# Everything below deletes inside $Dest only; make sure it is that folder.
if (-not $Dest.EndsWith("\Programs\Hatchling") -or $Dest.Length -lt 25) { throw "Unexpected install folder '$Dest'; nothing was removed." }

Get-AppxPackage -Name "Hatchling.ContextMenu" | Remove-AppxPackage
Get-Process hatchling -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($Dest, [StringComparison]::OrdinalIgnoreCase) } |
    Stop-Process -Force
$gui = Join-Path $Dest "hatchling.exe"
if (Test-Path $gui) { & $gui --unregister-shell | Out-Null }
Remove-Item (Join-Path ([Environment]::GetFolderPath("Programs")) "Hatchling.lnk") -ErrorAction SilentlyContinue
Remove-Item "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Hatchling" -Recurse -ErrorAction SilentlyContinue

# This script runs from inside the folder, and Explorer may still hold the
# DLL, so the folder is removed by a short-lived helper once this exits.
if (Test-Path $Dest) {
    Start-Process cmd.exe -WindowStyle Hidden -ArgumentList "/c", "timeout /t 3 /nobreak >nul & rmdir /s /q `"$Dest`""
}
Write-Host "Hatchling a ete desinstalle. Vos reglages restent dans %APPDATA%\Hatchling."
Write-Host "Le certificat du projet reste approuve ; pour le retirer (administrateur) : certlm.msc > Personnes autorisees > Hatchling."
