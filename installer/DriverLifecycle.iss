[Code]
const
  DriverKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\ParsecVDD';
var
  RemoveDriver: Boolean;
  DriverRestart: Boolean;

function ReadDriverAt(Root: Integer; var Location, Uninstaller: String): Boolean;
var
  DisplayName, Command: String;
begin
  Result := False;
  if not RegQueryStringValue(Root, DriverKey, 'DisplayName', DisplayName) then Exit;
  if CompareText(DisplayName, 'Parsec Virtual Display Driver') <> 0 then Exit;
  RegQueryStringValue(Root, DriverKey, 'InstallLocation', Location);
  RegQueryStringValue(Root, DriverKey, 'UninstallString', Command);
  { The vendor registers a quoted executable without arguments. }
  Uninstaller := RemoveQuotes(Trim(Command));
  Result := True;
end;

function ReadDriver(var Location, Uninstaller: String): Boolean;
begin
  Location := '';
  Uninstaller := '';
  Result := ReadDriverAt(HKLM64, Location, Uninstaller);
  if not Result then Result := ReadDriverAt(HKLM32, Location, Uninstaller);
end;

function DriverUninstallerValid(Location, Uninstaller: String): Boolean;
begin
  Result := (Location <> '') and
    (CompareText(Uninstaller, AddBackslash(Location) + 'uninstall.exe') = 0) and
    FileExists(Uninstaller);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Location, Uninstaller: String;
  Code: Integer;
begin
  Result := '';
  if ReadDriver(Location, Uninstaller) and DriverUninstallerValid(Location, Uninstaller) then begin
    Log('Reusing installed Parsec VDD at ' + Location);
    Exit;
  end;
  WizardForm.StatusLabel.Caption := CustomMessage('DriverInstalling');
  ExtractTemporaryFile('parsec-vdd-0.45.0.0.exe');
  if not Exec(ExpandConstant('{tmp}\parsec-vdd-0.45.0.0.exe'), '/S', ExpandConstant('{tmp}'),
      SW_HIDE, ewWaitUntilTerminated, Code) then begin
    Result := FmtMessage(CustomMessage('DriverInstallFailed'), [SysErrorMessage(Code)]);
    Exit;
  end;
  Log('Parsec VDD install exit code: ' + IntToStr(Code));
  DriverRestart := (Code = 3010) or (Code = 1641);
  if (Code <> 0) and not DriverRestart then
    Result := FmtMessage(CustomMessage('DriverInstallFailed'), ['exit code ' + IntToStr(Code)])
  else if not ReadDriver(Location, Uninstaller) or not DriverUninstallerValid(Location, Uninstaller) then
    Result := FmtMessage(CustomMessage('DriverInstallFailed'), ['registration missing']);
end;

function NeedRestart(): Boolean;
begin
  Result := DriverRestart;
end;

function InitializeUninstall(): Boolean;
var
  Location, Uninstaller: String;
  Answer: Integer;
  I: Integer;
begin
  Result := True;
  RemoveDriver := False;
  if not ReadDriver(Location, Uninstaller) then Exit;
  { Silent uninstall also defaults to removing VDD. /KEEPVDD retains it. }
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/KEEPVDD') = 0 then Exit;
  Answer := SuppressibleMsgBox(CustomMessage('RemoveDriver'), mbConfirmation,
    MB_YESNOCANCEL or MB_DEFBUTTON1, IDYES);
  Result := Answer <> IDCANCEL;
  RemoveDriver := Answer = IDYES;
  Log('Remove Parsec VDD selected: ' + IntToStr(Ord(RemoveDriver)));
end;

procedure DriverRemovalFailed(Detail: String);
begin
  Log('Parsec VDD removal failed: ' + Detail);
  SuppressibleMsgBox(FmtMessage(CustomMessage('DriverRemoveFailed'), [Detail]),
    mbError, MB_OK, IDOK);
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Location, Uninstaller, TemporaryUninstaller: String;
  Code: Integer;
begin
  { Called only after the user confirms the actual application uninstall. }
  if (CurUninstallStep <> usUninstall) or not RemoveDriver then Exit;
  if not ReadDriver(Location, Uninstaller) then Exit;
  if not DriverUninstallerValid(Location, Uninstaller) then begin
    DriverRemovalFailed('registered uninstaller missing or unsupported');
    Exit;
  end;
  UninstallProgressForm.StatusLabel.Caption := CustomMessage('DriverRemoving');
  TemporaryUninstaller := ExpandConstant('{tmp}\parsec-vdd-uninstall.exe');
  { NSIS _?= prevents asynchronous self-copy. Our temporary copy runs with the
    registered installation directory, so Exec waits for actual removal. }
  if not FileCopy(Uninstaller, TemporaryUninstaller, False) then begin
    DriverRemovalFailed('copy failed');
    Exit;
  end;
  if not Exec(TemporaryUninstaller, '/S _?=' + Location, Location, SW_HIDE,
      ewWaitUntilTerminated, Code) then begin
    DriverRemovalFailed(SysErrorMessage(Code));
    Exit;
  end;
  Log('Parsec VDD uninstall exit code: ' + IntToStr(Code));
  DriverRestart := (Code = 3010) or (Code = 1641);
  if ((Code <> 0) and not DriverRestart) or ReadDriver(Location, Uninstaller) then
    DriverRemovalFailed('exit code ' + IntToStr(Code) + '; registry checked');
end;

function UninstallNeedRestart(): Boolean;
begin
  Result := DriverRestart;
end;
