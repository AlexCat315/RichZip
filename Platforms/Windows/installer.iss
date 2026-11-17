; MugenZip per-user installer. Built on Windows with Inno Setup 6.
#ifndef AppVersion
  #define AppVersion "0.3.2"
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
; Remove the old ProgID-only cascade and the three flat folder verbs on upgrade.
; A `*` menu and a `Directory` cascade below replace them, so an archive file
; must not show two MugenZip submenus.
Root: HKCU; Subkey: "Software\Classes\MugenZip.Archive\shell\MugenZip"; ValueType: none; Flags: deletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompressZip"; ValueType: none; Flags: deletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompress7z"; ValueType: none; Flags: deletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZipCompressUi"; ValueType: none; Flags: deletekey

; The canonical file menu lives on `*`, so it appears for every file — including
; installers and unknown extensions — not only for extensions associated with
; MugenZip.Archive. `MultiSelectModel=Player` plus `%*` hands the whole Explorer
; selection to one process, so a multi-select produces one archive/report.
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "MugenZip"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\MugenZip.exe,0"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\ExtendedSubCommandsKey"; Flags: uninsdeletekey

Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\extract-here"; ValueType: string; ValueName: "MUIVerb"; ValueData: "解压到当前目录"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\extract-here"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\extract-here\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb extract-here %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\extract-folder"; ValueType: string; ValueName: "MUIVerb"; ValueData: "解压到同名目录"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\extract-folder"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\extract-folder\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb extract-folder %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\smart-extract"; ValueType: string; ValueName: "MUIVerb"; ValueData: "智能解压到同名目录"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\smart-extract"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\smart-extract\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb smart-extract %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\open"; ValueType: string; ValueName: "MUIVerb"; ValueData: "在 MugenZip 打开"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\open"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Single"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb open ""%1"""
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\reveal"; ValueType: string; ValueName: "MUIVerb"; ValueData: "在文件夹中显示"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\reveal"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Single"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\reveal\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb reveal ""%1"""
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-zip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 ZIP"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-zip"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-zip\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-zip %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-7z"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 7Z"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-7z"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-7z\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-7z %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-targz"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 TAR.GZ"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-targz"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-targz\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-targz %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-ui"; ValueType: string; ValueName: "MUIVerb"; ValueData: "在 MugenZip 中压缩"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-ui"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\compress-ui\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-ui %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash"; ValueType: string; ValueName: "MUIVerb"; ValueData: "校验哈希"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\ExtendedSubCommandsKey"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\crc32"; ValueType: string; ValueName: "MUIVerb"; ValueData: "CRC32"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\crc32"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\crc32\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-crc32 %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\md5"; ValueType: string; ValueName: "MUIVerb"; ValueData: "MD5"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\md5"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\md5\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-md5 %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha1"; ValueType: string; ValueName: "MUIVerb"; ValueData: "SHA-1"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha1"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha1\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-sha1 %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha256"; ValueType: string; ValueName: "MUIVerb"; ValueData: "SHA-256"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha256"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha256\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-sha256 %*"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha512"; ValueType: string; ValueName: "MUIVerb"; ValueData: "SHA-512"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha512"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\*\shell\MugenZip\shell\hash\shell\sha512\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb hash-sha512 %*"

; Folders are not archives, so they get compression verbs only. The same
; cascade shape keeps the submenu consistent with the file menu.
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "MugenZip"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\MugenZip.exe,0"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\ExtendedSubCommandsKey"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-zip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 ZIP"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-zip"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-zip\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-zip %*"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-7z"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 7Z"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-7z"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-7z\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-7z %*"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-targz"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩至 TAR.GZ"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-targz"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-targz\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-targz %*"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-ui"; ValueType: string; ValueName: "MUIVerb"; ValueData: "在 MugenZip 中压缩"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-ui"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"
Root: HKCU; Subkey: "Software\Classes\Directory\shell\MugenZip\shell\compress-ui\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-ui %*"

; Empty space inside a folder is a different intent from right-clicking the
; folder itself: archive the *contents* into an archive inside this folder.
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "MugenZip"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\MugenZip.exe,0"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Single"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\ExtendedSubCommandsKey"; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-zip"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩当前文件夹至 ZIP"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-zip"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Single"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-zip\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-current-zip ""%V"""
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-7z"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩当前文件夹至 7Z"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-7z"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Single"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-7z\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-current-7z ""%V"""
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-targz"; ValueType: string; ValueName: "MUIVerb"; ValueData: "压缩当前文件夹至 TAR.GZ"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-targz"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Single"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-current-targz\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-current-targz ""%V"""
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-ui"; ValueType: string; ValueName: "MUIVerb"; ValueData: "在 MugenZip 中压缩"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-ui"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Single"
Root: HKCU; Subkey: "Software\Classes\Directory\Background\shell\MugenZip\shell\compress-ui\command"; ValueType: string; ValueName: ""; ValueData: """{app}\MugenZip.exe"" --verb compress-ui ""%V"""
; One login entry for the dot. Turning the switch off in Settings removes this
; value, and turning it back on restores it.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "MugenZipWidget"; ValueData: """{app}\MugenZip.exe"" --widget"; Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\MugenZip.exe"; Description: "启动 MugenZip"; Flags: nowait postinstall skipifsilent
