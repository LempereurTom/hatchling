# Registers the Windows 11 top-level context menu entry from the signed
# package next to this script, if Windows trusts its certificate. With -Ask,
# offers to trust it first, which shows the administrator prompt once.
# With -Remove, unregisters the package instead.

param([switch]$Ask, [switch]$Remove)

$ErrorActionPreference = "Stop"
$Here = $PSScriptRoot
$PackageName = "Hatchling.ContextMenu"

Get-AppxPackage -Name $PackageName | Remove-AppxPackage
if ($Remove) { return }
if ([Environment]::OSVersion.Version.Build -lt 22000) { return }

$msix = Join-Path $Here "Hatchling-ContextMenu.msix"
$cer = Join-Path $Here "Hatchling.cer"
if (-not (Test-Path $msix) -or -not (Test-Path $cer)) { return }

$cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($cer)
$isTrusted = { [bool](Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object { $_.Thumbprint -eq $cert.Thumbprint }) }

if (-not (& $isTrusted) -and $Ask) {
    $cmd = "Import-Certificate -FilePath '$cer' -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null"
    try {
        Start-Process powershell.exe -Verb RunAs -Wait -WindowStyle Hidden -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $cmd
    } catch {
        # Declined: the entry stays under "Show more options".
        return
    }
}

if (& $isTrusted) {
    Add-AppxPackage -Path $msix -ExternalLocation $Here
}
