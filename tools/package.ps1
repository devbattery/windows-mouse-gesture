<#
.SYNOPSIS
  Glide for Windows 설치 패키지(dist\glide-windows-<버전>\)를 만든다.

.DESCRIPTION
  1. 일반 빌드: glide-core.exe(관리자 창 미지원판), glide-settings.exe
  2. uiAccess 빌드: glide-core-uiaccess.exe(관리자 창 지원판, 설치 때 서명)
  3. 설치·제거 스크립트를 함께 복사하고, uiAccess 매니페스트가 실제로 들어갔는지 mt.exe로 확인한다.

  이 스크립트는 빌드와 파일 복사만 한다. 인증서·서명·Program Files 설치는 installer\install.ps1이 한다.

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

Invoke-Cargo @('build', '--release', '--workspace')
# 같은 target 폴더에서 기능만 바꿔 빌드하면 일반판을 덮어쓰므로 폴더를 나눈다.
Invoke-Cargo @('build', '--release', '-p', 'glide-core', '--features', 'uiaccess', '--target-dir', 'target\uiaccess')

$output = Join-Path $root "dist\glide-windows-$version"
if (Test-Path $output) { Remove-Item -Recurse -Force $output }
$bin = New-Item -ItemType Directory -Force (Join-Path $output 'bin')
Copy-Item (Join-Path $root 'target\release\glide-core.exe') $bin
Copy-Item (Join-Path $root 'target\release\glide-settings.exe') $bin
Copy-Item (Join-Path $root 'target\uiaccess\release\glide-core.exe') (Join-Path $bin 'glide-core-uiaccess.exe')
Copy-Item (Join-Path $root 'installer\*') $output
# 실행 파일에는 버전 자원이 없으므로 설치 스크립트가 install.json에 기록할 버전을 따로 둔다.
Set-Content -LiteralPath (Join-Path $output 'version.txt') -Value $version -Encoding ASCII

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
} else {
    Write-Warning 'mt.exe를 찾지 못해 매니페스트 확인을 건너뜁니다(Windows SDK 필요).'
}

Write-Host "패키지: $output"
