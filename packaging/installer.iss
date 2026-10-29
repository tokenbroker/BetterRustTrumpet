; Better Rust Trumpet setup (D57), after the original's installer.iss.
; Built by .github/workflows/release.yml:
;   ISCC /DVersion=0.1.0-alpha.4 /DNumVersion=0.1.0.0 /DSourceDir=<exes> /O<out> installer.iss
;
; Differences from the original, so both apps can be installed side by side:
; its own AppId, folder, Start menu entry and startup value (the one the app's
; own "Run at startup" switch uses), and it closes only this app (by its
; hidden window), never by the shared exe name BetterTrumpet.exe.

#ifndef Version
  #define Version "0.0.0-dev"
#endif
#ifndef NumVersion
  #define NumVersion "0.0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\target\release"
#endif

#define AppName "Better Rust Trumpet"
#define RepoUrl "https://github.com/tokenbroker/BetterRustTrumpet"
; The app's hidden window title (ui::HIDDEN_TITLE) and its quit code
; (ui::APP_QUIT, posted as WM_APP + 1).
#define HiddenTitle "BetterTrumpet Rust"
#define AppQuit 8
#define RunValue "BetterTrumpet Rust Preview"

[Setup]
AppId={{4A0AB8A6-D8B7-4365-AADB-82565A5263F8}
AppName={#AppName}
AppVersion={#Version}
AppVerName={#AppName} {#Version}
AppPublisher=tokenbroker
AppPublisherURL={#RepoUrl}
AppSupportURL={#RepoUrl}/issues
AppUpdatesURL={#RepoUrl}/releases
DefaultDirName={autopf}\BetterRustTrumpet
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
UninstallDisplayIcon={app}\BetterTrumpet.exe
UninstallDisplayName={#AppName}
OutputBaseFilename=BetterRustTrumpet-{#Version}-setup
Compression=lzma2/ultra64
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\crates\bt-app\icons\icon.ico
WizardStyle=modern
; Per user by default, as the original (no admin prompt), which also lets the
; app's signed updater (D56) replace its own files.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
LicenseFile=..\LICENSE
VersionInfoVersion={#NumVersion}
VersionInfoCompany=tokenbroker
VersionInfoDescription={#AppName} setup
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#NumVersion}
MinVersion=10.0.17134
ChangesEnvironment=yes
; A fallback for files still in use: the Restart Manager closes only the
; programs using files in {app}.
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "french"; MessagesFile: "compiler:Languages\French.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "startup"; Description: "{cm:StartupTask}"; GroupDescription: "{cm:OptionsGroup}"
Name: "addtopath"; Description: "{cm:AddToPathTask}"; GroupDescription: "{cm:OptionsGroup}"; Flags: checkedonce

[CustomMessages]
english.StartupTask=Launch Better Rust Trumpet at Windows startup
french.StartupTask=Lancer Better Rust Trumpet au demarrage de Windows
english.AddToPathTask=Add the "bt" command-line tool to PATH (control volume, devices and presets from any terminal, e.g. "bt --help")
french.AddToPathTask=Ajouter l'outil en ligne de commande "bt" au PATH (controler volume, peripheriques et presets depuis n'importe quel terminal, ex. "bt --help")
english.OptionsGroup=Options:
french.OptionsGroup=Options :
english.LaunchAfterInstall=Launch Better Rust Trumpet
french.LaunchAfterInstall=Lancer Better Rust Trumpet

[Files]
Source: "{#SourceDir}\BetterTrumpet.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#SourceDir}\bt.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\THIRD_PARTY_NOTICES.md"; DestDir: "{app}"; Flags: ignoreversion

[InstallDelete]
; What an interrupted self-update may have left.
Type: files; Name: "{app}\*.old"
Type: files; Name: "{app}\*.new"

[UninstallDelete]
Type: files; Name: "{app}\*.old"
Type: files; Name: "{app}\*.new"
Type: files; Name: "{app}\.update-write-test"

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\BetterTrumpet.exe"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\BetterTrumpet.exe"; Tasks: desktopicon

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#RunValue}"; ValueData: """{app}\BetterTrumpet.exe"""; Flags: uninsdeletevalue; Tasks: startup
Root: HKCU; Subkey: "Environment"; ValueType: expandsz; ValueName: "Path"; ValueData: "{olddata};{app}"; Check: NeedsAddPath(ExpandConstant('{app}')); Tasks: addtopath

[Run]
Filename: "{app}\BetterTrumpet.exe"; Description: "{cm:LaunchAfterInstall}"; Flags: nowait postinstall skipifsilent
Filename: "{app}\BetterTrumpet.exe"; Flags: nowait postinstall skipifnotsilent

[Code]
// Asks the running app to quit through its hidden window and waits up to
// five seconds. Only Better Rust Trumpet has this window; the C#
// BetterTrumpet (same exe name) is never touched.
procedure CloseRunningApp();
var
  Wnd: HWND;
  Waited: Integer;
begin
  Wnd := FindWindowByWindowName('{#HiddenTitle}');
  if Wnd = 0 then
    exit;
  PostMessage(Wnd, $8001, {#AppQuit}, 0);
  Waited := 0;
  while (FindWindowByWindowName('{#HiddenTitle}') <> 0) and (Waited < 5000) do
  begin
    Sleep(100);
    Waited := Waited + 100;
  end;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  CloseRunningApp();
  Result := '';
end;

// Whether a directory is already in the user PATH.
function NeedsAddPath(Param: string): boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
  begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Uppercase(Param) + ';', ';' + Uppercase(OrigPath) + ';') = 0;
end;

// Takes {app} back out of the user PATH.
procedure RemoveFromPath(Dir: string);
var
  OrigPath, Upper, Needle: string;
  P: Integer;
begin
  if not RegQueryStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath) then
    exit;
  OrigPath := ';' + OrigPath + ';';
  Upper := Uppercase(OrigPath);
  Needle := ';' + Uppercase(Dir) + ';';
  P := Pos(Needle, Upper);
  if P = 0 then
    exit;
  Delete(OrigPath, P, Length(Needle) - 1);
  // Trim the separators added above.
  if Copy(OrigPath, 1, 1) = ';' then
    Delete(OrigPath, 1, 1);
  if Copy(OrigPath, Length(OrigPath), 1) = ';' then
    Delete(OrigPath, Length(OrigPath), 1);
  RegWriteExpandStringValue(HKEY_CURRENT_USER, 'Environment', 'Path', OrigPath);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  RunData: string;
begin
  if CurUninstallStep = usUninstall then
  begin
    CloseRunningApp();
    RemoveFromPath(ExpandConstant('{app}'));
    // The app's own startup switch writes the same value; remove it when it
    // points into this install. Settings in %LOCALAPPDATA%\BetterTrumpet-Rust
    // stay, as the original keeps its settings.
    if RegQueryStringValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#RunValue}', RunData) then
      if Pos(Uppercase(ExpandConstant('{app}')), Uppercase(RunData)) > 0 then
        RegDeleteValue(HKEY_CURRENT_USER, 'Software\Microsoft\Windows\CurrentVersion\Run', '{#RunValue}');
  end;
end;
