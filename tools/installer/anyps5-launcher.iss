; Inno Setup script for the AnyPS5 Launcher (a per-user install: no administrator rights).
; Build: iscc /DAppVersion=0.1.0 /DExeSource=<path to the release exe> /DOutputDir=<folder> anyps5-launcher.iss

#ifndef AppVersion
  #error Pass /DAppVersion=<x.y.z>
#endif
#ifndef ExeSource
  #error Pass /DExeSource=<path to anyps5-launcher.exe>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

#define AppName "AnyPS5 Launcher"
#define AppExe "AnyPS5-Launcher.exe"

[Setup]
; Never change the AppId: it is how a newer setup finds and replaces an older install.
AppId={{6F0B7F5E-3C1D-4A52-9B8E-0D5E7A41C2A9}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=AnyPS5 Launcher contributors
AppPublisherURL=https://github.com/AlxcBTW/anyps5-launcher
AppSupportURL=https://github.com/AlxcBTW/anyps5-launcher/issues
AppUpdatesURL=https://github.com/AlxcBTW/anyps5-launcher/releases
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\..\crates\anyps5-launcher\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir={#OutputDir}
OutputBaseFilename=AnyPS5-Launcher-{#AppVersion}-Setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern dynamic
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#ExeSource}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; The anyps5:// links and the folder entry "Open with AnyPS5" (docs/DESIGN.md 9.5). HKA is the
; current user for the default install and the machine for an administrator install. The launcher
; writes and removes the same keys from Settings; the uninstaller removes both here.
Root: HKA; Subkey: "Software\Classes\anyps5"; ValueType: string; ValueData: "URL:AnyPS5 Link"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\anyps5"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\anyps5\DefaultIcon"; ValueType: string; ValueData: """{app}\{#AppExe}"",0"
Root: HKA; Subkey: "Software\Classes\anyps5\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""
; Folders only, never files.
Root: HKA; Subkey: "Software\Classes\Directory\shell\AnyPS5"; ValueType: string; ValueData: "Open with AnyPS5"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\AnyPS5"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\{#AppExe}"",0"
Root: HKA; Subkey: "Software\Classes\Directory\shell\AnyPS5\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent
