# Installs this build of HatchZip for the current user, without admin rights:
# copies the programs to %LOCALAPPDATA%\Programs\HatchZip Fork, adds the
# classic right-click menu, a Start menu shortcut, and the Windows 11
# top-level menu entry through a signed sparse package.
#
# The package is signed with a certificate made on this computer. Windows
# only accepts it once that certificate is trusted, which takes one run of
# trust-certificate.ps1 as administrator; until then everything else works
# and the entry stays under "Show more options".
#
#   powershell -ExecutionPolicy Bypass -File packaging\windows\install.ps1

param(
    [string]$Build = (Join-Path $PSScriptRoot "..\..\target\release")
)

$ErrorActionPreference = "Stop"
$Build = (Resolve-Path $Build).Path
$Dest = Join-Path $env:LOCALAPPDATA "Programs\HatchZip Fork"
$Subject = "CN=HatchZip Fork"
$PackageName = "HatchZipFork.ContextMenu"

foreach ($f in "hatchzip-gui.exe", "hatchzip.exe", "hatchzip_shellext.dll") {
    if (-not (Test-Path (Join-Path $Build $f))) {
        throw "$f is missing from $Build. Run: cargo build --release --features rar"
    }
}

# Explorer holds the DLL while the package is registered, so it has to go
# before the files can be replaced.
Get-AppxPackage -Name $PackageName | Remove-AppxPackage
Get-Process hatchzip-gui -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -like "$Dest*" } | Stop-Process -Force

New-Item -ItemType Directory -Force $Dest | Out-Null
$dllTarget = Join-Path $Dest "hatchzip_shellext.dll"
if (Test-Path $dllTarget) {
    # Explorer may still have it loaded; a locked DLL can be renamed, not
    # overwritten, and the old copy goes away at the next restart.
    try { Remove-Item $dllTarget -Force } catch {
        Rename-Item $dllTarget ("hatchzip_shellext.old." + [guid]::NewGuid().ToString("N") + ".dll")
    }
}
Get-ChildItem $Dest -Filter "hatchzip_shellext.old.*.dll" -ErrorAction SilentlyContinue |
    ForEach-Object { try { Remove-Item $_.FullName -Force } catch {} }
Copy-Item (Join-Path $Build "hatchzip-gui.exe") $Dest -Force
Copy-Item (Join-Path $Build "hatchzip.exe") $Dest -Force
Copy-Item (Join-Path $Build "hatchzip.exe") (Join-Path $Dest "hz.exe") -Force
Copy-Item (Join-Path $Build "hatchzip_shellext.dll") $Dest -Force
$gui = Join-Path $Dest "hatchzip-gui.exe"

# Classic menu ("Show more options"), per user.
& $gui --register-shell | Out-Host

# Start menu.
$lnk = Join-Path ([Environment]::GetFolderPath("Programs")) "HatchZip.lnk"
$shell = New-Object -ComObject WScript.Shell
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = $gui
$s.WorkingDirectory = $Dest
$s.IconLocation = "$gui,0"
$s.Save()

# The sparse package for the Windows 11 menu.
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\makeappx.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $sdk) {
    Write-Warning "The Windows SDK (makeappx.exe) was not found, so the Windows 11 top-level entry is skipped."
    return
}
$makeappx = $sdk.FullName
$signtool = Join-Path $sdk.DirectoryName "signtool.exe"

$version = (& (Join-Path $Dest "hatchzip.exe") --version).Split(" ")[-1].Trim()
$stage = Join-Path $env:TEMP "hatchzip-sparse"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $stage "Assets") | Out-Null
(Get-Content (Join-Path $PSScriptRoot "sparse\AppxManifest.xml") -Raw).Replace("{VERSION}", "$version.0") |
    Set-Content (Join-Path $stage "AppxManifest.xml") -Encoding utf8
$png = Join-Path $PSScriptRoot "..\..\assets\icon\png"
Copy-Item (Join-Path $png "hatchzip-48.png") (Join-Path $stage "Assets\StoreLogo.png")
Copy-Item (Join-Path $png "hatchzip-256.png") (Join-Path $stage "Assets\Square150x150Logo.png")
Copy-Item (Join-Path $png "hatchzip-48.png") (Join-Path $stage "Assets\Square44x44Logo.png")
# The external location needs the same assets the manifest names.
Copy-Item (Join-Path $stage "Assets") $Dest -Recurse -Force

$msix = Join-Path $Dest "HatchZip-ContextMenu.msix"
& $makeappx pack /d $stage /p $msix /nv /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "makeappx failed ($LASTEXITCODE)" }

$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq $Subject } | Select-Object -First 1
if (-not $cert) {
    $cert = New-SelfSignedCertificate -Type Custom -Subject $Subject -FriendlyName "HatchZip Fork" `
        -KeyUsage DigitalSignature -CertStoreLocation Cert:\CurrentUser\My `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
}
Export-Certificate -Cert $cert -FilePath (Join-Path $Dest "HatchZip-Fork.cer") | Out-Null
& $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix | Out-Null
if ($LASTEXITCODE -ne 0) { throw "signtool failed ($LASTEXITCODE)" }

$trusted = Get-ChildItem Cert:\LocalMachine\TrustedPeople -ErrorAction SilentlyContinue |
    Where-Object { $_.Thumbprint -eq $cert.Thumbprint }
if (-not $trusted) {
    Write-Host ""
    Write-Host "HatchZip is installed. The entry is under 'Show more options' for now."
    Write-Host "For the Windows 11 top-level entry, run once as administrator:"
    Write-Host "  powershell -ExecutionPolicy Bypass -File `"$(Join-Path $PSScriptRoot 'trust-certificate.ps1')`""
    Write-Host "then run this script again."
    return
}

Add-AppxPackage -Path $msix -ExternalLocation $Dest
Write-Host "HatchZip is installed, with its Windows 11 top-level menu entry."
