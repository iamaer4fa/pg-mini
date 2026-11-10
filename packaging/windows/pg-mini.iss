#ifndef MyAppVersion
  #define MyAppVersion "0.1.10"
#endif

#define MyAppName "pg-mini"

[Setup]
AppId={{80E15521-53EC-4A42-8C70-833F11C06AE2}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher=pg-mini contributors
DefaultDirName={autopf}\pg-mini
DefaultGroupName=pg-mini
UninstallDisplayIcon={app}\pg-mini.exe
SetupIconFile=..\..\assets\pg-mini.ico
OutputDir=..\..\target\installer
OutputBaseFilename=pg-mini-setup-{#MyAppVersion}-x64
ArchitecturesAllowed=x64
ArchitecturesInstallIn64BitMode=x64
PrivilegesRequired=admin
WizardStyle=modern
Compression=lzma
SolidCompression=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"

[Files]
Source: "..\..\target\release\pg-mini.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\assets\pg-mini.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\target\vc_redist.x64.exe"; DestDir: "{tmp}"; Flags: deleteafterinstall

[Icons]
Name: "{group}\pg-mini"; Filename: "{app}\pg-mini.exe"; IconFilename: "{app}\pg-mini.ico"
Name: "{autodesktop}\pg-mini"; Filename: "{app}\pg-mini.exe"; IconFilename: "{app}\pg-mini.ico"; Tasks: desktopicon

[Run]
Filename: "{tmp}\vc_redist.x64.exe"; Parameters: "/install /quiet /norestart"; StatusMsg: "Installing Microsoft Visual C++ runtime..."; Flags: waituntilterminated; Check: not IsVCRedistInstalled
Filename: "{app}\pg-mini.exe"; Description: "Launch pg-mini"; Flags: postinstall nowait skipifsilent

[Code]
function IsVCRedistInstalled: Boolean;
var
  Installed: Cardinal;
begin
  Result :=
    RegQueryDWordValue(
      HKLM64,
      'SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64',
      'Installed',
      Installed
    ) and (Installed = 1);
end;
