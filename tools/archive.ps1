<#
.SYNOPSIS
  tools\package.ps1이 만든 dist\glide-windows-<버전>\ 폴더를 배포용 zip과 SHA256SUMS.txt로 묶는다.

.DESCRIPTION
  - dist\glide-windows-<버전>.zip: 압축을 풀면 glide-windows-<버전>\ 폴더 하나가 나온다(install.cmd 포함).
  - dist\SHA256SUMS.txt: zip의 SHA-256. `sha256sum -c`와 같은 형식("<해시>  <파일 이름>").
  CI는 package.ps1과 이 스크립트 사이에 실행 파일 코드 서명을 넣는다.

.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File tools\archive.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"' |
    Select-Object -First 1).Matches[0].Groups[1].Value
$name = "glide-windows-$version"
$dist = Join-Path $root 'dist'
$folder = Join-Path $dist $name
# 배포 zip에는 의존 라이브러리 라이선스 고지(THIRD-PARTY-NOTICES.txt, cargo-about 필요)도 반드시 넣는다.
foreach ($file in 'install.cmd', 'install.ps1', 'uninstall.cmd', 'uninstall.ps1', 'version.txt', 'LICENSE.txt',
    'THIRD-PARTY-NOTICES.txt', 'bin\glide-core.exe', 'bin\glide-core-uiaccess.exe', 'bin\glide-settings.exe') {
    if (-not (Test-Path -LiteralPath (Join-Path $folder $file))) { throw "$folder 에 $file 이 없습니다. 먼저 tools\package.ps1을 실행해 주세요." }
}

$zip = Join-Path $dist "$name.zip"
if (Test-Path -LiteralPath $zip) { Remove-Item -LiteralPath $zip }
# Windows PowerShell 5.1의 ZipFile.CreateFromDirectory는 항목 경로를 '\'로 기록해 zip 표준('/')과 다르다.
# 다른 압축 프로그램에서도 폴더 구조가 그대로 보이도록 항목을 하나씩 '/' 경로로 넣는다.
Add-Type -AssemblyName System.IO.Compression, System.IO.Compression.FileSystem
$folder = (Resolve-Path -LiteralPath $folder).Path
$stream = [IO.File]::Open($zip, [IO.FileMode]::CreateNew)
try {
    $archive = New-Object IO.Compression.ZipArchive($stream, [IO.Compression.ZipArchiveMode]::Create)
    try {
        foreach ($file in Get-ChildItem -LiteralPath $folder -File -Recurse | Sort-Object FullName) {
            $relative = $file.FullName.Substring($folder.Length).TrimStart('\').Replace('\', '/')
            [void][IO.Compression.ZipFileExtensions]::CreateEntryFromFile($archive, $file.FullName, "$name/$relative",
                [IO.Compression.CompressionLevel]::Optimal)
        }
    } finally {
        $archive.Dispose()
    }
} finally {
    $stream.Dispose()
}

$hash = (Get-FileHash -LiteralPath $zip -Algorithm SHA256).Hash.ToLowerInvariant()
$sums = Join-Path $dist 'SHA256SUMS.txt'
[IO.File]::WriteAllText($sums, "$hash  $name.zip`n", [Text.Encoding]::ASCII)

Write-Host "zip: $zip"
Write-Host "SHA-256: $hash"
