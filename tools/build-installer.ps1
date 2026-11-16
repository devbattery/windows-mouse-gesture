<#
.SYNOPSIS
  tools\package.ps1이 만든 dist\glide-windows-<버전>\ 폴더로 설치 프로그램과 SHA256SUMS.txt를 만든다.

.DESCRIPTION
  - dist\GlideSetup-<버전>.exe: installer\glide.iss를 Inno Setup 7(ISCC.exe)로 컴파일한 설치 프로그램
  - dist\SHA256SUMS.txt: 설치 프로그램의 SHA-256. `sha256sum -c`와 같은 형식("<해시>  <파일 이름>")

  Inno Setup 7.1.1 이상이 필요하다(버전은 glide.iss가 컴파일할 때 확인한다). ISCC.exe를 찾는 순서:
  -IsccPath → 환경 변수 INNO_SETUP_ISCC → %LOCALAPPDATA%\Programs\Inno Setup 7(winget 사용자 설치) → Program Files.
  이 스크립트는 아무것도 설치하지 않는다. 없으면 설치 방법만 알려 주고 멈춘다.

  CI는 package.ps1과 이 스크립트 사이에 실행 파일 코드 서명을 넣는다.

.PARAMETER IsccPath
  ISCC.exe 경로
.PARAMETER Signed
  세 실행 파일이 SignPath Foundation 인증서로 서명된 경우(릴리스 작업). 서명을 확인한 뒤, 설치 프로그램의
  스마트 앱 컨트롤 사전 확인(서명 전 판을 막는 안내)을 끈다.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\build-installer.ps1
#>
[CmdletBinding()]
param(
    [string] $IsccPath,
    [switch] $Signed
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$dist = Join-Path $root 'dist'
$package = Join-Path $dist "glide-windows-$version"
$inCi = $env:GITHUB_ACTIONS -eq 'true'

# ── 패키지 확인 ──
foreach ($file in 'bin\glide-core.exe', 'bin\glide-core-uiaccess.exe', 'bin\glide-settings.exe', 'LICENSE.txt') {
    if (-not (Test-Path -LiteralPath (Join-Path $package $file))) { throw "$package 에 $file 이 없습니다. 먼저 tools\package.ps1을 실행해 주세요." }
}
# 배포용 설치 프로그램에는 의존 라이브러리 라이선스 고지(cargo-about)가 반드시 들어가야 한다.
if (-not (Test-Path -LiteralPath (Join-Path $package 'THIRD-PARTY-NOTICES.txt'))) {
    if ($inCi) { throw 'THIRD-PARTY-NOTICES.txt가 없습니다(cargo-about 필요).' }
    Write-Warning 'THIRD-PARTY-NOTICES.txt가 없어 고지 없이 만듭니다(이 PC에 설치할 때만 쓰고 배포하지 마세요).'
}
# 서명판이라고 표시하기 전에 실제 서명을 확인한다(서명 전 판에서 스마트 앱 컨트롤 안내가 빠지지 않게).
if ($Signed) {
    foreach ($name in 'glide-core.exe', 'glide-core-uiaccess.exe', 'glide-settings.exe') {
        $signature = Get-AuthenticodeSignature -LiteralPath (Join-Path $package "bin\$name")
        if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notlike '*SignPath Foundation*') {
            throw "$name 이 SignPath Foundation 인증서로 서명돼 있지 않습니다(상태: $($signature.Status))."
        }
    }
}

# ── ISCC 찾기 ──
# 직접 지정한 경로(-IsccPath, INNO_SETUP_ISCC)는 다른 곳으로 넘어가지 않는다. CI가 확인한 판 대신
# 러너에 있던 다른 컴파일러로 빌드하는 일이 없게 한다.
$explicit = if ($IsccPath) { $IsccPath } elseif ($env:INNO_SETUP_ISCC) { $env:INNO_SETUP_ISCC } else { $null }
if ($explicit) {
    if (-not (Test-Path -LiteralPath $explicit -PathType Leaf)) { throw "지정한 ISCC.exe가 없습니다: $explicit" }
    $iscc = $explicit
} else {
    $iscc = @(
        $(if ($env:LOCALAPPDATA) { Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 7\ISCC.exe' })
        $(if ($env:ProgramFiles) { Join-Path $env:ProgramFiles 'Inno Setup 7\ISCC.exe' })
        $(if (${env:ProgramFiles(x86)}) { Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 7\ISCC.exe' })
    ) | Where-Object { $_ -and (Test-Path -LiteralPath $_ -PathType Leaf) } | Select-Object -First 1
}
if (-not $iscc) {
    Write-Host @'
Inno Setup 7(ISCC.exe)을 찾지 못했습니다. 다음으로 설치한 뒤 다시 실행해 주세요(관리자 권한 없이 현재 사용자에게 설치됩니다).
  winget install --id JRSoftware.InnoSetup.7 -e
다른 위치에 설치했다면 -IsccPath 또는 환경 변수 INNO_SETUP_ISCC로 ISCC.exe 경로를 알려 주세요.
'@
    exit 1
}

# ── 컴파일 ──
$setupName = "GlideSetup-$version.exe"
$setup = Join-Path $dist $setupName
$sums = Join-Path $dist 'SHA256SUMS.txt'
# 컴파일이 실패했을 때 이전 실행의 결과(다른 파일의 해시)가 남지 않게 먼저 지운다.
foreach ($old in $setup, $sums) { if (Test-Path -LiteralPath $old) { Remove-Item -LiteralPath $old } }
$arguments = @(
    "/DAppVersion=$version"
    "/DPackageDir=$((Resolve-Path -LiteralPath $package).Path)"
    "/DSigned=$(if ($Signed) { '1' } else { '0' })"
    "/O$dist"
    (Join-Path $root 'installer\glide.iss')
)
Write-Host "> $iscc $($arguments -join ' ')"
& $iscc @arguments
if ($LASTEXITCODE -ne 0) { throw "ISCC 실패(종료 코드 $LASTEXITCODE)" }
if (-not (Test-Path -LiteralPath $setup)) { throw "$setup 이 만들어지지 않았습니다." }

$hash = (Get-FileHash -LiteralPath $setup -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText($sums, "$hash  $setupName`n", [Text.Encoding]::ASCII)

Write-Host "설치 프로그램: $setup"
Write-Host "SHA-256: $hash"
