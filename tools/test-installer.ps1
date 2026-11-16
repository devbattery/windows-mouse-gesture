<#
.SYNOPSIS
  설치 프로그램(dist\GlideSetup-<버전>.exe)으로 실제로 설치·다시 설치·제거를 해 보고 결과를 확인한다(CI 전용).

.DESCRIPTION
  이 PC의 Program Files, 신뢰 루트 인증서, HKLM 레지스트리를 바꾸므로 GitHub Actions(GITHUB_ACTIONS=true)에서만
  실행한다. 러너는 관리자 권한이고 UAC 확인 창이 없다. 모든 설치는 /NOLAUNCH로 Glide를 시작하지 않는다.

  A. 0.5.x 스크립트 설치(install.cmd, 기본 선택)에서 넘어오기
     1. 그 상태를 흉내 낸다: "앱 및 기능"의 Uninstall\Glide 키, HKCU 자동 시작, 사용자 시작 메뉴 바로 가기,
        설치 폴더의 uninstall.cmd, 신뢰 루트의 Glide 인증서와 그 지문을 적은 install.json
     2. 기본 설치: 이전 설치가 정리되고, 코어가 새 인증서로 서명되고, 인증서가 정확히 1개인지
     3. 같은 버전을 다시 설치: 인증서가 새 것 1개로 바뀌는지
     4. 제거: 설치 폴더·등록·인증서·자동 시작·바로 가기가 모두 사라지는지
  B. 0.5.x 스크립트 설치(-NoUiAccess -NoAutoStart)에서 넘어오기
     1. 조용한 설치가 그때의 선택(관리자 창 미지원판, 자동 시작 안 함)을 이어받는지
     2. /TASKS로 둘 다 켜면 기본 설치와 같아지는지
     3. 서명 실패(/TESTSIGNFAILURE, CI에서만 동작): 관리자 창 미지원판으로 바꿔 설치를 마치고 인증서가 남지 않는지
     4. /MERGETASKS로 둘 다 끄기: 인증서 0개, 자동 시작 없음
     5. 다시 둘 다 켠 뒤 제거

.PARAMETER Signed
  실행 파일이 SignPath Foundation 인증서로 서명된 설치 프로그램(릴리스 작업). 이때는 이 PC 전용 인증서를 만들지
  않는 것이 정상이다.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\test-installer.ps1
