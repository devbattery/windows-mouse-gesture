<#
.SYNOPSIS
  Glide 설치 프로그램(installer\glide.iss)이 부르는 보조 스크립트. 직접 실행할 일은 없다.

.DESCRIPTION
  Inno Setup에 없는 일을 맡는다. 설치·제거 프로그램이 숨김 창으로 실행하고, 진행 내용은 -LogPath 파일로 돌려준다.

  - Stop: 설치 폴더에서 실행 중인 Glide를 종료한다. 다른 위치에서 실행 중이면 종료 코드 3으로 멈춘다
    (-IgnoreOtherLocations이면 무시한다. 제거 때).
  - Sign: 관리자 권한 창(uiAccess)을 지원하려면 코어가 (1) 이 PC가 신뢰하는 인증서로 서명되고 (2) Program Files에
    있어야 한다. 그래서 이 PC 전용 코드 서명 인증서를 만들어 glide-core.exe 하나에만 서명하고, 공개 인증서만 신뢰
    루트에 등록한 뒤 개인키는 바로 지운다(이후 다른 프로그램에 같은 인증서로 서명할 수 없다). 코어가 이미
    SignPath Foundation 인증서로 서명돼 있으면(정식 서명 릴리스) 인증서를 만들지 않는다.
    설치 정보(install.json)를 쓰고, 새 인증서가 자리 잡은 뒤 이전 설치의 인증서를 지운다.
  - WriteInfo: 관리자 창 미지원판의 설치 정보를 쓰고, 이전 설치의 인증서를 지운다.
  - RemoveCert: 설치 정보에 적힌 인증서를 신뢰 루트에서 지운다(제거 때).
  - CleanLegacyUser: 0.5.x의 스크립트 설치(install.ps1)가 현재 사용자 영역에 남긴 자동 시작 값과 시작 메뉴 바로
    가기를 지운다. 설치 프로그램이 관리자 권한이 아닌 원래 사용자 권한으로 실행한다.

  설치 폴더는 시스템이 알려 주는 Program Files 아래의 Glide 폴더만 받는다(엉뚱한 폴더를 다루지 않게).
  인증서는 지문과 주체 이름이 모두 Glide 것일 때만 지운다.

  종료 코드: 0 성공, 1 실패, 3 다른 위치에서 Glide가 실행 중(Stop).
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [ValidateSet('Stop', 'Sign', 'WriteInfo', 'RemoveCert', 'CleanLegacyUser')]
    [string] $Action,
    [Parameter(Mandatory)] [string] $InstallDir,
    [string] $Version,
    [switch] $IgnoreOtherLocations,
    [string] $LogPath
)

$ErrorActionPreference = 'Stop'
$CertSubjectPrefix = 'CN=Glide Local UIAccess Signing'
$ExitRunningElsewhere = 3

# 진행 내용을 설치 프로그램 로그로 돌려준다. 원래 사용자 권한으로 실행되면 로그 파일(관리자 임시 폴더)에 쓰지
# 못할 수 있는데, 그때도 작업은 계속한다.
function Write-Log([string] $Text) {
    Write-Host $Text
    if (-not $LogPath) { return }
    try { Add-Content -LiteralPath $LogPath -Value $Text -Encoding UTF8 } catch { }
}

