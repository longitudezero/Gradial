#ifndef SourceDir
  #error SourceDir must point to the staged Gradial release directory.
#endif
#ifndef OutputDir
  #error OutputDir must point to the release artifact directory.
#endif
#ifndef AppVersion
  #define AppVersion "0.3.0-beta.1"
#endif
#ifndef VersionInfoVersion
  #define VersionInfoVersion "0.3.0.1"
#endif

[Setup]
AppId={{FB307C83-7B0F-4F0E-8CA7-7F984BCD246D}
AppName=Gradial
AppVersion={#AppVersion}
AppVerName=Gradial {#AppVersion}
AppPublisher=longitudezero
AppPublisherURL=https://github.com/longitudezero/Gradial
AppSupportURL=https://github.com/longitudezero/Gradial/issues
AppUpdatesURL=https://github.com/longitudezero/Gradial/releases
DefaultDirName={localappdata}\Programs\Gradial
DefaultGroupName=Gradial
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayIcon={app}\gradial.exe
SetupIconFile=..\crates\gradial-app\resources\gradial.ico
LicenseFile=..\..\LICENSE
InfoBeforeFile=..\..\Documentation\beta-installer-notice.txt
OutputDir={#OutputDir}
OutputBaseFilename=Gradial-{#AppVersion}-Windows-x86_64-Setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no
VersionInfoVersion={#VersionInfoVersion}
VersionInfoCompany=Gradial contributors
VersionInfoDescription=Gradial public beta installer
VersionInfoProductName=Gradial
VersionInfoProductVersion={#VersionInfoVersion}
VersionInfoCopyright=Copyright (c) 2026 longitudezero

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Icons]
Name: "{group}\Gradial"; Filename: "{app}\gradial.exe"
Name: "{group}\Uninstall Gradial"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Gradial"; Filename: "{app}\gradial.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\gradial.exe"; Description: "Launch Gradial"; Flags: nowait postinstall skipifsilent