#>
[CmdletBinding()]
param(
    [string] $Setup,
    [switch] $Signed
)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true') {
    Write-Host '이 스크립트는 시스템을 바꾸므로 GitHub Actions에서만 실행합니다.'
    exit 1
}
$root = Split-Path -Parent $PSScriptRoot
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $Setup) { $Setup = Join-Path $root "dist\GlideSetup-$version.exe" }
if (-not (Test-Path -LiteralPath $Setup)) { throw "$Setup 이 없습니다. 먼저 tools\build-installer.ps1을 실행해 주세요." }
# "앱 및 기능" 등록 키 이름은 installer\glide.iss의 AppGuid에서 읽는다(값을 두 곳에 적지 않게).
$appGuid = (Select-String -Path (Join-Path $root 'installer\glide.iss') -Pattern '^#define AppGuid "([0-9A-Fa-f-]{36})"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
if (-not $appGuid) { throw 'installer\glide.iss에서 AppGuid를 찾지 못했습니다.' }

$InstallDir = Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Glide'
$Core = Join-Path $InstallDir 'glide-core.exe'
$InfoPath = Join-Path $InstallDir 'install.json'
$CertSubjectPrefix = 'CN=Glide Local UIAccess Signing'
$UninstallKey = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\{$appGuid}_is1"
$LegacyUninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Glide'
$MachineRunKey = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Run'
$UserRunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$CommonShortcut = Join-Path ([Environment]::GetFolderPath('CommonPrograms')) 'Glide 설정.lnk'
$UserShortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Glide 설정.lnk'
$LogDir = Join-Path $root 'dist\installer-test-logs'
New-Item -ItemType Directory -Force -Path $LogDir | Out-Null

function Assert-That([bool] $Condition, [string] $Text) {
    if (-not $Condition) { throw "실패: $Text" }
    Write-Host "  확인: $Text"
}

function Get-GlideRootCertificates {
    @(Get-ChildItem -LiteralPath 'Cert:\LocalMachine\Root' | Where-Object { $_.Subject.StartsWith($CertSubjectPrefix) })
}

function Get-RunValue([string] $Key) {
    (Get-ItemProperty -Path $Key -Name 'Glide' -ErrorAction SilentlyContinue).Glide
}

function Read-Info { Get-Content -Raw -LiteralPath $InfoPath | ConvertFrom-Json }

# 설치 프로그램 실행. 실패하면 설치 로그를 보여 준다.
function Invoke-Setup([string] $Name, [string[]] $Extra) {
    Write-Host "== $Name"
    $log = Join-Path $LogDir "$Name.log"
    $arguments = @('/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/NOLAUNCH', "/LOG=`"$log`"") + $Extra
    $process = Start-Process -FilePath $Setup -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) {
        if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log | Write-Host }
        throw "$Name 실패(종료 코드 $($process.ExitCode))"
    }
}

# 0.5.x 스크립트 설치가 남기는 상태를 만든다. 관리자 창 지원판이면 신뢰 루트 인증서와 그 지문도 남긴다.
function New-LegacyInstall([bool] $UiAccess, [bool] $AutoStart) {
    Write-Host "== 0.5.x 스크립트 설치 상태 만들기(uiAccess=$UiAccess, autoStart=$AutoStart)"
    Assert-That (-not (Test-Path -LiteralPath $InstallDir)) '시작 전 설치 폴더 없음'
    Assert-That ((Get-GlideRootCertificates).Count -eq 0) '시작 전 Glide 인증서 없음'
    $thumbprint = $null
    if ($UiAccess) {
        $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject "$CertSubjectPrefix (legacy test)" `
            -CertStoreLocation 'Cert:\LocalMachine\My' -KeyAlgorithm RSA -KeyLength 2048 -HashAlgorithm SHA256
        $cer = Join-Path $env:RUNNER_TEMP 'glide-legacy.cer'
        Export-Certificate -Cert $cert -FilePath $cer | Out-Null
        Import-Certificate -FilePath $cer -CertStoreLocation 'Cert:\LocalMachine\Root' | Out-Null
        Remove-Item -LiteralPath "Cert:\LocalMachine\My\$($cert.Thumbprint)" -DeleteKey
        $thumbprint = $cert.Thumbprint
    }
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    [ordered]@{ version = '0.5.7'; uiAccess = $UiAccess; signedByPublisher = $false; certificateThumbprint = $thumbprint } |
        ConvertTo-Json | Set-Content -LiteralPath $InfoPath -Encoding UTF8
    Set-Content -LiteralPath (Join-Path $InstallDir 'uninstall.cmd') -Value '@echo off' -Encoding ASCII
    New-Item -Path $LegacyUninstallKey -Force | Out-Null
    New-ItemProperty -Path $LegacyUninstallKey -Name 'DisplayName' -Value 'Glide for Windows' -PropertyType String -Force | Out-Null
    if ($AutoStart) { Set-ItemProperty -Path $UserRunKey -Name 'Glide' -Value "`"$Core`"" }
    # 지워지는지만 보면 되므로 빈 파일로 만든다. WScript.Shell은 시스템 ANSI 코드 페이지에 없는 글자(영어 러너의 한글)가
    # 들어간 바로 가기를 저장하지 못한다.
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $UserShortcut) | Out-Null
    [IO.File]::WriteAllBytes($UserShortcut, [byte[]]@())
    return $thumbprint
}

# 이전 스크립트 설치의 흔적이 정리됐는지
function Assert-LegacyCleaned([string] $LegacyThumbprint) {
    if ($LegacyThumbprint) {
        Assert-That (-not (Test-Path -LiteralPath "Cert:\LocalMachine\Root\$LegacyThumbprint")) '이전 설치의 인증서 제거'
    }
    Assert-That (-not (Test-Path -LiteralPath $LegacyUninstallKey)) '이전 "앱 및 기능" 등록 제거'
    Assert-That (-not (Get-RunValue $UserRunKey)) '이전 자동 시작(HKCU Run) 없음'
    Assert-That (-not (Test-Path -LiteralPath $UserShortcut)) '이전 사용자 시작 메뉴 바로 가기 제거'
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $InstallDir 'uninstall.cmd'))) '이전 제거 스크립트 삭제'
}

# 모든 설치에 공통인 확인
function Assert-CommonInstall {
    foreach ($file in 'glide-core.exe', 'glide-settings.exe', 'glide-setup.ps1', 'LICENSE.txt', 'THIRD-PARTY-NOTICES.txt', 'install.json') {
        Assert-That (Test-Path -LiteralPath (Join-Path $InstallDir $file)) "설치 폴더에 $file"
    }
    Assert-That (-not (Test-Path -LiteralPath (Join-Path $InstallDir 'glide-core-fallback.exe'))) '대체용 코어는 설치 폴더에 없음'
    Assert-That (Test-Path -LiteralPath $CommonShortcut) '모든 사용자 시작 메뉴 바로 가기'
    $uninstall = Get-ItemProperty -LiteralPath $UninstallKey -ErrorAction SilentlyContinue
    Assert-That ($uninstall -and $uninstall.DisplayVersion -eq $version) "앱 및 기능 등록(버전 $version)"
    Assert-That (-not (Get-Process -Name 'glide-core' -ErrorAction SilentlyContinue)) 'Glide를 시작하지 않음(/NOLAUNCH)'
    $mine = @(Get-ChildItem -LiteralPath 'Cert:\LocalMachine\My' | Where-Object { $_.Subject.StartsWith($CertSubjectPrefix) })
    Assert-That ($mine.Count -eq 0) '개인키가 있는 인증서가 남지 않음(LocalMachine\My)'
}

# 관리자 창 지원 + 자동 시작 설치 확인. 새로 만든 인증서의 지문을 돌려준다(정식 서명판이면 없음).
function Assert-DefaultInstall {
    Assert-CommonInstall
    $info = Read-Info
    Assert-That ($info.uiAccess -eq $true -and $info.version -eq $version) "install.json(uiAccess=true, version=$version)"
    Assert-That ((Get-RunValue $MachineRunKey) -eq "`"$Core`"") '자동 시작(HKLM Run)'
    $signature = Get-AuthenticodeSignature -LiteralPath $Core
    Assert-That ($signature.Status -eq 'Valid') "코어 서명 Valid($($signature.Status))"
    $roots = Get-GlideRootCertificates
    if ($Signed) {
        Assert-That ($signature.SignerCertificate.Subject -like '*SignPath Foundation*') '코어 서명자가 SignPath Foundation'
        Assert-That ($info.signedByPublisher -eq $true -and -not $info.certificateThumbprint) 'install.json(정식 서명, 지문 없음)'
        Assert-That ($roots.Count -eq 0) '이 PC 전용 인증서를 만들지 않음'
        return $null
    }
    Assert-That ($signature.SignerCertificate.Subject.StartsWith($CertSubjectPrefix)) '코어 서명자가 이 PC 전용 인증서'
    Assert-That ($signature.SignerCertificate.Thumbprint -eq $info.certificateThumbprint) 'install.json 지문 = 코어 서명 지문'
    Assert-That ($roots.Count -eq 1 -and $roots[0].Thumbprint -eq $info.certificateThumbprint) '신뢰 루트의 Glide 인증서 1개(지금 것)'
    return $info.certificateThumbprint
}

# 관리자 창 미지원판 설치 확인(자동 시작 여부는 따로 받는다)
function Assert-NoUiAccessInstall([bool] $AutoStart) {
    Assert-CommonInstall
    $info = Read-Info
    Assert-That ($info.uiAccess -eq $false -and -not $info.certificateThumbprint) 'install.json(uiAccess=false, 지문 없음)'
    # 미지원판 코어는 서명 전 원본이다(정식 서명판이면 SignPath 서명).
    $expected = if ($Signed) { 'Valid' } else { 'NotSigned' }
    Assert-That ((Get-AuthenticodeSignature -LiteralPath $Core).Status -eq $expected) "미지원판 코어 서명 상태 $expected"
    Assert-That ((Get-GlideRootCertificates).Count -eq 0) '신뢰 루트의 Glide 인증서 0개'
    if ($AutoStart) {
        Assert-That ((Get-RunValue $MachineRunKey) -eq "`"$Core`"") '자동 시작(HKLM Run)'
    } else {
        Assert-That (-not (Get-RunValue $MachineRunKey)) '자동 시작 없음'
    }
}

# 제거하고 모두 정리됐는지 확인
function Invoke-UninstallAndAssert([string] $Name) {
    Write-Host "== $Name"
    $uninstaller = Join-Path $InstallDir 'unins000.exe'
    Assert-That (Test-Path -LiteralPath $uninstaller) '제거 프로그램(unins000.exe)'
    $log = Join-Path $LogDir "$Name.log"
    $process = Start-Process -FilePath $uninstaller -ArgumentList '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', "/LOG=`"$log`"" -Wait -PassThru
    Assert-That ($process.ExitCode -eq 0) "제거 프로그램 종료 코드 0($($process.ExitCode))"
    # 제거 프로그램은 임시 폴더의 사본으로 다시 실행되므로, 정리가 끝날 때까지 기다린다.
    $deadline = (Get-Date).AddSeconds(120)
    while (((Test-Path -LiteralPath $InstallDir) -or (Test-Path -LiteralPath $UninstallKey)) -and (Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
    }
    Assert-That (-not (Test-Path -LiteralPath $InstallDir)) '설치 폴더 삭제'
    Assert-That (-not (Test-Path -LiteralPath $UninstallKey)) '앱 및 기능 등록 제거'
    Assert-That ((Get-GlideRootCertificates).Count -eq 0) '신뢰 루트의 Glide 인증서 제거'
    Assert-That (-not (Get-RunValue $MachineRunKey)) '자동 시작 제거'
    Assert-That (-not (Test-Path -LiteralPath $CommonShortcut)) '시작 메뉴 바로 가기 제거'
}

# ── A. 0.5.x 기본 설치에서 넘어오기 ──
$legacy = New-LegacyInstall -UiAccess $true -AutoStart $true
Invoke-Setup 'a1-install-from-legacy' @()
$first = Assert-DefaultInstall
Assert-LegacyCleaned $legacy

Invoke-Setup 'a2-reinstall' @()
$second = Assert-DefaultInstall
if (-not $Signed) { Assert-That ($second -ne $first) '다시 설치하면 인증서가 새 것으로 바뀜' }

Invoke-UninstallAndAssert 'a3-uninstall'

# ── B. 0.5.x -NoUiAccess -NoAutoStart 설치에서 넘어오기 ──
$null = New-LegacyInstall -UiAccess $false -AutoStart $false
Invoke-Setup 'b1-install-from-legacy-choices' @()
Assert-NoUiAccessInstall -AutoStart $false
Assert-LegacyCleaned $null

Invoke-Setup 'b2-tasks-on' @('/TASKS="uiaccess,autostart"')
$null = Assert-DefaultInstall

# 서명 실패 대체 경로. 이전 설치(b2)의 인증서도 지워져야 한다.
Invoke-Setup 'b3-sign-failure' @('/TESTSIGNFAILURE')
Assert-NoUiAccessInstall -AutoStart $true

Invoke-Setup 'b4-tasks-off' @('/MERGETASKS="!uiaccess,!autostart"')
Assert-NoUiAccessInstall -AutoStart $false

# 조용한 설치는 이전 선택을 이어받으므로 기본 선택을 명시한다.
Invoke-Setup 'b5-tasks-on' @('/TASKS="uiaccess,autostart"')
$null = Assert-DefaultInstall
Invoke-UninstallAndAssert 'b6-uninstall'

Write-Host '설치 프로그램 확인을 모두 마쳤습니다.'
