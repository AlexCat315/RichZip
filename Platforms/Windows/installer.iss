; MugenZip per-user installer. Built on Windows with Inno Setup 6.
#ifndef AppVersion
  #define AppVersion "0.3.0"
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
Name: "startup"; Description: "登录时启动桌面悬浮球"; GroupDescription: "启动:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\MugenZip"; Filename: "{app}\MugenZip.exe"
Name: "{group}\MugenZip 悬浮球"; Filename: "{app}\MugenZip.exe"; Parameters: "--widget"
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
; The file-menu cascade. Every command is the same `--verb` entry point the
; Linux action files use, so Windows and Linux share one implementation.
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "MugenZip"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\MugenZip.exe,0"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\ExtendedSubCommandsKey"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\extract-here"; ValueType: string; ValueName: "MUIVerb"; ValueData: "解压到当前目录"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\extract-here\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb extract-here ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\extract-folder"; ValueType: string; ValueName: "MUIVerb"; ValueData: "解压到同名目录"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\extract-folder\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb extract-folder ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\smart-extract"; ValueType: string; ValueName: "MUIVerb"; ValueData: "智能解压到同名目录"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\smart-extract\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb smart-extract ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\open"; ValueType: string; ValueName: "MUIVerb"; ValueData: "在 MugenZip 打开"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb open ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\compress-zip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 ZIP"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\compress-zip\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-zip ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\compress-7z"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 7Z"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\compress-7z\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-7z ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\compress-ui"; ValueType: string; ValueName: "MUIVerb"; ValueData: "在 MugenZip 中压缩"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\compress-ui\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-ui ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash"; ValueType: string; ValueName: "MUIVerb"; ValueData: "校验哈希"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\ExtendedSubCommandsKey"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\crc32"; ValueType: string; ValueName: "MUIVerb"; ValueData: "CRC32"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\crc32\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-crc32 ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\md5"; ValueType: string; ValueName: "MUIVerb"; ValueData: "MD5"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\md5\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-md5 ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\sha1"; ValueType: string; ValueName: "MUIVerb"; ValueData: "SHA-1"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\sha1\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-sha1 ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\sha256"; ValueType: string; ValueName: "MUIVerb"; ValueData: "SHA-256"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\sha256\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-sha256 ""%1"""
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\sha512"; ValueType: string; ValueName: "MUIVerb"; ValueData: "SHA-512"
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip\shell\hash\shell\sha512\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-sha512 ""%1"""
; Folders get the compression verbs directly: there is no archive to extract.
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompressZip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 ZIP"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompressZip\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-zip ""%1"""
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompress7z"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 7Z"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompress7z\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-7z ""%1"""
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompressUi"; ValueType: string; ValueName: "MUIVerb"; ValueData: "在 MugenZip 中压缩"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompressUi\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-ui ""%1"""
; One login entry for the dot. Turning the switch off in Settings removes this
; value, and turning it back on restores it.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "MugenZipWidget"; ValueData: """{app}\MugenZip.exe"" --widget"; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\MugenZip.exe"; Description: "启动 MugenZip"; Flags: nowait postinstall skipifsilent
