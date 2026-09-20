#define MyAppName "Virtual Display Workspace"
#define MyAppVersion "0.1.3"
#define MyAppExeName "virtual-display-workspace.exe"

[Setup]
AppId={{B1A4A5F0-7C95-4B0F-9E94-5C9BB7E0E1B4}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
DefaultDirName={autopf}\Virtual Display Workspace
DefaultGroupName={#MyAppName}
OutputBaseFilename=VirtualDisplayWorkspace-Setup
OutputDir=..\target\installer
Compression=lzma2
SolidCompression=yes
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible and not arm64
MinVersion=10.0.19044
PrivilegesRequired=admin
SetupIconFile=..\assets\app-icon.ico
UninstallDisplayIcon={app}\{#MyAppExeName}
WizardStyle=modern
SetupLogging=yes
CloseApplications=yes
RestartApplications=no
AppMutex=Local\VirtualDisplayWorkspace.App

[Languages]
Name: "japanese"; MessagesFile: "compiler:Languages\Japanese.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
japanese.RemoveDriver=Parsec Virtual Display Driver も削除しますか？%n%n通常は「はい」を選択してください。Parsecや他のアプリで引き続き使う場合は「いいえ」で残せます。%n%n「キャンセル」でアプリのアンインストールも中止します。
english.RemoveDriver=Also remove Parsec Virtual Display Driver?%n%nYes is the default. Choose No to keep the shared driver for Parsec or other applications.%n%nCancel stops the application uninstall as well.
japanese.DriverInstalling=Parsec Virtual Display Driver をインストールしています...
english.DriverInstalling=Installing Parsec Virtual Display Driver...
japanese.DriverInstallFailed=Parsec VDDのインストールに失敗しました（%1）。セットアップを完了できません。
english.DriverInstallFailed=Parsec VDD installation failed (%1). Setup cannot continue.
japanese.DriverRemoving=Parsec Virtual Display Driver を削除しています...
english.DriverRemoving=Removing Parsec Virtual Display Driver...
japanese.DriverRemoveFailed=Parsec VDDを削除できませんでした（%1）。アプリの削除は続行します。ドライバーはWindowsの「インストールされているアプリ」から削除できます。
english.DriverRemoveFailed=Could not remove Parsec VDD (%1). The application will still be removed. You can remove the driver later from Windows Installed apps.

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion
; Embed only the signed vendor setup, not the third-party manager or fonts.
Source: "..\driver\parsec-vdd\parsec-vdd-0.45.0.0.exe"; Flags: dontcopy

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{autodesktop}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "{cm:LaunchProgram,{#MyAppName}}"; Flags: nowait postinstall skipifsilent runasoriginaluser

#include "DriverLifecycle.iss"
