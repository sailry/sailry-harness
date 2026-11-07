; Per-user installer for the staged portable Sailry Desktop bundle.
; scripts/package/windows-installer.py supplies the defines below and compiles this script.
#ifndef AppVersion
  #error Compile with scripts/package/windows-installer.py
#endif

[Setup]
AppId={{2B5E4F50-B9F0-4AC4-AB04-50FA7C8CDC25}
AppName=Sailry
AppVersion={#AppVersion}
AppVerName=Sailry {#AppVersion}
AppPublisher=Sailry
VersionInfoVersion={#VersionCore}
; The in-app updater replaces the bundle in place, so it must stay in a user-writable folder.
PrivilegesRequired=lowest
DefaultDirName={autopf}\Sailry
DisableDirPage=yes
DisableProgramGroupPage=yes
DisableReadyPage=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
CloseApplications=yes
; The launch option starts Sailry once; restarting closed instances too would compete for its profile.
RestartApplications=no
SetupIconFile={#IconFile}
UninstallDisplayIcon={app}\app\sailry-desktop.exe
UninstallDisplayName=Sailry
WizardStyle=modern dynamic
WizardImageFile={#ArtDir}\wizard-*.png
WizardSmallImageFile={#ArtDir}\small-*.png
ShowLanguageDialog=auto
Compression=lzma2/max
SolidCompression=yes
#ifdef SignTool
SignTool={#SignTool}
SignedUninstaller=yes
#endif

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "es"; MessagesFile: "compiler:Languages\Spanish.isl"
Name: "fr"; MessagesFile: "compiler:Languages\French.isl"
Name: "ja"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "ko"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "ptbr"; MessagesFile: "compiler:Languages\BrazilianPortuguese.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "zhcn"; MessagesFile: "compiler:Languages\ChineseSimplified.isl"
Name: "zhtw"; MessagesFile: "compiler:Languages\ChineseTraditional.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[InstallDelete]
; Replace the whole bundle, as the updater does, so files from another version never remain.
Type: filesandordirs; Name: "{app}\app"

[Files]
Source: "{#Bundle}\*"; DestDir: "{app}\app"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Sailry"; Filename: "{app}\app\sailry-desktop.exe"
Name: "{autodesktop}\Sailry"; Filename: "{app}\app\sailry-desktop.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\app\sailry-desktop.exe"; Description: "{cm:LaunchProgram,Sailry}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Updated bundles contain files the uninstall log does not list, and the updater keeps
; recovery copies beside the bundle. The profile in ~/.sailry is outside {app}.
Type: filesandordirs; Name: "{app}\app"
Type: filesandordirs; Name: "{app}\.sailry-update-*"
