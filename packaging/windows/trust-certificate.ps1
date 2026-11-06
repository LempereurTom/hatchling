# Run once, as administrator, after install.ps1: trusts the certificate
# install.ps1 made on this computer, so Windows accepts the package that
# puts HatchZip in the top level of the Windows 11 context menu. Only that
# one certificate, kept in this user's store, is added; its private key
# never leaves the computer.

$ErrorActionPreference = "Stop"
$cer = Join-Path $env:LOCALAPPDATA "Programs\HatchZip Fork\HatchZip-Fork.cer"
if (-not (Test-Path $cer)) {
    # Elevated with another account, LOCALAPPDATA points elsewhere.
    $cer = Get-ChildItem "C:\Users\*\AppData\Local\Programs\HatchZip Fork\HatchZip-Fork.cer" |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $cer) { throw "Run install.ps1 first." }
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null
Write-Host "Certificate trusted. Run install.ps1 again."
