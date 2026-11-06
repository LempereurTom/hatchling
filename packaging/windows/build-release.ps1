# Builds dist\Hatchling-<version>-x64.zip, the file people download: the
# programs, Installer.cmd, and the Windows 11 menu package signed with the
# project's certificate, so installing needs neither Rust nor the Windows SDK.
#
# Needs Rust, the Visual Studio C++ build tools and the Windows SDK. The
# signing certificate is made once in this user's store (CN=Hatchling); keep
# that same certificate for every release, or people who trusted the old
# one have to approve the new one.
#
#   powershell -ExecutionPolicy Bypass -File packaging\windows\build-release.ps1

param(
    [string]$TargetDir = "",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$Root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
if (-not $TargetDir) { $TargetDir = Join-Path $Root "target" }
$Subject = "CN=Hatchling"

if (-not $SkipBuild) {
    $env:CARGO_TARGET_DIR = $TargetDir
    Push-Location $Root
    try {
        cargo build --release --features rar -p hatchzip-gui -p hatchzip -p hatchzip-shellext
        if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    } finally { Pop-Location }
}
$bin = Join-Path $TargetDir "release"
$version = ((& (Join-Path $bin "hatchzip.exe") --version) -split " ")[-1].Trim()

$dist = Join-Path $Root "dist"
$stage = Join-Path $dist "Hatchling"
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force (Join-Path $stage "Assets") | Out-Null

foreach ($f in "hatchling.exe", "hatchzip.exe", "hatchzip_shellext.dll") {
    Copy-Item (Join-Path $bin $f) $stage
}
foreach ($f in "install.ps1", "uninstall.ps1", "Installer.cmd", "Desinstaller.cmd", "LISEZMOI.txt") {
    Copy-Item (Join-Path $PSScriptRoot $f) $stage
}
foreach ($f in "LICENSE-MIT", "LICENSE-APACHE", "LICENSE-UNRAR") {
    Copy-Item (Join-Path $Root $f) $stage
}
$png = Join-Path $Root "assets\icon\png"
Copy-Item (Join-Path $png "hatchzip-48.png") (Join-Path $stage "Assets\StoreLogo.png")
Copy-Item (Join-Path $png "hatchzip-256.png") (Join-Path $stage "Assets\Square150x150Logo.png")
Copy-Item (Join-Path $png "hatchzip-48.png") (Join-Path $stage "Assets\Square44x44Logo.png")

# The Windows 11 menu package.
$sdk = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\10.*\x64\makeappx.exe" |
    Sort-Object FullName -Descending | Select-Object -First 1
if (-not $sdk) { throw "The Windows SDK (makeappx.exe) is needed to build the release." }
$signtool = Join-Path $sdk.DirectoryName "signtool.exe"
$pkg = Join-Path $env:TEMP "hatchling-sparse"
Remove-Item $pkg -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $pkg | Out-Null
Copy-Item (Join-Path $stage "Assets") $pkg -Recurse
(Get-Content (Join-Path $PSScriptRoot "sparse\AppxManifest.xml") -Raw).Replace("{VERSION}", "$version.0") |
    Set-Content (Join-Path $pkg "AppxManifest.xml") -Encoding utf8
$msix = Join-Path $stage "Hatchling-ContextMenu.msix"
& $sdk.FullName pack /d $pkg /p $msix /nv /o | Out-Null
if ($LASTEXITCODE -ne 0) { throw "makeappx failed" }

$cert = Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Subject -eq $Subject } | Select-Object -First 1
if (-not $cert) {
    $cert = New-SelfSignedCertificate -Type Custom -Subject $Subject -FriendlyName "Hatchling" `
        -KeyUsage DigitalSignature -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddYears(10) `
        -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}")
}
& $signtool sign /fd SHA256 /sha1 $cert.Thumbprint $msix | Out-Null
if ($LASTEXITCODE -ne 0) { throw "signtool failed" }
Export-Certificate -Cert $cert -FilePath (Join-Path $stage "Hatchling.cer") | Out-Null

$zip = Join-Path $dist "Hatchling-$version-x64.zip"
Remove-Item $zip -ErrorAction SilentlyContinue
Compress-Archive -Path $stage -DestinationPath $zip
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
# LF, so `sha256sum -c` reads it on Linux and macOS too.
[IO.File]::WriteAllText("$zip.sha256", "$hash  $(Split-Path $zip -Leaf)`n")
Write-Host $zip
Write-Host "SHA-256 $hash"
Write-Host "Certificate thumbprint $($cert.Thumbprint)"
