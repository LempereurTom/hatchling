# Installs Hatchling for the current user, from the folder of the release
# zip (build-release.ps1 makes it). No administrator rights are needed for
# the program, the Start menu shortcut, the classic right-click menu and the
# entry in Settings > Apps.
#
# The Windows 11 top-level menu entry comes from a signed package. Windows
# accepts it once the certificate shipped next to it is trusted, which asks
# for administrator rights once; the person can say no and keep the rest.
#
#   Installer.cmd, or: powershell -ExecutionPolicy Bypass -File install.ps1

param(
    # Don't ask about the Windows 11 entry; install it only if the
    # certificate is already trusted.
    [switch]$NoPrompt
)

$ErrorActionPreference = "Stop"
$Here = $PSScriptRoot
if (-not $env:LOCALAPPDATA) { throw "LOCALAPPDATA is not set; nothing was installed." }
$Dest = Join-Path $env:LOCALAPPDATA "Programs\Hatchling"
$PackageName = "Hatchling.ContextMenu"
$UninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\Hatchling"

$files = "hatchling.exe", "hatchzip.exe", "hatchzip_shellext.dll", "uninstall.ps1", "Desinstaller.cmd"
foreach ($f in $files) {
    if (-not (Test-Path (Join-Path $Here $f))) {
        throw "$f is missing next to install.ps1. Extract the whole zip, then run Installer.cmd again."
    }
}

Write-Host "Installation de Hatchling..."

# A copy installed by the setup is removed first, so Settings > Apps never
# lists Hatchling twice. Its uninstaller keeps the settings.
$setupKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\io.github.lempereurtom.hatchling_is1"
if (Test-Path $setupKey) {
    $unins = (Get-ItemProperty $setupKey).UninstallString -replace '"', ''
    if ($unins -and (Test-Path $unins)) {
        Start-Process $unins -ArgumentList "/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART" -Wait
    }
}

# Explorer holds the DLL while the package is registered, so it goes first.
Get-AppxPackage -Name $PackageName | Remove-AppxPackage
Get-Process hatchling -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.StartsWith($Dest, [StringComparison]::OrdinalIgnoreCase) } |
    Stop-Process -Force

New-Item -ItemType Directory -Force $Dest | Out-Null
$dllTarget = Join-Path $Dest "hatchzip_shellext.dll"
if (Test-Path $dllTarget) {
    # Explorer may still have it loaded: a loaded DLL can be renamed, not
    # replaced, and the old copy is removed by the next install.
    try { Remove-Item $dllTarget -Force } catch {
        Rename-Item $dllTarget ("hatchzip_shellext.old." + [guid]::NewGuid().ToString("N") + ".dll")
    }
}
Get-ChildItem $Dest -Filter "hatchzip_shellext.old.*.dll" -ErrorAction SilentlyContinue |
    ForEach-Object { try { Remove-Item $_.FullName -Force } catch {} }

$copy = $files + "Hatchling-ContextMenu.msix", "Hatchling.cer"
foreach ($f in $copy) {
    $src = Join-Path $Here $f
    if (Test-Path $src) { Copy-Item $src $Dest -Force }
}
if (Test-Path (Join-Path $Here "Assets")) { Copy-Item (Join-Path $Here "Assets") $Dest -Recurse -Force }
# Downloaded files carry a mark that makes Windows warn on every launch.
Get-ChildItem $Dest -Recurse -File | Unblock-File
$gui = Join-Path $Dest "hatchling.exe"

# Classic menu ("Show more options" on Windows 11, the only menu on 10).
& $gui --register-shell | Out-Null

# Start menu.
$lnk = Join-Path ([Environment]::GetFolderPath("Programs")) "Hatchling.lnk"
$shell = New-Object -ComObject WScript.Shell
$s = $shell.CreateShortcut($lnk)
$s.TargetPath = $gui
$s.WorkingDirectory = [Environment]::GetFolderPath("MyDocuments")
$s.IconLocation = "$gui,0"
$s.Description = "Extraire et creer des archives ZIP, 7z, RAR"
$s.Save()

# Settings > Apps, so it uninstalls like any other program.
$version = ((& (Join-Path $Dest "hatchzip.exe") --version) -split " ")[-1].Trim()
New-Item -Path $UninstallKey -Force | Out-Null
$uninstall = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$Dest\uninstall.ps1`""
@{
    DisplayName     = "Hatchling"
    DisplayVersion  = $version
    Publisher       = "Hatchling (fork de HatchZip)"
    DisplayIcon     = "$gui,0"
    InstallLocation = $Dest
    UninstallString = $uninstall
    URLInfoAbout    = "https://github.com/LempereurTom/hatchling"
}.GetEnumerator() | ForEach-Object { Set-ItemProperty $UninstallKey -Name $_.Key -Value $_.Value }
Set-ItemProperty $UninstallKey -Name NoModify -Value 1 -Type DWord
Set-ItemProperty $UninstallKey -Name NoRepair -Value 1 -Type DWord

Write-Host "Hatchling est installe : menu Demarrer, et clic droit sous 'Afficher d'autres options'."

# The Windows 11 top-level entry.
$msix = Join-Path $Dest "Hatchling-ContextMenu.msix"
$cer = Join-Path $Dest "Hatchling.cer"
$windows11 = [Environment]::OSVersion.Version.Build -ge 22000
if (-not $windows11 -or -not (Test-Path $msix) -or -not (Test-Path $cer)) { return }

$cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($cer)
$isTrusted = { [bool](Get-ChildItem Cert:\LocalMachine\TrustedPeople | Where-Object { $_.Thumbprint -eq $cert.Thumbprint }) }

if (-not (& $isTrusted) -and -not $NoPrompt) {
    Write-Host ""
    Write-Host "Pour afficher Hatchling directement dans le menu clic droit de Windows 11"
    Write-Host "(sans passer par 'Afficher d'autres options'), Windows doit faire confiance"
    Write-Host "au certificat du projet. Il le demande une seule fois, avec les droits administrateur."
    $answer = Read-Host "L'activer maintenant ? (O/N)"
    if ($answer -match "^[oOyY]") {
        $cmd = "Import-Certificate -FilePath '$cer' -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null"
        try {
            Start-Process powershell.exe -Verb RunAs -Wait -ArgumentList "-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", $cmd
        } catch {
            Write-Host "Autorisation refusee : l'entree reste sous 'Afficher d'autres options'."
        }
    }
}

if (& $isTrusted) {
    Add-AppxPackage -Path $msix -ExternalLocation $Dest
    Write-Host "Entree Hatchling ajoutee au menu clic droit de Windows 11."
    Write-Host "Elle apparait dans les nouvelles fenetres de l'Explorateur."
}
