<#
.SYNOPSIS
  CI 러너에 Inno Setup 7(설치 프로그램 컴파일러)을 설치하고 ISCC.exe 경로를 INNO_SETUP_ISCC로 알린다(CI 전용).

.DESCRIPTION
  GitHub 러너 이미지에는 Inno Setup 6만 있어 고정한 판을 받아 설치한다. ci.yml과 release.yml이 같은 판을 쓰도록
  URL과 SHA-256을 이 파일 한 곳에만 둔다. SHA-256은 공식 GitHub Release(jrsoftware/issrc)의 자산 다이제스트,
  winget 매니페스트(JRSoftware.InnoSetup.7)와 같은 값이다. 판을 올릴 때는 두 곳의 값을 대조해 함께 바꾼다.

  설치 위치는 러너 임시 폴더이고, 현재 사용자 설치(/CURRENTUSER)라 시스템 설정을 바꾸지 않는다.
  로컬 PC에서는 winget install --id JRSoftware.InnoSetup.7 -e 로 설치한다.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\install-inno-setup.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or -not $env:RUNNER_TEMP -or -not $env:GITHUB_ENV) {
    Write-Host '이 스크립트는 GitHub Actions에서만 실행합니다. 로컬 PC에서는 winget install --id JRSoftware.InnoSetup.7 -e 를 쓰세요.'
    exit 1
}

$url = 'https://github.com/jrsoftware/issrc/releases/download/is-7_1_1/innosetup-7.1.1-x64.exe'
$sha256 = '8a5a6711c3766a6e5fb74f280ba0e3551e719ce94538d901fdbd1f8f9920b3f0'

$installer = Join-Path $env:RUNNER_TEMP 'innosetup-7.1.1-x64.exe'
# Windows PowerShell 5.1의 진행 표시는 내려받기를 크게 느리게 한다. GitHub는 TLS 1.2 이상만 받는다.
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
Invoke-WebRequest -Uri $url -OutFile $installer -UseBasicParsing
$hash = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash.ToLowerInvariant()
if ($hash -ne $sha256) { throw "Inno Setup 설치 파일의 SHA-256이 다릅니다: $hash" }

$dir = Join-Path $env:RUNNER_TEMP 'inno-setup-7'
$process = Start-Process -FilePath $installer -Wait -PassThru -ArgumentList @(
    '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', '/SP-', '/CURRENTUSER', "/DIR=`"$dir`"")
if ($process.ExitCode -ne 0) { throw "Inno Setup 설치 실패(종료 코드 $($process.ExitCode))" }
$iscc = Join-Path $dir 'ISCC.exe'
if (-not (Test-Path -LiteralPath $iscc)) { throw "ISCC.exe를 찾지 못했습니다($iscc)." }

# 다음 단계부터 build-installer.ps1이 이 경로를 쓴다. GITHUB_ENV는 BOM 없는 UTF-8이어야 한다.
[IO.File]::AppendAllText($env:GITHUB_ENV, "INNO_SETUP_ISCC=$iscc`n", (New-Object Text.UTF8Encoding $false))
Write-Host "Inno Setup 7.1.1: $iscc"
