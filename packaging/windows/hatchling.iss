; Hatchling-<version>-x64-setup.exe, built with Inno Setup 6 by
; build-release.ps1 from the folder it stages:
;
;   iscc /DPayload=dist\Hatchling /DAppVersion=0.2.0 packaging\windows\hatchling.iss
;
; Installs for the current user only, with no administrator rights. The one
; optional exception is the Windows 11 top-level menu entry, which needs the
; project's certificate trusted: a task, ticked by default, that shows the
; administrator prompt once. A silent install (winget) never shows it.

#ifndef Payload
  #define Payload "..\..\dist\Hatchling"
#endif
#ifndef AppVersion
  #define AppVersion GetStringFileInfo(Payload + "\hatchling.exe", "ProductVersion")
#endif
#define NumVersion GetVersionNumbersString(Payload + "\hatchling.exe")

[Setup]
; Never change: Windows finds the installed copy to update or remove by it.
AppId=io.github.lempereurtom.hatchling
AppName=Hatchling
AppVersion={#AppVersion}
AppVerName=Hatchling {#AppVersion}
AppPublisher=Hatchling (fork de HatchZip)
AppPublisherURL=https://github.com/LempereurTom/hatchling
AppSupportURL=https://github.com/LempereurTom/hatchling/issues
AppUpdatesURL=https://github.com/LempereurTom/hatchling/releases
AppComments=Extraire et créer des archives ZIP, 7z, RAR
VersionInfoVersion={#NumVersion}
VersionInfoTextVersion={#AppVersion}
VersionInfoProductVersion={#NumVersion}
VersionInfoProductTextVersion={#AppVersion}
VersionInfoCompany=Hatchling
VersionInfoDescription=Hatchling Setup
VersionInfoProductName=Hatchling
VersionInfoCopyright=MIT OR Apache-2.0
DefaultDirName={localappdata}\Programs\Hatchling
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
UsedUserAreasWarning=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\..\dist
OutputBaseFilename=Hatchling-{#AppVersion}-x64-setup
SetupIconFile=..\..\assets\icon\hatchzip.ico
WizardStyle=modern
UninstallDisplayIcon={app}\hatchling.exe
UninstallDisplayName=Hatchling
Compression=lzma2/max
SolidCompression=yes
ChangesAssociations=yes
CloseApplications=yes
LicenseFile={#Payload}\LICENSE-MIT

[Languages]
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
fr.Win11Menu=Afficher Hatchling directement dans le menu clic droit de Windows 11 (Windows demandera une fois l'autorisation administrateur)
en.Win11Menu=Show Hatchling directly in the Windows 11 context menu (Windows asks for administrator approval once)
fr.Registering=Ajout de Hatchling au menu clic droit...
en.Registering=Adding Hatchling to the right-click menu...
fr.OpenApp=Ouvrir Hatchling
en.OpenApp=Open Hatchling
fr.ExtractNote=Par défaut, « Extraire ici » et « Extraire vers » suppriment l'archive au fur et à mesure de l'extraction, pour économiser l'espace disque. Utilisez « Extraire ici (garder l'archive) » pour la conserver.
en.ExtractNote=By default, "Extract here" and "Extract to" remove the archive progressively while extracting, to save disk space. Use "Extract here (keep archive)" to keep it.

[Tasks]
Name: "win11menu"; Description: "{cm:Win11Menu}"; Check: IsWindows11

[Files]
Source: "{#Payload}\hatchling.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Payload}\hatchzip.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Payload}\hatchzip_shellext.dll"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Payload}\Hatchling-ContextMenu.msix"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Payload}\Hatchling.cer"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Payload}\menu11.ps1"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#Payload}\Assets\*"; DestDir: "{app}\Assets"; Flags: ignoreversion
Source: "{#Payload}\LICENSE-*"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Hatchling"; Filename: "{app}\hatchling.exe"; WorkingDir: "{userdocs}"; Comment: "Extraire et créer des archives"

[Registry]
; "Open with" for each archive type, without taking over the default app.
Root: HKCU; Subkey: "Software\Classes\Hatchling.Archive"; ValueType: string; ValueData: "Archive"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Hatchling.Archive\DefaultIcon"; ValueType: string; ValueData: """{app}\hatchling.exe"",0"
Root: HKCU; Subkey: "Software\Classes\Hatchling.Archive\shell\open\command"; ValueType: string; ValueData: """{app}\hatchling.exe"" ""%1"""
Root: HKCU; Subkey: "Software\Classes\Applications\hatchling.exe"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "Hatchling"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\.zip\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.7z\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.rar\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.tar\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.tgz\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.gz\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.xz\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.txz\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.001\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\.r00\OpenWithProgids"; ValueType: string; ValueName: "Hatchling.Archive"; ValueData: ""; Flags: uninsdeletevalue

[Run]
; The classic menu is written by the app itself, so the installer and the
; app's Options switch agree.
Filename: "{app}\hatchling.exe"; Parameters: "--register-shell"; StatusMsg: "{cm:Registering}"; Flags: runhidden waituntilterminated
; The Windows 11 entry: asks for the certificate when the task is ticked, in
; an interactive install only; otherwise registers it only if already trusted.
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\menu11.ps1"" -Ask"; StatusMsg: "{cm:Registering}"; Flags: runhidden waituntilterminated; Check: AskForMenu
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\menu11.ps1"""; StatusMsg: "{cm:Registering}"; Flags: runhidden waituntilterminated; Check: not AskForMenu
Filename: "{app}\hatchling.exe"; Description: "{cm:OpenApp}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "powershell.exe"; Parameters: "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -File ""{app}\menu11.ps1"" -Remove"; RunOnceId: "RemoveMenu11"; Flags: runhidden waituntilterminated
Filename: "{app}\hatchling.exe"; Parameters: "--unregister-shell"; RunOnceId: "UnregisterShell"; Flags: runhidden waituntilterminated

[UninstallDelete]
; Explorer can keep the DLL loaded until it restarts; whatever is locked is
; left for the next install to remove.
Type: files; Name: "{app}\hatchzip_shellext.old.*.dll"
Type: dirifempty; Name: "{app}\Assets"
Type: dirifempty; Name: "{app}"

[Code]
const
  ZipInstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Hatchling';

function IsWindows11: Boolean;
var
  V: TWindowsVersion;
begin
  GetWindowsVersionEx(V);
  Result := V.Build >= 22000;
end;

function AskForMenu: Boolean;
begin
  Result := IsWindows11 and WizardIsTaskSelected('win11menu') and not WizardSilent;
end;

{ Before files are copied: Explorer holds the shell extension while its
  package is registered, so the package goes first, and a DLL still loaded is
  renamed, which Windows allows, instead of overwritten. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
  Dll: String;
begin
  Result := '';
  Exec('powershell.exe', '-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command "Get-AppxPackage -Name Hatchling.ContextMenu | Remove-AppxPackage"',
    '', SW_HIDE, ewWaitUntilTerminated, Code);
  Dll := ExpandConstant('{app}\hatchzip_shellext.dll');
  if FileExists(Dll) and not DeleteFile(Dll) then
    RenameFile(Dll, ExpandConstant('{app}\hatchzip_shellext.old.') + GetDateTimeString('yyyymmddhhnnsszzz', #0, #0) + '.dll');
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  FindRec: TFindRec;
begin
  if CurStep = ssPostInstall then
  begin
    { A copy installed earlier from the zip had its own entry in Settings >
      Apps; this setup's entry replaces it. }
    RegDeleteKeyIncludingSubkeys(HKCU, ZipInstallKey);
    DeleteFile(ExpandConstant('{app}\uninstall.ps1'));
    DeleteFile(ExpandConstant('{app}\Desinstaller.cmd'));
    { Old shell extensions renamed by an earlier update, once unloaded. }
    if FindFirst(ExpandConstant('{app}\hatchzip_shellext.old.*.dll'), FindRec) then
    try
      repeat
        DeleteFile(ExpandConstant('{app}\') + FindRec.Name);
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if CurPageID = wpFinished then
    WizardForm.FinishedLabel.Caption := WizardForm.FinishedLabel.Caption + #13#10#13#10 + CustomMessage('ExtractNote');
end;
