#define MyAppName      "iooffbx"
#define MyAppVersion   "0.3.0"
#define MyAppPublisher "Reenallarealo"
#define MyAppURL       "https://github.com/Reenallarealo/iooffbx"
#define MyAppExeName   "iooffbx.exe"
#define MyAppDate      "2026/3/30"

[Setup]
AppId={{A3F2C8D1-5B6E-4A0F-9C3D-7E8B2F1A4D56}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}
AppUpdatesURL={#MyAppURL}
AppCopyright=Copyright (C) 2026 {#MyAppPublisher}
DefaultDirName={autopf}\{#MyAppName}
DefaultGroupName={#MyAppName}
AllowNoIcons=yes
OutputDir=..\user_release\v0.3.0
OutputBaseFilename=iooffbx_setup_v0.3.0
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
ArchitecturesInstallIn64BitMode=x64compatible
ArchitecturesAllowed=x64compatible
MinVersion=10.0
SetupIconFile=
; 安装完成后可直接运行
UninstallDisplayName={#MyAppName} {#MyAppVersion}
UninstallDisplayIcon={app}\{#MyAppExeName}
VersionInfoVersion=0.3.0
VersionInfoCompany={#MyAppPublisher}
VersionInfoDescription=将 BrickLink Studio .io 文件转换为 .fbx 文件
VersionInfoCopyright=Copyright (C) 2026 {#MyAppPublisher}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "addtopath"; Description: "将 iooffbx 添加到系统 PATH（推荐，便于命令行调用）"; GroupDescription: "附加任务:"; Flags: checkedonce

[Files]
Source: "..\target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"
Name: "{group}\卸载 {#MyAppName}"; Filename: "{uninstallexe}"

[Registry]
; 添加到 PATH
Root: HKLM; Subkey: "SYSTEM\CurrentControlSet\Control\Session Manager\Environment"; \
    ValueType: expandsz; ValueName: "Path"; \
    ValueData: "{olddata};{app}"; \
    Check: NeedsAddPath(ExpandConstant('{app}')); \
    Tasks: addtopath; Flags: preservestringtype

[Code]
function NeedsAddPath(Param: string): boolean;
var
  OrigPath: string;
begin
  if not RegQueryStringValue(HKEY_LOCAL_MACHINE,
    'SYSTEM\CurrentControlSet\Control\Session Manager\Environment',
    'Path', OrigPath)
  then begin
    Result := True;
    exit;
  end;
  Result := Pos(';' + Param + ';', ';' + OrigPath + ';') = 0;
end;

[Run]
Filename: "{app}\{#MyAppExeName}"; Description: "查看版本信息（可选）"; \
    Parameters: "--version"; Flags: postinstall skipifsilent nowait shellexec

[UninstallRun]
Filename: "{cmd}"; Parameters: "/C setx PATH ""%PATH:{app};=%"" /M"; \
    RunOnceId: "RemoveFromPath"; Flags: runhidden; Tasks: addtopath
