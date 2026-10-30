; SPDX-License-Identifier: GPL-3.0-or-later
; Copyright (C) 2026 Reviera contributors
;
; Inno Setup script (Inno Setup 7, or 6.3 and later) for BIM Reviera. build-installer.ps1 compiles it with:
;   ISCC /DAppVersion=0.1.0-pre.5 /DAppVersionNumeric=0.1.0 /DStageDir=<stage> /DOutputDir=<dist> reviera.iss
; The stage folder holds reviera.exe, its DLLs, samples\, COPYING.txt, THIRD-PARTY-NOTICES.txt
; and reviera.ico.

#ifndef AppVersion
  #define AppVersion "0.1.0-pre.5"
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

#define AppName "BIM Reviera"
#define AppExe "reviera.exe"
; Opens the window with no console window (reviera.exe is a console program: the command-line tool).
#define GuiExe "reviera-gui.exe"
#define AppUrl "https://github.com/Rational-Boxes/NoDesk_Reviera"
#define ProgId "Reviera.IfcModel"

[Setup]
; A fixed AppId keeps upgrades and the uninstaller tied to one entry. Never change it.
AppId={{6B1F2D47-5C8E-4F0A-9D3B-2A7E4C1B9F60}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
VersionInfoVersion={#AppVersionNumeric}
AppPublisher=Reviera contributors
AppPublisherURL=https://bimreviera.com/
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
DefaultDirName={autopf}\BIM Reviera
DefaultGroupName={#AppName}
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
OutputBaseFilename=BIM-Reviera-{#AppVersion}-windows-x64-setup
SetupIconFile={#StageDir}\reviera.ico
UninstallDisplayIcon={app}\reviera.ico
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

[Files]
Source: "{#StageDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
; reviera-gui.exe opens the window (it starts `reviera.exe gui` without a console window);
; reviera.exe in a terminal is the command-line tool.
Name: "{group}\{#AppName}"; Filename: "{app}\{#GuiExe}"; WorkingDir: "{userdocs}"; IconFilename: "{app}\reviera.ico"
Name: "{group}\Third-party notices"; Filename: "{app}\THIRD-PARTY-NOTICES.txt"
Name: "{group}\{cm:UninstallProgram,{#AppName}}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#GuiExe}"; WorkingDir: "{userdocs}"; IconFilename: "{app}\reviera.ico"; Tasks: desktopicon

[Registry]
; .ifc association (optional task). HKA is HKCU for a per-user install and HKLM for all users.
Root: HKA; Subkey: "Software\Classes\.ifc\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "IFC building model"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\reviera.ico"; Tasks: associate
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
    MsgBox('The Microsoft Edge WebView2 Runtime was not found. Reviera works without it, but its ' +
      'Help pages need it. Get it from https://developer.microsoft.com/microsoft-edge/webview2/',
      mbInformation, MB_OK);
end;