# ── 설치 폴더 확인 ──
# 설치 프로그램은 {app}(= Program Files\Glide)만 넘긴다. 다른 경로면 아무것도 하지 않는다.
$ProgramFiles = [Environment]::GetFolderPath('ProgramFiles')
if (-not $ProgramFiles -or -not [IO.Path]::IsPathRooted($ProgramFiles) -or -not (Test-Path -LiteralPath $ProgramFiles)) {
    Write-Log '오류: Program Files 폴더를 찾지 못했습니다.'; exit 1
}
$ExpectedDir = Join-Path $ProgramFiles 'Glide'
$GivenDir = try { [IO.Path]::GetFullPath($InstallDir).TrimEnd('\') } catch { '' }
if (-not [string]::Equals($GivenDir, $ExpectedDir, [StringComparison]::OrdinalIgnoreCase)) {
    Write-Log "오류: 설치 폴더가 예상과 다릅니다($InstallDir). $ExpectedDir 만 다룹니다."; exit 1
}
$InstallDir = $ExpectedDir
$InfoPath = Join-Path $InstallDir 'install.json'
$Core = Join-Path $InstallDir 'glide-core.exe'

function Test-Administrator {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Read-InstallInfo {
    if (-not (Test-Path -LiteralPath $InfoPath)) { return $null }
    try { Get-Content -Raw -LiteralPath $InfoPath | ConvertFrom-Json } catch { Write-Log "참고: install.json을 읽지 못했습니다($($_.Exception.Message))."; $null }
}

# 설치 정보. 제거 때 지울 인증서의 지문을 기록한다(0.5.x 스크립트 설치와 같은 형식).
function Write-InstallInfo([bool] $UiAccess, [bool] $SignedByPublisher, [string] $Thumbprint) {
    [ordered]@{
        version = [string]$Version
        uiAccess = $UiAccess
        # 정식 서명판이면 이 PC 전용 인증서가 없으므로 지문도 없다(제거 때 지울 인증서가 없음).
        signedByPublisher = $SignedByPublisher
        certificateThumbprint = $(if ($Thumbprint) { $Thumbprint } else { $null })
        installedAt = (Get-Date).ToString('o')
    } | ConvertTo-Json | Set-Content -LiteralPath $InfoPath -Encoding UTF8
    Write-Log '- 설치 정보 기록(install.json)'
}

# 지문으로 신뢰 루트의 Glide 인증서를 지운다. 다른 인증서를 지우지 않도록 주체 이름도 확인한다.
function Remove-GlideRootCertificate([string] $Thumbprint) {
    if (-not $Thumbprint) { return }
    $path = "Cert:\LocalMachine\Root\$Thumbprint"
    $cert = Get-Item -LiteralPath $path -ErrorAction SilentlyContinue
    if ($cert -and $cert.Subject.StartsWith($CertSubjectPrefix)) {
        Write-Log "- 신뢰 루트에서 Glide 인증서 제거($Thumbprint)"
        Remove-Item -LiteralPath $path
    }
}

# 이전 설치(0.5.x 스크립트 설치 포함)의 인증서를 지운다. 지금 쓰는 인증서는 남긴다.
function Remove-PreviousCertificate($Previous, [string] $Keep) {
    if ($Previous -and $Previous.certificateThumbprint -and $Previous.certificateThumbprint -ne $Keep) {
        Remove-GlideRootCertificate $Previous.certificateThumbprint
    }
}

# 이 프로젝트의 정식 서명(SignPath Foundation, docs/signing.md)인지 확인한다.
# 'Valid'는 서명이 손상되지 않았고 인증서가 이 PC의 신뢰 루트로 이어진다는 뜻이다. 다른 서명자는 정식 서명으로 보지 않는다.
function Test-PublisherSigned([string] $Path) {
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    $signature.Status -eq 'Valid' -and $signature.SignerCertificate.Subject -like '*SignPath Foundation*'
}

# 실행 중인 프로세스의 실행 파일 경로. uiAccess로 실행 중인 코어는 .NET(Process.Path)이 요구하는 조회 권한으로
# 열 수 없어 경로가 비므로, 제한된 조회 권한(PROCESS_QUERY_LIMITED_INFORMATION)으로 다시 읽는다.
function Get-ProcessPath([Diagnostics.Process] $Process) {
    if ($Process.Path) { return $Process.Path }
    if (-not ('GlideProcessPath' -as [type])) {
        Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
public static class GlideProcessPath {
    [DllImport("kernel32.dll", SetLastError = true)] static extern IntPtr OpenProcess(uint access, bool inherit, int pid);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool QueryFullProcessImageName(IntPtr handle, int flags, StringBuilder name, ref int size);
    public static string Get(int pid) {
        IntPtr handle = OpenProcess(0x1000, false, pid);
        if (handle == IntPtr.Zero) return null;
        try {
            var name = new StringBuilder(32768);
            int size = name.Capacity;
            return QueryFullProcessImageName(handle, 0, name, ref size) ? name.ToString() : null;
        } finally {
            CloseHandle(handle);
        }
    }
}
'@
    }
    return [GlideProcessPath]::Get($Process.Id)
}

# ── Stop: 설치 폴더에서 실행 중인 Glide 종료 ──
function Stop-RunningGlide {
    $prefix = $InstallDir + '\'
    $elsewhere = @()
    foreach ($process in Get-Process -Name 'glide-core', 'glide-settings' -ErrorAction SilentlyContinue) {
        $path = Get-ProcessPath $process
        if ($path -and $path.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
            Write-Log "- 설치된 Glide 종료($($process.ProcessName), PID $($process.Id))"
            Stop-Process -Id $process.Id -Force
            if (-not $process.WaitForExit(5000)) { throw "Glide($($process.ProcessName))가 종료되지 않았습니다." }
        } else {
            $elsewhere += $(if ($path) { $path } else { "$($process.ProcessName), PID $($process.Id)" })
        }
    }
    if ($elsewhere.Count -gt 0 -and -not $IgnoreOtherLocations) {
        # 설치 프로그램이 이 줄을 사용자 안내에 넣는다.
        Write-Log "다른 위치에서 실행 중인 Glide: $($elsewhere -join ', ')"
        exit $ExitRunningElsewhere
    }
}

# ── Sign: 이 PC 전용 인증서로 코어 서명 ──
function Invoke-Sign {
    if (-not (Test-Administrator)) { throw '관리자 권한이 필요합니다.' }
    if (-not (Test-Path -LiteralPath $Core)) { throw "$Core 이 없습니다." }
    $previous = Read-InstallInfo
    if (Test-PublisherSigned $Core) {
        Write-Log '- 코어가 SignPath Foundation 인증서로 서명돼 있어 이 PC 전용 인증서를 만들지 않습니다.'
        Write-InstallInfo -UiAccess $true -SignedByPublisher $true -Thumbprint $null
        Remove-PreviousCertificate $previous $null
        return
    }

    Write-Log '- 이 PC 전용 코드 서명 인증서 생성 → 공개 인증서만 신뢰 루트에 등록 → glide-core.exe 서명 → 개인키 삭제'
    $cert = New-SelfSignedCertificate -Type CodeSigningCert `
        -Subject "$CertSubjectPrefix ($env:COMPUTERNAME)" -FriendlyName 'Glide uiAccess (이 PC 전용)' `
        -CertStoreLocation 'Cert:\LocalMachine\My' -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
        -KeyExportPolicy NonExportable -NotAfter (Get-Date).AddYears(10)
    $thumbprint = $cert.Thumbprint
    $publicCert = Join-Path ([IO.Path]::GetTempPath()) "glide-$thumbprint.cer"
    try {
        try {
            # 먼저 신뢰 루트에 공개 인증서를 넣어야 서명 결과를 바로 'Valid'로 확인할 수 있다.
            Export-Certificate -Cert $cert -FilePath $publicCert | Out-Null
            Import-Certificate -FilePath $publicCert -CertStoreLocation 'Cert:\LocalMachine\Root' | Out-Null
            $signature = Set-AuthenticodeSignature -LiteralPath $Core -Certificate $cert -HashAlgorithm SHA256
            if ($signature.Status -ne 'Valid') { throw "서명하지 못했습니다: $($signature.StatusMessage)" }
        } finally {
            # 개인키는 서명 직후 반드시 지운다. 남아 있으면 그 키로 서명한 프로그램을 이 PC가 신뢰하게 된다.
            Remove-Item -LiteralPath "Cert:\LocalMachine\My\$thumbprint" -DeleteKey -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $publicCert -ErrorAction SilentlyContinue
        }
        if (Test-Path -LiteralPath "Cert:\LocalMachine\My\$thumbprint") { throw '개인키를 지우지 못했습니다.' }
        $status = (Get-AuthenticodeSignature -LiteralPath $Core).Status
        if ($status -ne 'Valid') { throw "서명 확인 결과가 올바르지 않습니다: $status" }
        Write-InstallInfo -UiAccess $true -SignedByPublisher $false -Thumbprint $thumbprint
    } catch {
        # 되돌리기: 이번에 만든 인증서를 지운다. 설치 프로그램은 관리자 창 미지원판으로 바꿔 설치를 마친다.
        Remove-Item -LiteralPath "Cert:\LocalMachine\My\$thumbprint" -DeleteKey -ErrorAction SilentlyContinue
        Remove-GlideRootCertificate $thumbprint
        throw
    }
    # 새 인증서가 자리 잡은 뒤에 이전 인증서를 지운다.
    Remove-PreviousCertificate $previous $thumbprint
}

# ── WriteInfo: 관리자 창 미지원판 ──
function Invoke-WriteInfo {
    if (-not (Test-Administrator)) { throw '관리자 권한이 필요합니다.' }
    $previous = Read-InstallInfo
    Write-InstallInfo -UiAccess $false -SignedByPublisher $false -Thumbprint $null
    Remove-PreviousCertificate $previous $null
}

# ── RemoveCert: 제거 때 인증서 정리 ──
function Invoke-RemoveCert {
    if (-not (Test-Administrator)) { throw '관리자 권한이 필요합니다.' }
    $info = Read-InstallInfo
    if ($info -and $info.certificateThumbprint) { Remove-GlideRootCertificate $info.certificateThumbprint }
}

# ── CleanLegacyUser: 0.5.x 스크립트 설치의 사용자 영역 정리(원래 사용자 권한) ──
# 지금은 자동 시작을 HKLM Run(모든 사용자)에, 바로 가기를 모든 사용자 시작 메뉴에 둔다. 예전 값이 남으면 두 번 시작된다.
function Invoke-CleanLegacyUser {
    $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
    $run = (Get-ItemProperty -Path $runKey -Name 'Glide' -ErrorAction SilentlyContinue).Glide
    if ($run -and $run.IndexOf($InstallDir, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
        Write-Log '- 예전 자동 시작 등록 제거(현재 사용자)'
        Remove-ItemProperty -Path $runKey -Name 'Glide'
    }
    $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Glide 설정.lnk'
    if (Test-Path -LiteralPath $shortcut) {
        Write-Log "- 예전 시작 메뉴 바로 가기 삭제: $shortcut"
        Remove-Item -LiteralPath $shortcut
    }
}

try {
    switch ($Action) {
        'Stop' { Stop-RunningGlide }
        'Sign' { Invoke-Sign }
        'WriteInfo' { Invoke-WriteInfo }
        'RemoveCert' { Invoke-RemoveCert }
        'CleanLegacyUser' { Invoke-CleanLegacyUser }
    }
    exit 0
} catch {
    Write-Log "오류: $($_.Exception.Message)"
    exit 1
}
