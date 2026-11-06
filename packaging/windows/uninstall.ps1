# Removes what install.ps1 added, for the current user.

$Dest = Join-Path $env:LOCALAPPDATA "Programs\HatchZip Fork"
Get-AppxPackage -Name "HatchZipFork.ContextMenu" | Remove-AppxPackage
$gui = Join-Path $Dest "hatchzip-gui.exe"
if (Test-Path $gui) { & $gui --unregister-shell | Out-Host }
Remove-Item (Join-Path ([Environment]::GetFolderPath("Programs")) "HatchZip.lnk") -ErrorAction SilentlyContinue
# The DLL can stay locked by Explorer until it restarts.
Get-ChildItem $Dest -ErrorAction SilentlyContinue | ForEach-Object {
    try { Remove-Item $_.FullName -Recurse -Force } catch { Write-Warning "$($_.Name) is in use; it can be deleted after a restart." }
}
Write-Host "HatchZip was removed. To also stop trusting its certificate, delete 'HatchZip Fork' from Cert:\LocalMachine\TrustedPeople (as administrator)."
