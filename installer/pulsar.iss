; Compiled by scripts\package.ps1, which passes AppVersion and SourceDir.
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef SourceDir
  #define SourceDir "..\dist\stage"
#endif
#define AppGuid "8F3B6C1E-3D8A-4B8E-9C61-5A2E7D4F0B19"
#define RunKey "Software\Microsoft\Windows\CurrentVersion\Run"

[Setup]
AppId={{{#AppGuid}}
AppName=Pulsar
AppVersion={#AppVersion}
AppVerName=Pulsar {#AppVersion}
AppPublisher=oMaN-Rod (简体中文版：JasonChow94)
AppPublisherURL=https://github.com/JasonChow94/Pulsar
AppSupportURL=https://github.com/JasonChow94/Pulsar/issues
AppUpdatesURL=https://github.com/JasonChow94/Pulsar/releases
VersionInfoVersion={#AppVersion}
DefaultDirName={localappdata}\Programs\Pulsar
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.22000
OutputDir=..\dist
OutputBaseFilename=pulsar-{#AppVersion}-setup
SetupIconFile=..\crates\pulsar-monitor\assets\pulsar.ico
UninstallDisplayIcon={app}\pulsar.exe
UninstallDisplayName=Pulsar
WizardStyle=modern
Compression=lzma2
SolidCompression=yes
CloseApplications=no

[Languages]
Name: "chinesesimplified"; MessagesFile: "ChineseSimplified.isl"

[Tasks]
Name: autostart; Description: "登录 Windows 时启动 Pulsar"; Check: not IsUpgrade

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Pulsar"; Filename: "{app}\pulsar.exe"

[Registry]
Root: HKCU; Subkey: "{#RunKey}"; ValueType: string; ValueName: "Pulsar"; ValueData: """{app}\pulsar.exe"""; Tasks: autostart

[Run]
Filename: "{app}\pulsar.exe"; Description: "立即启动 Pulsar"; Flags: nowait postinstall skipifsilent
Filename: "{app}\pulsar.exe"; Flags: nowait; Check: RestartAfterSilentUpgrade

[Code]
const
  WM_CLOSE = $0010;

  SensorsKey = 'SYSTEM\CurrentControlSet\Services\PulsarSensors';
  PawnIoKey = 'SYSTEM\CurrentControlSet\Services\PawnIO';
  PawnIoSite = 'https://pawnio.eu';
  PawnIoRepo = 'https://github.com/namazso/PawnIO';

var
  WasRunning: Boolean;
  SensorsCheck, PawnIoCheck: TNewCheckBox;

function IsUpgrade: Boolean;
begin
  Result := RegValueExists(HKCU,
    'Software\Microsoft\Windows\CurrentVersion\Uninstall\{' + '{#AppGuid}' + '}_is1',
    'UninstallString');
end;

function AppWindow: HWND;
begin
  Result := FindWindowByClassName('PulsarHost');
end;

function SettingsWindow: HWND;
begin
  Result := FindWindowByWindowName('Pulsar 设置');
end;

{ Asks both processes to close, so Settings saves pending edits, then forces them. }
procedure ClosePulsar;
var
  I, ResultCode: Integer;
begin
  if SettingsWindow <> 0 then
    PostMessage(SettingsWindow, WM_CLOSE, 0, 0);
  if AppWindow <> 0 then
    PostMessage(AppWindow, WM_CLOSE, 0, 0);
  for I := 1 to 50 do
  begin
    if (AppWindow = 0) and (SettingsWindow = 0) then
      Break;
    Sleep(100);
  end;
  Sleep(300);
  Exec(ExpandConstant('{sys}\taskkill.exe'), '/F /IM pulsar.exe /IM pulsar-settings.exe /IM pulsar-details.exe',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

function SensorsInstalled: Boolean;
begin
  Result := RegKeyExists(HKLM, SensorsKey);
end;

function PawnIoInstalled: Boolean;
begin
  Result := RegKeyExists(HKLM, PawnIoKey);
end;

procedure OpenLink(Sender: TObject);
var
  ResultCode: Integer;
begin
  ShellExec('open', TNewStaticText(Sender).Caption, '', '', SW_SHOWNORMAL, ewNoWait, ResultCode);
end;

procedure SensorsToggled(Sender: TObject);
begin
  PawnIoCheck.Enabled := SensorsCheck.Checked and not PawnIoInstalled;
end;

function AddLink(Page: TWizardPage; Url: String; Top: Integer): TNewStaticText;
begin
  Result := TNewStaticText.Create(Page);
  Result.Parent := Page.Surface;
  Result.Top := Top;
  Result.Caption := Url;
  Result.Cursor := crHand;
  Result.Font.Color := clBlue;
  Result.Font.Style := [fsUnderline];
  Result.OnClick := @OpenLink;
end;

{ The optional elevated part: everything else installs without administrator rights. }
procedure InitializeWizard;
var
  Page: TWizardPage;
  Intro: TNewStaticText;
  Link: TNewStaticText;
begin
  Page := CreateCustomPage(wpSelectTasks, 'CPU 温度（可选）',
    '需要管理员权限和 PawnIO 驱动。');

  Intro := TNewStaticText.Create(Page);
  Intro.Parent := Page.Surface;
  Intro.AutoSize := False;
  Intro.WordWrap := True;
  Intro.Width := Page.SurfaceWidth;
  Intro.Height := ScaleY(132);
  Intro.Caption :=
    'Pulsar 无需管理员权限即可运行，但 Windows 不允许这类程序读取 CPU 温度。' +
    '要显示 CPU 温度，Pulsar 还需要两个额外组件：' + #13#10 + #13#10 +
    '  -  PawnIO：由 namazso 开发的开源签名驱动，用于读取传感器。' + #13#10 +
    '  -  Pulsar 传感器服务：一个小型后台服务，向 PawnIO 读取温度，' +
    '并只把这个数值传给 Pulsar。' + #13#10 + #13#10 +
    'Windows 会请求管理员授权。不勾选时，Pulsar 不会安装任何驱动或服务，' +
    '其他功能完全不受影响。支持 Intel 和 AMD Ryzen CPU。之后可以重新运行本安装程序更改选择。';

  SensorsCheck := TNewCheckBox.Create(Page);
  SensorsCheck.Parent := Page.Surface;
  SensorsCheck.Top := Intro.Top + Intro.Height + ScaleY(8);
  SensorsCheck.Width := Page.SurfaceWidth;
  SensorsCheck.Caption := '安装 Pulsar 传感器服务';
  SensorsCheck.Checked := SensorsInstalled;
  SensorsCheck.OnClick := @SensorsToggled;

  PawnIoCheck := TNewCheckBox.Create(Page);
  PawnIoCheck.Parent := Page.Surface;
  PawnIoCheck.Top := SensorsCheck.Top + SensorsCheck.Height + ScaleY(4);
  PawnIoCheck.Width := Page.SurfaceWidth;
  if PawnIoInstalled then
    PawnIoCheck.Caption := '已安装 PawnIO'
  else
    PawnIoCheck.Caption := '同时安装 PawnIO（通过 winget 下载）';
  PawnIoCheck.Checked := not PawnIoInstalled;
  SensorsToggled(nil);

  Link := AddLink(Page, PawnIoSite, PawnIoCheck.Top + PawnIoCheck.Height + ScaleY(12));
  AddLink(Page, PawnIoRepo, Link.Top + Link.Height + ScaleY(4));
end;

function RunElevated(Filename, Params: String): Boolean;
var
  ResultCode: Integer;
begin
  Result := ShellExec('runas', Filename, Params, '', SW_HIDE, ewWaitUntilTerminated, ResultCode)
    and (ResultCode = 0);
end;

procedure InstallPawnIo;
var
  ResultCode: Integer;
begin
  if not ShellExec('', 'winget.exe',
    'install --id namazso.PawnIO --exact --accept-source-agreements --accept-package-agreements',
    '', SW_SHOWNORMAL, ewWaitUntilTerminated, ResultCode) or (ResultCode <> 0) then
  begin
    MsgBox('无法自动安装 PawnIO。即将打开其官网，请手动安装；' +
      '安装完成后即可显示 CPU 温度。',
      mbInformation, MB_OK);
    ShellExec('open', PawnIoSite, '', '', SW_SHOWNORMAL, ewNoWait, ResultCode);
  end;
end;

{ A silent upgrade never raises an administrator prompt; it leaves the service as it is. }
procedure SetUpSensors;
var
  Sensors: String;
begin
  if WizardSilent then
    Exit;
  Sensors := ExpandConstant('{app}\pulsar-sensors.exe');
  if SensorsCheck.Checked then
  begin
    if PawnIoCheck.Enabled and PawnIoCheck.Checked then
      InstallPawnIo;
    if not RunElevated(Sensors, '--install') then
      MsgBox('传感器服务未安装，CPU 温度将显示为短横线。' +
        '请重新运行本安装程序重试。', mbInformation, MB_OK);
  end
  else if SensorsInstalled then
    RunElevated(Sensors, '--uninstall');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SetUpSensors;
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  WasRunning := AppWindow <> 0;
  ClosePulsar;
  Result := '';
end;

function RestartAfterSilentUpgrade: Boolean;
begin
  Result := WizardSilent and WasRunning;
end;

function InitializeUninstall: Boolean;
begin
  ClosePulsar;
  Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  { While pulsar-sensors.exe is still there to remove its own service. }
  if (CurUninstallStep = usUninstall) and SensorsInstalled then
    RunElevated(ExpandConstant('{app}\pulsar-sensors.exe'), '--uninstall');
  if CurUninstallStep = usPostUninstall then
  begin
    RegDeleteValue(HKCU, '{#RunKey}', 'Pulsar');
    if not UninstallSilent and
      (MsgBox('是否同时删除 Pulsar 的设置和日志？', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES) then
    begin
      DelTree(ExpandConstant('{userappdata}\Pulsar'), True, True, True);
      DelTree(ExpandConstant('{localappdata}\Pulsar'), True, True, True);
    end;
  end;
end;
