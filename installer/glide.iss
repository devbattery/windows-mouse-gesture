; Glide for Windows 설치 프로그램 (Inno Setup 7.1.1 이상)
;
; tools\build-installer.ps1이 컴파일한다. 직접 컴파일하려면:
;   ISCC.exe /DAppVersion=<버전> /DPackageDir=<dist\glide-windows-버전 폴더> [/DSigned=1] installer\glide.iss
;
; 하는 일
; - C:\Program Files\Glide에 설치한다(관리자 권한 창 지원 조건 때문에 폴더는 바꿀 수 없다).
; - 관리자 권한 창 지원(기본): 보조 스크립트(glide-setup.ps1)가 이 PC 전용 인증서를 만들어 코어 하나에만 서명하고
;   개인키를 바로 지운다. 서명하지 못하면 관리자 창 미지원판으로 바꿔 설치를 마친다.
; - 로그인 시 자동 시작(기본, 모든 사용자), 모든 사용자 시작 메뉴 "Glide 설정", "앱 및 기능" 등록과 제거 프로그램.
; - 0.5.x의 스크립트 설치(install.cmd)를 발견하면 그 등록·인증서·바로 가기를 정리한다.
;
; 조용한 설치 예: GlideSetup-<버전>.exe /VERYSILENT /SUPPRESSMSGBOXES /NORESTART [/MERGETASKS="!uiaccess,!autostart"]
; 추가 명령줄 스위치: /NOLAUNCH(설치 뒤 Glide를 시작하지 않음), /SKIPSACCHECK(스마트 앱 컨트롤 확인을 건너뜀)

#if Ver < EncodeVer(7, 1, 1)
  #error Inno Setup 7.1.1 이상이 필요합니다.
#endif
#ifndef AppVersion
  #error AppVersion이 없습니다(/DAppVersion=x.y.z).
#endif
#ifndef PackageDir
  #error PackageDir가 없습니다(/DPackageDir=dist\glide-windows-x.y.z).
#endif
; 세 실행 파일이 모두 SignPath Foundation 인증서로 서명됐는지(릴리스 작업이 넘긴다). 스마트 앱 컨트롤 확인에 쓴다.
#ifndef Signed
  #define Signed "0"
#endif
#if Signed != "0" && Signed != "1"
  #error Signed는 0 또는 1이어야 합니다.
#endif

#define AppName "Glide for Windows"
#define RepoUrl "https://github.com/devbattery/windows-mouse-gesture"
; AppId GUID. 바꾸지 않는다. "앱 및 기능" 등록 키(Uninstall\{GUID}_is1)와 업데이트 감지가 이 값에 묶여 있다.
; tools\test-installer.ps1도 이 줄에서 값을 읽는다.
#define AppGuid "B2F694BE-4E4A-4DD1-BEBF-E1638E565570"

