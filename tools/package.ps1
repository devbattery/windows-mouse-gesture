<#
.SYNOPSIS
  Glide for Windows 설치 프로그램에 넣을 파일(dist\glide-windows-<버전>\)을 만든다.

.DESCRIPTION
  1. 일반 빌드: glide-core.exe(관리자 창 미지원판), glide-settings.exe
  2. uiAccess 빌드: glide-core-uiaccess.exe(관리자 창 지원판, 설치 때 서명)
  3. LICENSE와 의존 라이브러리 라이선스 고지를 함께 두고, uiAccess 매니페스트가 실제로 들어갔는지 mt.exe로 확인한다.

  이 스크립트는 빌드와 파일 복사만 한다. 설치 프로그램(dist\GlideSetup-<버전>.exe)과 SHA256SUMS.txt는
  tools\build-installer.ps1이 만든다(그 사이에 CI가 코드 서명을 넣을 수 있다). 인증서·서명·Program Files 설치는
  설치 프로그램(installer\glide.iss, installer\glide-setup.ps1)이 한다.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\package.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# Cargo.toml의 버전을 패키지 이름에 쓴다.
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value

function Invoke-Cargo([string[]] $Arguments) {
    Write-Host "> cargo $($Arguments -join ' ')"
    & cargo @Arguments
    if ($LASTEXITCODE -ne 0) { throw "cargo $($Arguments -join ' ') 실패(종료 코드 $LASTEXITCODE)" }
}

# 빌드 도구 확인. 없으면 설치 방법만 알려 주고 멈춘다(이 스크립트는 아무것도 설치하지 않는다).
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    Write-Host @'
Rust(cargo)를 찾지 못했습니다. 소스에서 빌드하려면 다음을 설치한 뒤 새 터미널에서 다시 실행해 주세요.
  winget install --id Rustlang.Rustup
  winget install --id Microsoft.VisualStudio.2022.BuildTools --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
빌드 없이 설치만 하려면 GitHub Releases의 GlideSetup-<버전>.exe를 받아 실행하면 됩니다.
'@
    exit 1
}

# --locked: Cargo.lock에 기록된 의존성 버전 그대로 빌드한다(누가 빌드해도 같은 의존성).
Invoke-Cargo @('build', '--release', '--locked', '--workspace')
# 같은 target 폴더에서 기능만 바꿔 빌드하면 일반판을 덮어쓰므로 폴더를 나눈다.
Invoke-Cargo @('build', '--release', '--locked', '-p', 'glide-core', '--features', 'uiaccess', '--target-dir', 'target\uiaccess')

$output = Join-Path $root "dist\glide-windows-$version"
if (Test-Path $output) { Remove-Item -Recurse -Force $output }
$bin = New-Item -ItemType Directory -Force (Join-Path $output 'bin')
Copy-Item (Join-Path $root 'target\release\glide-core.exe') $bin
Copy-Item (Join-Path $root 'target\release\glide-settings.exe') $bin
Copy-Item (Join-Path $root 'target\uiaccess\release\glide-core.exe') (Join-Path $bin 'glide-core-uiaccess.exe')
# MIT 라이선스는 배포물에 저작권·허가 문구를 함께 넣어야 한다.
Copy-Item (Join-Path $root 'LICENSE') (Join-Path $output 'LICENSE.txt')

# 의존 라이브러리 라이선스 고지(THIRD-PARTY-NOTICES.txt). 설정은 about.toml, 템플릿은 tools\about.hbs.
# 허용 목록 밖의 라이선스가 있거나 라이선스를 판별하지 못하면 --fail로 멈춘다.
$notices = Join-Path $output 'THIRD-PARTY-NOTICES.txt'
# cargo 하위 명령은 PATH의 cargo-about.exe다. Windows PowerShell 5.1에서 실행 파일의 오류 출력을 돌리면
# ErrorActionPreference=Stop 때문에 바로 멈추므로, 실행해 보지 않고 파일이 있는지로 확인한다.
if (Get-Command cargo-about -ErrorAction SilentlyContinue) {
    Invoke-Cargo @('about', 'generate', '--workspace', '--locked', '--fail', '-c', 'about.toml',
        '-o', $notices, 'tools\about.hbs')
} elseif ($env:GITHUB_ACTIONS -eq 'true') {
    # 배포 zip에는 고지가 꼭 들어가야 하므로 CI에서는 건너뛰지 않는다.
    throw 'cargo-about을 찾지 못했습니다(cargo install cargo-about --version 0.9.3 --locked --features cli).'
} else {
    # 소스에서 직접 빌드해 이 PC에만 설치하는 경우는 막지 않는다.
    Write-Warning 'cargo-about이 없어 THIRD-PARTY-NOTICES.txt를 만들지 않았습니다(배포용 설치 프로그램을 만들 때는 필요).'
}

# uiAccess판에만 uiAccess="true"가 들어갔는지 확인한다(Windows SDK의 mt.exe).
$mt = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\mt.exe" -ErrorAction SilentlyContinue |
    Sort-Object FullName -Descending | Select-Object -First 1
if ($mt) {
    foreach ($name in 'glide-core-uiaccess.exe', 'glide-core.exe') {
        $manifest = Join-Path ([IO.Path]::GetTempPath()) "glide-$name.manifest"
        & $mt.FullName -nologo "-inputresource:$(Join-Path $bin $name);#1" "-out:$manifest" 2>$null | Out-Null
        $text = if (Test-Path $manifest) { Get-Content -Raw $manifest } else { '' }
        Remove-Item $manifest -ErrorAction SilentlyContinue
        $hasUiAccess = $text -match "uiAccess=.true."
        $expected = $name -eq 'glide-core-uiaccess.exe'
        if ($hasUiAccess -ne $expected) { throw "$name 매니페스트가 예상과 다릅니다(uiAccess=$hasUiAccess)." }
        Write-Host "확인: $name uiAccess=$hasUiAccess"
    }
} elseif ($env:GITHUB_ACTIONS -eq 'true') {
    # 릴리스 빌드에서는 확인을 건너뛰지 않는다.
    throw 'mt.exe를 찾지 못했습니다(Windows SDK 필요).'
} else {
    Write-Warning 'mt.exe를 찾지 못해 매니페스트 확인을 건너뜁니다(Windows SDK 필요).'
}

Write-Host "패키지: $output"
