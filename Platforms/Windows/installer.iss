; MugenZip per-user installer. Built on Windows with Inno Setup 6.
#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\..\target\release-app\MugenZip"
#endif
#ifndef OutputDir
  #define OutputDir "..\..\dist"
#endif

[Setup]
AppId={{7D5D6F2A-3B4C-4E71-9A8F-6C2B1E5D4A70}
AppName=MugenZip
AppVersion={#AppVersion}
AppPublisher=AlexCat315
AppPublisherURL=https://github.com/AlexCat315/RichZip
AppSupportURL=https://github.com/AlexCat315/RichZip/issues
AppUpdatesURL=https://github.com/AlexCat315/RichZip/releases
DefaultDirName={localappdata}\Programs\MugenZip
DefaultGroupName=MugenZip
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir={#OutputDir}
OutputBaseFilename=MugenZip-{#AppVersion}-windows-x64-setup
SetupIconFile=..\..\assets\logo_box.ico
UninstallDisplayIcon={app}\MugenZip.exe
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
ChangesAssociations=yes
VersionInfoVersion={#AppVersion}
LicenseFile=..\..\LICENSE

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "创建桌面快捷方式"; GroupDescription: "快捷方式:"; Flags: unchecked
Name: "associate"; Description: "把常见压缩包关联到 MugenZip"; GroupDescription: "文件关联:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\MugenZip"; Filename: "{app}\MugenZip.exe"
Name: "{group}\卸载 MugenZip"; Filename: "{uninstallexe}"
Name: "{autodesktop}\MugenZip"; Filename: "{app}\MugenZip.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Classes\.zip"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.7z"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.rar"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.tar"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.gz"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.tgz"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.bz2"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.tbz2"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.xz"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\.txz"; ValueType: string; ValueName: ""; ValueData: "MugenZip.Archive"; Flags: uninsdeletevalue; Tasks: associate
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive"; ValueType: string; ValueName: ""; ValueData: "MugenZip Archive"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: "{app}\MugenZip.exe,0"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" ""%1"""

[Run]
Filename: "{app}\MugenZip.exe"; Description: "启动 MugenZip"; Flags: nowait postinstall skipifsilent