[Setup]
AppId={{{#AppGuid}}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=devbattery
AppPublisherURL={#RepoUrl}
AppSupportURL={#RepoUrl}/issues
AppUpdatesURL={#RepoUrl}/releases
AppCopyright=Copyright (c) 2026 devbattery
; uiAccess는 Program Files 같은 보안 위치의 실행 파일만 허용하므로 설치 폴더를 고정한다.
DefaultDirName={autopf}\Glide
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=admin
SetupArchitecture=x64
; Windows 10 22H2(19045) 이상
MinVersion=10.0.19045
; 실행 중인 Glide는 보조 스크립트가 경로를 확인해 직접 종료한다(Restart Manager 확인 창이 겹치지 않게 끈다).
CloseApplications=no
RestartApplications=no
SetupIconFile=..\settings\icons\icon.ico
UninstallDisplayIcon={app}\glide-core.exe
UninstallDisplayName={#AppName}
WizardStyle=modern
ShowLanguageDialog=auto
; 설치·제거 기록을 임시 폴더에 남긴다(문제를 알려 줄 때 첨부할 수 있게).
SetupLogging=yes
UninstallLogging=yes
OutputDir=..\dist
OutputBaseFilename=GlideSetup-{#AppVersion}
Compression=lzma2/max
SolidCompression=yes
; 설치 프로그램 서명은 아직 하지 않는다(docs/signing.md).
SignedUninstaller=no
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}
VersionInfoProductVersion={#AppVersion}
VersionInfoCompany=devbattery
VersionInfoDescription={#AppName} Setup

[Languages]
; 첫 항목이 기본 언어다(Windows 표시 언어가 둘 다 아니면 한국어).
Name: "korean"; MessagesFile: "compiler:Languages\Korean.isl"
Name: "english"; MessagesFile: "compiler:Default.isl"

[CustomMessages]
korean.TaskGroup=추가 설정:
korean.TaskUiAccess=관리자 권한 창에서도 제스처 사용(이 PC 전용 인증서를 만들어 Glide에만 서명)
korean.TaskAutoStart=로그인할 때 자동으로 시작(이 PC의 모든 사용자)
korean.LaunchGlide=Glide 시작
korean.SmartAppControlBlocked=이 PC는 스마트 앱 컨트롤이 켜져 있어 Glide를 실행할 수 없습니다. 아무것도 바꾸지 않고 설치를 멈췄습니다.%n%n스마트 앱 컨트롤은 공인 인증서로 서명된 앱만 실행하는데, 이 판은 아직 공식 코드 서명이 없습니다.%n%n- 지금 쓰려면: Windows 보안 → 앱 및 브라우저 컨트롤 → 스마트 앱 컨트롤 설정에서 끈 뒤 다시 설치합니다.%n- 끄지 않으려면: 공식 서명판이 나올 때까지 기다려 주세요.%n%n자세한 내용은 README의 "스마트 앱 컨트롤" 절을 보세요.
korean.RunningElsewhere=다른 위치에서 실행 중인 Glide가 있습니다. 트레이 메뉴에서 종료한 뒤 다시 설치해 주세요.%n%n%1
korean.StopFailed=실행 중인 Glide를 종료하지 못했습니다. 트레이 메뉴에서 종료한 뒤 다시 설치해 주세요.%n%n%1
korean.SignFallback=관리자 권한 창 지원판을 서명하지 못해 관리자 창 미지원판으로 설치했습니다. 관리자 권한 창(작업 관리자 등) 위에서는 제스처가 동작하지 않습니다.%n%n%1
korean.FallbackFailed=관리자 창 미지원판으로 바꾸지 못했습니다. Glide가 시작되지 않으면 "관리자 권한 창에서도 제스처 사용"을 끄고 다시 설치해 주세요.
korean.InfoFailed=설치 정보(install.json)를 기록하지 못했습니다. 제거할 때 이전 인증서가 남을 수 있습니다.%n%n%1
korean.RemoveCertFailed=신뢰 루트의 Glide 인증서를 지우지 못했습니다. certlm.msc의 "신뢰할 수 있는 루트 인증 기관"에서 "Glide Local UIAccess Signing" 인증서를 직접 지워 주세요.%n%n%1
english.TaskGroup=Additional options:
english.TaskUiAccess=Use gestures over administrator windows too (creates a certificate for this PC and signs only Glide)
english.TaskAutoStart=Start automatically at sign-in (all users of this PC)
english.LaunchGlide=Start Glide
english.SmartAppControlBlocked=Smart App Control is on, so Glide cannot run on this PC. Setup stopped without changing anything.%n%nSmart App Control only runs apps signed with a publicly trusted certificate, and this build does not have an official code signature yet.%n%n- To use Glide now, turn it off in Windows Security > App & browser control > Smart App Control settings, then run Setup again.%n- Otherwise, wait for an officially signed release.%n%nSee "Smart App Control" in the README for details.
english.RunningElsewhere=Glide is running from another location. Exit it from the tray menu, then run Setup again.%n%n%1
english.StopFailed=Could not stop the running Glide. Exit it from the tray menu, then run Setup again.%n%n%1
english.SignFallback=Could not sign the administrator-window build, so the build without administrator-window support was installed. Gestures will not work over administrator windows such as Task Manager.%n%n%1
english.FallbackFailed=Could not switch to the build without administrator-window support. If Glide does not start, run Setup again with "Use gestures over administrator windows too" turned off.
english.InfoFailed=Could not write the install information (install.json). An old certificate may be left behind when uninstalling.%n%n%1
english.RemoveCertFailed=Could not remove the Glide certificate from the trusted root store. Remove the "Glide Local UIAccess Signing" certificate from "Trusted Root Certification Authorities" in certlm.msc.%n%n%1

[Tasks]
Name: "uiaccess"; Description: "{cm:TaskUiAccess}"; GroupDescription: "{cm:TaskGroup}"
Name: "autostart"; Description: "{cm:TaskAutoStart}"; GroupDescription: "{cm:TaskGroup}"

[Files]
; 압축을 풀 때 앞쪽 파일이 빠르도록, 설치 전에 꺼내 쓰는 파일을 맨 앞에 둔다(SolidCompression).
Source: "glide-setup.ps1"; DestDir: "{app}"; Flags: ignoreversion
; 서명에 실패했을 때 코어를 바꿔 넣을 관리자 창 미지원판(설치하지 않고 임시 폴더에만 꺼낸다)
Source: "{#PackageDir}\bin\glide-core.exe"; DestName: "glide-core-fallback.exe"; Flags: dontcopy
; 같은 버전을 다시 설치해도 서명 전 원본으로 바꾸고 새로 서명하도록 ignoreversion을 쓴다.
Source: "{#PackageDir}\bin\glide-core-uiaccess.exe"; DestDir: "{app}"; DestName: "glide-core.exe"; Tasks: uiaccess; Flags: ignoreversion
Source: "{#PackageDir}\bin\glide-core.exe"; DestDir: "{app}"; Tasks: not uiaccess; Flags: ignoreversion
Source: "{#PackageDir}\bin\glide-settings.exe"; DestDir: "{app}"; Flags: ignoreversion
; 설정 창 "Glide 정보"의 라이선스 버튼이 여는 파일
Source: "{#PackageDir}\LICENSE.txt"; DestDir: "{app}"; Flags: ignoreversion
#ifexist AddBackslash(PackageDir) + "THIRD-PARTY-NOTICES.txt"
Source: "{#PackageDir}\THIRD-PARTY-NOTICES.txt"; DestDir: "{app}"; Flags: ignoreversion
#endif

[InstallDelete]
; 0.5.x 스크립트 설치가 설치 폴더에 두던 제거 스크립트
Type: files; Name: "{app}\uninstall.cmd"
Type: files; Name: "{app}\uninstall.ps1"
#ifnexist AddBackslash(PackageDir) + "THIRD-PARTY-NOTICES.txt"
; 고지 파일 없이 만든 설치 프로그램이면, 이전 설치의 고지가 지금 실행 파일과 어긋나지 않도록 지운다.
Type: files; Name: "{app}\THIRD-PARTY-NOTICES.txt"
#endif

[Icons]
Name: "{autoprograms}\Glide 설정"; Filename: "{app}\glide-settings.exe"; WorkingDir: "{app}"; Comment: "Glide 마우스 제스처 설정"

[Registry]
; 로그인 시 자동 시작(모든 사용자). 관리자 설치에서 HKCU에 쓰면 UAC에 입력한 다른 계정에 등록될 수 있어 HKLM을 쓴다.
Root: HKLM; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "Glide"; ValueData: """{app}\glide-core.exe"""; Tasks: autostart; Flags: uninsdeletevalue

[UninstallDelete]
; 보조 스크립트가 설치 중에 만드는 파일
Type: files; Name: "{app}\install.json"

[Run]
; postinstall 항목은 원래 사용자(관리자 권한이 아닌) 권한으로 실행된다. 그래야 일반 권한 + uiAccess로 시작된다.
Filename: "{app}\glide-core.exe"; Description: "{cm:LaunchGlide}"; Flags: postinstall nowait; Check: ShouldLaunch

[Code]
const
  HelperName = 'glide-setup.ps1';
  FallbackCoreName = 'glide-core-fallback.exe';
  RunKeyPath = 'Software\Microsoft\Windows\CurrentVersion\Run';
  // 0.5.x 스크립트 설치의 "앱 및 기능" 등록과, 이 설치 프로그램의 등록
  LegacyUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\Glide';
  InnoUninstallKey = 'Software\Microsoft\Windows\CurrentVersion\Uninstall\{{#AppGuid}}_is1';
  SmartAppControlKey = 'SYSTEM\CurrentControlSet\Control\CI\Policy';
  HelperOk = 0;
  HelperRunningElsewhere = 3;
  PublisherSigned = {#Signed};

var
  // 마지막으로 실행한 보조 스크립트의 출력(사용자 안내에 넣는다)
  HelperOutput: String;

// ── 명령줄 ──

// 명령줄에 /Name 스위치가 있는지(대소문자 구분 없음)
function HasSwitch(const Name: String): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(ParamStr(I), '/' + Name) = 0 then
    begin
      Result := True;
      Exit;
    end;
end;

// 명령줄에 /Name=... 인자가 있는지(대소문자 구분 없음)
function HasParamWithValue(const Name: String): Boolean;
var
  I: Integer;
begin
  Result := False;
  for I := 1 to ParamCount do
    if CompareText(Copy(ParamStr(I), 1, Length(Name) + 2), '/' + Name + '=') = 0 then
    begin
      Result := True;
      Exit;
    end;
end;

// [Run]의 "Glide 시작" 조건. CI 스모크 테스트는 /NOLAUNCH로 시작하지 않게 한다.
function ShouldLaunch: Boolean;
begin
  Result := not HasSwitch('NOLAUNCH');
end;

// CI 스모크 테스트 전용: 서명 실패 때의 대체 경로를 시험한다. GitHub Actions 환경 변수가 함께 있을 때만 동작한다.
function SimulateSignFailure: Boolean;
begin
  Result := HasSwitch('TESTSIGNFAILURE') and (GetEnv('GITHUB_ACTIONS') = 'true');
end;

// ── 보조 스크립트 실행 ──

// 64비트 설치 모드에서 {sys}는 64비트 System32다.
function PowerShellPath: String;
begin
  Result := ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe');
end;

// glide-setup.ps1을 숨김 창으로 실행하고 종료 코드를 돌려준다(시작하지 못하면 -1).
// 스크립트가 쓴 진행 내용은 설치 로그(/LOG, SetupLogging)로 옮기고 HelperOutput에 남긴다.
// AsOriginalUser는 설치 때만 쓸 수 있다(ExecAsOriginalUser는 제거 때 지원되지 않음).
function RunHelper(const Script, Arguments: String; const AsOriginalUser: Boolean): Integer;
var
  LogFile, Params: String;
  Lines: TArrayOfString;
  I, ResultCode: Integer;
  Started: Boolean;
begin
  LogFile := GenerateUniqueName(ExpandConstant('{tmp}'), '.log');
  Params := '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "' + Script + '" ' + Arguments +
    ' -InstallDir "' + ExpandConstant('{app}') + '" -LogPath "' + LogFile + '"';
  Log('보조 스크립트 실행: ' + Arguments);
  // 작업 폴더를 임시 폴더로 둔다(설치 폴더를 붙잡아 지우지 못하는 일이 없게).
  if AsOriginalUser then
    Started := ExecAsOriginalUser(PowerShellPath, Params, ExpandConstant('{tmp}'), SW_HIDE, ewWaitUntilTerminated, ResultCode)
  else
    Started := Exec(PowerShellPath, Params, ExpandConstant('{tmp}'), SW_HIDE, ewWaitUntilTerminated, ResultCode);
  if not Started then
  begin
    Log('보조 스크립트를 시작하지 못했습니다: ' + SysErrorMessage(ResultCode));
    ResultCode := -1;
  end;
  HelperOutput := '';
  if LoadStringsFromFile(LogFile, Lines) then
    for I := 0 to GetArrayLength(Lines) - 1 do
    begin
      Log('  ' + Lines[I]);
      HelperOutput := HelperOutput + Lines[I] + #13#10;
    end;
  DeleteFile(LogFile);
  Log('보조 스크립트 종료 코드: ' + IntToStr(ResultCode));
  Result := ResultCode;
end;

function InstalledHelper: String;
begin
  Result := ExpandConstant('{app}\') + HelperName;
end;

// ── 설치 전 확인 ──

// 스마트 앱 컨트롤(Windows 11)이 차단 모드인지. 값: 0 끔, 1 켬(차단), 2 평가 모드(차단하지 않음).
// 차단 모드에서는 이 PC 전용 인증서를 인정하지 않고 서명 없는 설정 창도 막으므로, 서명 전 판은 실행되지 않는다.
// Windows 10처럼 값이 없으면 꺼진 것으로 본다.
function SmartAppControlEnforced: Boolean;
var
  State: Cardinal;
begin
  Result := RegQueryDWordValue(HKLM, SmartAppControlKey, 'VerifiedAndReputablePolicyState', State) and (State = 1);
end;

// 스마트 앱 컨트롤이 실행을 막을 판이면 아무것도 바꾸기 전에 멈춘다.
// 그대로 진행하면 인증서·파일·자동 시작을 다 등록한 뒤 마지막 시작 단계에서야 실패한다.
function InitializeSetup: Boolean;
begin
  Result := True;
  if (PublisherSigned = 0) and (not HasSwitch('SKIPSACCHECK')) and SmartAppControlEnforced then
  begin
    Log('스마트 앱 컨트롤이 켜져 있어 설치를 멈춥니다.');
    SuppressibleMsgBox(CustomMessage('SmartAppControlBlocked'), mbCriticalError, MB_OK, IDOK);
    Result := False;
  end;
end;

// 0.5.x 스크립트 설치에서 처음 넘어올 때는 그때 고른 설정(-NoUiAccess, -NoAutoStart)을 기본값으로 이어받는다.
// 예: 자체 서명 uiAccess가 거부되는 PC에서 -NoUiAccess로 설치했다면, 다시 켜면 Glide가 시작되지 않는다.
// 이 설치 프로그램의 기록이 있으면 UsePreviousTasks가 지난 선택을 쓰고, 명령줄로 작업을 정했으면 그대로 둔다.
procedure InitializeWizard;
var
  Info: AnsiString;
  Text: String;
begin
  if not RegKeyExists(HKLM, LegacyUninstallKey) or RegKeyExists(HKLM, InnoUninstallKey) then
    Exit;
  if HasParamWithValue('TASKS') or HasParamWithValue('MERGETASKS') then
    Exit;
  // install.json(PowerShell ConvertTo-Json): "uiAccess":  false
  if LoadStringFromFile(ExpandConstant('{autopf}\Glide\install.json'), Info) then
  begin
    Text := Lowercase(String(Info));
    StringChangeEx(Text, ' ', '', True);
    if Pos('"uiaccess":false', Text) > 0 then
    begin
      Log('이전 스크립트 설치의 선택을 이어받음: 관리자 창 미지원판');
      WizardSelectTasks('!uiaccess');
    end;
  end;
  // 스크립트 설치는 자동 시작을 현재 사용자(HKCU) Run에 등록했다. 없으면 -NoAutoStart로 설치한 것이다.
  if not RegValueExists(HKCU, RunKeyPath, 'Glide') then
  begin
    Log('이전 스크립트 설치의 선택을 이어받음: 자동 시작 안 함');
    WizardSelectTasks('!autostart');
  end;
end;

// 파일을 바꾸기 전에 설치 폴더에서 실행 중인 Glide를 종료한다. 다른 위치에서 실행 중이면 준비 화면에서 멈춘다.
function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  Code: Integer;
begin
  Result := '';
  try
    ExtractTemporaryFile(HelperName);
    // 서명에 실패했을 때 쓸 파일. 설치 중이 아니라 지금 꺼내 두어, 꺼내지 못하는 문제를 설치 전에 알게 한다.
    if WizardIsTaskSelected('uiaccess') then
      ExtractTemporaryFile(FallbackCoreName);
  except
    Result := GetExceptionMessage;
    Exit;
  end;
  Code := RunHelper(ExpandConstant('{tmp}\') + HelperName, '-Action Stop', False);
  if Code = HelperRunningElsewhere then
    Result := FmtMessage(CustomMessage('RunningElsewhere'), [Trim(HelperOutput)])
  else if Code <> HelperOk then
    Result := FmtMessage(CustomMessage('StopFailed'), [Trim(HelperOutput)]);
end;

// ── 설치 후 ──

// 관리자 권한 창 지원: 코어 서명. 이 단계는 파일 복사가 끝난 뒤라 되돌릴 수 없으므로,
// 서명하지 못하면 관리자 창 미지원판으로 바꿔 설치를 마친다(보조 스크립트가 이번에 만든 인증서는 이미 지웠다).
procedure ApplyUiAccess;
var
  SignOutput: String;
begin
  if WizardIsTaskSelected('uiaccess') then
  begin
    if SimulateSignFailure then
      HelperOutput := '(CI 시험: 서명 실패를 흉내 냄)'
    else if RunHelper(InstalledHelper, '-Action Sign -Version "{#AppVersion}"', False) = HelperOk then
      Exit;
    SignOutput := Trim(HelperOutput);
    Log('서명하지 못해 관리자 창 미지원판으로 바꿉니다.');
    if not CopyFile(ExpandConstant('{tmp}\') + FallbackCoreName, ExpandConstant('{app}\glide-core.exe'), False) then
    begin
      Log('관리자 창 미지원판으로 바꾸지 못했습니다.');
      SuppressibleMsgBox(CustomMessage('FallbackFailed'), mbError, MB_OK, IDOK);
      Exit;
    end;
    if RunHelper(InstalledHelper, '-Action WriteInfo -Version "{#AppVersion}"', False) <> HelperOk then
      Log('설치 정보를 기록하지 못했습니다.');
    SuppressibleMsgBox(FmtMessage(CustomMessage('SignFallback'), [SignOutput]), mbError, MB_OK, IDOK);
  end
  else if RunHelper(InstalledHelper, '-Action WriteInfo -Version "{#AppVersion}"', False) <> HelperOk then
    SuppressibleMsgBox(FmtMessage(CustomMessage('InfoFailed'), [Trim(HelperOutput)]), mbError, MB_OK, IDOK);
end;

// 다시 설치하면서 자동 시작을 끈 경우 이전 설치가 등록한 값을 지운다([Registry] 항목은 선택했을 때만 쓴다).
procedure ApplyAutoStart;
begin
  if not WizardIsTaskSelected('autostart') then
    if RegValueExists(HKLM, RunKeyPath, 'Glide') then
    begin
      Log('자동 시작 등록 해제');
      RegDeleteValue(HKLM, RunKeyPath, 'Glide');
    end;
end;

// 0.5.x 스크립트 설치 정리: "앱 및 기능"의 이전 등록(같은 앱이 두 줄로 보이지 않게),
// 그리고 현재 사용자 영역의 자동 시작·바로 가기(두 번 시작되지 않게, 원래 사용자 권한으로 실행).
// 이전 인증서는 Sign·WriteInfo가 install.json의 지문으로 지운다.
procedure CleanLegacyInstall;
begin
  if RegKeyExists(HKLM, LegacyUninstallKey) then
  begin
    Log('이전 스크립트 설치의 "앱 및 기능" 등록 제거');
    RegDeleteKeyIncludingSubkeys(HKLM, LegacyUninstallKey);
  end;
  if RunHelper(InstalledHelper, '-Action CleanLegacyUser', True) <> HelperOk then
    Log('현재 사용자 영역의 이전 설치 정리에 실패했습니다(설치는 계속).');
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
  begin
    ApplyUiAccess;
    ApplyAutoStart;
    CleanLegacyInstall;
  end;
end;

// ── 제거 ──

// 파일을 지우기 전에 Glide를 종료하고, 설치 정보에 적힌 인증서를 신뢰 루트에서 지운다.
// 자동 시작 값·바로 가기·"앱 및 기능" 등록·설치 폴더는 제거 프로그램이 지운다. 설정(%APPDATA%\Glide)은 남긴다.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if (CurUninstallStep = usUninstall) and FileExists(InstalledHelper) then
  begin
    if RunHelper(InstalledHelper, '-Action Stop -IgnoreOtherLocations', False) <> HelperOk then
      Log('실행 중인 Glide를 종료하지 못했습니다.');
    if RunHelper(InstalledHelper, '-Action RemoveCert', False) <> HelperOk then
      SuppressibleMsgBox(FmtMessage(CustomMessage('RemoveCertFailed'), [Trim(HelperOutput)]), mbError, MB_OK, IDOK);
  end;
end;
