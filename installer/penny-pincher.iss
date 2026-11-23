; Inno Setup script for the Penny Pincher installer.
; Build with: ISCC /DAppVersion=0.2.1 installer\penny-pincher.iss  (after cargo build --release)
; Installs for the current user only, so no admin prompt and the app can update its own exe.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
; Never change AppId: Windows uses it to recognise upgrades and uninstalls.
AppId={{B8235A72-5E2C-43D0-8B56-D783C38D29D4}
AppName=Penny Pincher
AppVersion={#AppVersion}
AppVerName=Penny Pincher {#AppVersion}
AppPublisher=Penny Pincher
AppPublisherURL=https://github.com/petmar20/penny-pincher
AppSupportURL=https://github.com/petmar20/penny-pincher/issues
DefaultDirName={localappdata}\Programs\Penny Pincher
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=..\target\installer
OutputBaseFilename=penny-pincher-setup
SetupIconFile=..\assets\icon.ico
UninstallDisplayIcon={app}\penny-pincher.exe
UninstallDisplayName=Penny Pincher
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "..\target\release\penny-pincher.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Penny Pincher"; Filename: "{app}\penny-pincher.exe"
Name: "{autodesktop}\Penny Pincher"; Filename: "{app}\penny-pincher.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\penny-pincher.exe"; Description: "Open Penny Pincher"; Flags: nowait postinstall skipifsilent
