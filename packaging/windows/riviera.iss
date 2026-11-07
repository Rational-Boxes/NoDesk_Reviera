; SPDX-License-Identifier: GPL-3.0-or-later
; Copyright (C) 2026 Riviera contributors
;
; Inno Setup script (Inno Setup 7, or 6.3 and later) for BIM Riviera. build-installer.ps1 compiles it with:
;   ISCC /DAppVersion=0.1.0-pre.12 /DAppVersionNumeric=0.1.0 /DStageDir=<stage> /DOutputDir=<dist> riviera.iss
; The stage folder holds riviera.exe, its DLLs, samples\, COPYING.txt, THIRD-PARTY-NOTICES.txt
; and riviera.ico.

#ifndef AppVersion
  #define AppVersion "0.1.0-pre.12"
#endif
#ifndef AppVersionNumeric
  #define AppVersionNumeric "0.1.0"
#endif
#ifndef StageDir
  #define StageDir "build\stage"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

#define AppName "BIM Riviera"
#define AppExe "riviera.exe"
; Opens the window with no console window (riviera.exe is a console program: the command-line tool).
#define GuiExe "riviera-gui.exe"
#define AppUrl "https://github.com/Rational-Boxes/BIM_Riviera"
#define ProgId "Riviera.IfcModel"

[Setup]
; A fixed AppId keeps upgrades and the uninstaller tied to one entry. Never change it.
AppId={{6B1F2D47-5C8E-4F0A-9D3B-2A7E4C1B9F60}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
VersionInfoVersion={#AppVersionNumeric}
AppPublisher=Riviera contributors
AppPublisherURL=https://bimriviera.com/
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
DefaultDirName={autopf}\BIM Riviera
DefaultGroupName={#AppName}
; Upgrades from the old spellings (NoDesk Reviera, BIM Reviera) get the new group, not theirs.
UsePreviousGroup=no
DisableProgramGroupPage=yes
; The GPL is shown on the licence page.
LicenseFile={#StageDir}\COPYING.txt
; Per-user by default (no administrator rights); the dialog offers an install for all users.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 (TECH_STACK §5.4).
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=BIM-Riviera-{#AppVersion}-windows-x64-setup
SetupIconFile={#StageDir}\riviera.ico
UninstallDisplayIcon={app}\riviera.ico
UninstallDisplayName={#AppName} {#AppVersion}
ChangesAssociations=yes
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "associate"; Description: "Open .ifc files with {#AppName}"; GroupDescription: "File types:"; Flags: unchecked

[InstallDelete]
; What an install under the old spellings left behind (0.1.0-pre.9 and earlier): its programs, icon
; and library index, and its shortcuts, which would still open the old program.
Type: files; Name: "{app}\reviera.exe"
Type: files; Name: "{app}\reviera-gui.exe"
Type: files; Name: "{app}\reviera-analysis-builtin.exe"
Type: files; Name: "{app}\reviera-compute-host.exe"
Type: files; Name: "{app}\reviera.ico"
Type: files; Name: "{app}\library\reviera-library.json"
Type: filesandordirs; Name: "{autoprograms}\NoDesk Reviera"
Type: filesandordirs; Name: "{autoprograms}\BIM Reviera"
Type: files; Name: "{autodesktop}\NoDesk Reviera.lnk"
Type: files; Name: "{autodesktop}\BIM Reviera.lnk"

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
; riviera-gui.exe opens the window (it starts `riviera.exe gui` without a console window);
; riviera.exe in a terminal is the command-line tool.
Name: "{group}\{#AppName}"; Filename: "{app}\{#GuiExe}"; WorkingDir: "{userdocs}"; IconFilename: "{app}\riviera.ico"
Name: "{group}\Third-party notices"; Filename: "{app}\THIRD-PARTY-NOTICES.txt"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#GuiExe}"; WorkingDir: "{userdocs}"; IconFilename: "{app}\riviera.ico"; Tasks: desktopicon

[Registry]
; The old spelling's .ifc association opened reviera-gui.exe, which is removed above.
Root: HKA; Subkey: "Software\Classes\Reviera.IfcModel"; ValueType: none; Flags: deletekey
Root: HKA; Subkey: "Software\Classes\.ifc\OpenWithProgids"; ValueType: none; ValueName: "Reviera.IfcModel"; Flags: deletevalue
Root: HKA; Subkey: "Software\Classes\Applications\reviera-gui.exe"; ValueType: none; Flags: deletekey
; .ifc association (optional task). HKA is HKCU for a per-user install and HKLM for all users.
Root: HKA; Subkey: "Software\Classes\.ifc\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "IFC building model"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\riviera.ico"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#GuiExe}"" ""%1"""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#GuiExe}\SupportedTypes"; ValueType: string; ValueName: ".ifc"; ValueData: ""; Flags: uninsdeletekey; Tasks: associate

[Run]
Filename: "{app}\{#GuiExe}"; WorkingDir: "{userdocs}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
{ The help pages use the Microsoft Edge WebView2 runtime (wxWidgets' Edge backend, linked with the
  static loader, so there is no WebView2Loader.dll to ship). Windows 11 has the runtime, and so
  do updated Windows 10 systems; tell the user where to get it otherwise. }
const
  WebView2Key = 'SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';
  WebView2Key32 = 'SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}';

function WebView2Installed: Boolean;
var
  Version: String;
begin
  Result :=
    (RegQueryStringValue(HKLM, WebView2Key32, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0')) or
    (RegQueryStringValue(HKLM, WebView2Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0')) or
    (RegQueryStringValue(HKCU, WebView2Key, 'pv', Version) and (Version <> '') and (Version <> '0.0.0.0'));
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and (not WizardSilent) and (not WebView2Installed) then
    MsgBox('The Microsoft Edge WebView2 Runtime was not found. Riviera works without it, but its ' +
      'Help pages need it. Get it from https://developer.microsoft.com/microsoft-edge/webview2/',
      mbInformation, MB_OK);
end;
