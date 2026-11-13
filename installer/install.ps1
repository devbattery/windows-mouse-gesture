<#
.SYNOPSIS
  Glide for Windows를 C:\Program Files\Glide에 설치한다(본인 PC용).

.DESCRIPTION
  관리자 권한 창(작업 관리자 등)에서도 제스처를 쓰려면 코어가 uiAccess로 실행돼야 하고, Windows는
  (1) 이 PC가 신뢰하는 인증서로 서명되고 (2) Program Files 같은 보안 위치에 있는 실행 파일만 허용한다.
  그래서 설치 때 이 PC 전용 코드 서명 인증서를 만들어 glide-core.exe 하나에만 서명하고, 공개 인증서만
  신뢰 루트에 등록한 뒤 개인키는 바로 지운다(이후 다른 프로그램에 같은 인증서로 서명할 수 없다).

  패키지의 uiAccess판이 이미 SignPath Foundation 인증서로 서명돼 있으면(공개 릴리스의 정식 서명) 인증서를 만들지 않는다.

  단계
  - 일반 권한: 실행 중인 Glide 확인·종료 → 관리자 단계 실행(UAC 확인 1회)
  - 관리자 권한: 파일 복사, 다운로드 표시 제거, 인증서 생성·서명·신뢰 루트 등록·개인키 삭제,
    이전 인증서 제거, "앱 및 기능" 등록
  - 일반 권한: 로그인 시 자동 시작 등록, 시작 메뉴 바로 가기, Glide 시작(관리자 권한이 아닌 일반 권한으로)

.PARAMETER NoUiAccess
  인증서를 만들지 않고 관리자 창 미지원판을 설치한다.
.PARAMETER NoAutoStart
  로그인할 때 자동으로 시작하지 않는다.
.PARAMETER DryRun
  아무것도 바꾸지 않고 할 일만 출력한다.

.EXAMPLE
  install.cmd
.EXAMPLE
  powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -NoUiAccess
#>
[CmdletBinding()]
param(
    [switch] $NoUiAccess,
    [switch] $NoAutoStart,
    [switch] $DryRun,
    # 내부용: 관리자 단계로 다시 실행될 때만 쓴다.
    [ValidateSet('user', 'admin')] [string] $Stage = 'user',
    [string] $LogPath
)

$ErrorActionPreference = 'Stop'
# 보안 위치 조건 때문에 설치 폴더는 바꿀 수 없게 고정한다. 환경 변수가 아니라 시스템이 알려 주는
# Program Files 경로를 쓰고, 드라이브 루트 아래의 실제 폴더인지 확인한다(엉뚱한 폴더를 지우지 않게).
$ProgramFiles = [Environment]::GetFolderPath('ProgramFiles')
if (-not $ProgramFiles -or -not [IO.Path]::IsPathRooted($ProgramFiles) -or -not (Test-Path -LiteralPath $ProgramFiles)) {
    Write-Host '오류: Program Files 폴더를 찾지 못했습니다.'; exit 1
}
$InstallDir = Join-Path $ProgramFiles 'Glide'
$Package = $PSScriptRoot
$CertSubjectPrefix = 'CN=Glide Local UIAccess Signing'
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$Shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Glide 설정.lnk'
# "설정 → 앱 및 기능"에 보이는 등록 정보(제거 버튼이 설치 폴더의 uninstall.cmd를 실행)
$UninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Glide'

function Write-Log([string] $Text) {
    Write-Host $Text
    if ($LogPath) { Add-Content -LiteralPath $LogPath -Value $Text -Encoding UTF8 }
}

# 시스템을 바꾸는 단계. DryRun이면 설명만 출력한다.
function Invoke-Step([string] $Text, [scriptblock] $Action) {
    if ($DryRun) { Write-Log "[계획] $Text"; return }
    Write-Log "- $Text"
    & $Action
}

function Test-Administrator {
    $principal = [Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
    $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Read-InstallInfo {
    $path = Join-Path $InstallDir 'install.json'
    if (Test-Path -LiteralPath $path) { Get-Content -Raw -LiteralPath $path | ConvertFrom-Json } else { $null }
}

# 지문으로 신뢰 루트의 Glide 인증서를 지운다. 다른 인증서를 지우지 않도록 주체 이름도 확인한다.
function Remove-GlideRootCertificate([string] $Thumbprint) {
    if (-not $Thumbprint) { return }
    $path = "Cert:\LocalMachine\Root\$Thumbprint"
    $cert = Get-Item -LiteralPath $path -ErrorAction SilentlyContinue
    if ($cert -and $cert.Subject.StartsWith($CertSubjectPrefix)) {
        Invoke-Step "신뢰 루트에서 이전 Glide 인증서 제거($Thumbprint)" { Remove-Item -LiteralPath $path }
    }
}

# ── 관리자 단계 ──
function Install-Files {
    if (-not $DryRun -and -not (Test-Administrator)) { throw '관리자 권한이 필요합니다.' }
    $previous = Read-InstallInfo
    $useUiAccess = -not $NoUiAccess
    $coreSource = Join-Path $Package ($(if ($useUiAccess) { 'bin\glide-core-uiaccess.exe' } else { 'bin\glide-core.exe' }))
    $core = Join-Path $InstallDir 'glide-core.exe'
    # Get-Content 결과에는 경로 등 부가 속성이 붙어 JSON·레지스트리에 객체로 들어가므로 문자열로 바꾼다.
    $version = [string](Get-Content -LiteralPath (Join-Path $Package 'version.txt') -ErrorAction SilentlyContinue | Select-Object -First 1)
    # 공개 릴리스가 정식 인증서로 서명한 uiAccess판이면 이 PC 전용 인증서가 필요 없다.
    # 'Valid'는 서명이 손상되지 않았고 인증서가 이 PC의 신뢰 루트로 이어진다는 뜻이다. 이 프로젝트가 쓰는
    # 서명자(SignPath Foundation, docs/signing.md)일 때만 정식 서명판으로 본다. 다른 서명이면 자체 서명으로 덮어쓴다.
    $alreadySigned = $false
    if ($useUiAccess) {
        $signature = Get-AuthenticodeSignature -LiteralPath $coreSource
        $alreadySigned = $signature.Status -eq 'Valid' -and $signature.SignerCertificate.Subject -like '*SignPath Foundation*'
    }
    # 인증서 단계(자식 범위)에서 만든 지문을 되돌리기에서도 쓰도록 스크립트 범위에 둔다.
    $script:newThumbprint = $null
    $isNewInstall = -not (Test-Path -LiteralPath $InstallDir)
    $hadUninstallKey = Test-Path -LiteralPath $UninstallKey
    try {
        Invoke-Step "설치 폴더 준비: $InstallDir" { New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null }
        Invoke-Step '실행 파일·제거 스크립트 복사' {
            Copy-Item -LiteralPath $coreSource -Destination $core -Force
            Copy-Item -LiteralPath (Join-Path $Package 'bin\glide-settings.exe') -Destination $InstallDir -Force
            # "앱 및 기능"의 제거 버튼이 패키지 폴더 없이도 동작하도록 제거 스크립트를 함께 둔다.
            Copy-Item -LiteralPath (Join-Path $Package 'uninstall.ps1'), (Join-Path $Package 'uninstall.cmd') -Destination $InstallDir -Force
        }
        # 인터넷에서 받은 zip을 풀면 파일마다 다운로드 표시(Zone.Identifier)가 붙고, 복사해도 그대로 남는다.
        # 남아 있으면 실행할 때마다 SmartScreen 확인 창이 뜰 수 있으므로, 사용자가 설치를 실행한 것을
        # 동의로 보고 설치 폴더의 파일에서만 지운다.
        Invoke-Step '설치한 파일의 다운로드 표시 제거(SmartScreen 반복 확인 방지)' {
            Get-ChildItem -LiteralPath $InstallDir -File | Unblock-File
        }
        if ($alreadySigned) {
            $prefix = if ($DryRun) { '[계획]' } else { '-' }
            Write-Log "$prefix 패키지의 관리자 창 지원판이 SignPath Foundation 인증서로 서명돼 있어 이 PC 전용 인증서를 만들지 않습니다."
        } elseif ($useUiAccess) {
            Invoke-Step '이 PC 전용 코드 서명 인증서 생성 → 공개 인증서만 신뢰 루트에 등록 → glide-core.exe 서명 → 개인키 삭제' {
                $cert = New-SelfSignedCertificate -Type CodeSigningCert `
                    -Subject "$CertSubjectPrefix ($env:COMPUTERNAME)" -FriendlyName 'Glide uiAccess (이 PC 전용)' `
                    -CertStoreLocation 'Cert:\LocalMachine\My' -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
                    -KeyExportPolicy NonExportable -NotAfter (Get-Date).AddYears(10)
                $script:newThumbprint = $cert.Thumbprint
                $publicCert = Join-Path ([IO.Path]::GetTempPath()) "glide-$($cert.Thumbprint).cer"
                try {
                    # 먼저 신뢰 루트에 공개 인증서를 넣어야 서명 결과를 바로 'Valid'로 확인할 수 있다.
                    Export-Certificate -Cert $cert -FilePath $publicCert | Out-Null
                    Import-Certificate -FilePath $publicCert -CertStoreLocation 'Cert:\LocalMachine\Root' | Out-Null
                    $signature = Set-AuthenticodeSignature -LiteralPath $core -Certificate $cert -HashAlgorithm SHA256
                    if ($signature.Status -ne 'Valid') { throw "서명하지 못했습니다: $($signature.StatusMessage)" }
                } finally {
                    # 개인키는 서명 직후 반드시 지운다. 남아 있으면 그 키로 서명한 프로그램을 이 PC가 신뢰하게 된다.
                    Remove-Item -LiteralPath "Cert:\LocalMachine\My\$($cert.Thumbprint)" -DeleteKey -ErrorAction SilentlyContinue
                    Remove-Item -LiteralPath $publicCert -ErrorAction SilentlyContinue
                }
                if (Test-Path -LiteralPath "Cert:\LocalMachine\My\$($cert.Thumbprint)") { throw '개인키를 지우지 못했습니다.' }
                $status = (Get-AuthenticodeSignature -LiteralPath $core).Status
                if ($status -ne 'Valid') { throw "서명 확인 결과가 올바르지 않습니다: $status" }
            }
        }
        $newThumbprint = $script:newThumbprint
        Invoke-Step '설치 정보 기록(install.json)' {
            [ordered]@{
                version = $version
                uiAccess = $useUiAccess
                # 정식 서명판이면 이 PC 전용 인증서가 없으므로 지문도 없다(제거 때 지울 인증서가 없음).
                signedByPublisher = [bool]$alreadySigned
                certificateThumbprint = $newThumbprint
                installedAt = (Get-Date).ToString('o')
            } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $InstallDir 'install.json') -Encoding UTF8
        }
        # 새 인증서가 자리 잡은 뒤에 이전 인증서를 지운다.
        if ($previous -and $previous.certificateThumbprint -and $previous.certificateThumbprint -ne $newThumbprint) {
            Remove-GlideRootCertificate $previous.certificateThumbprint
        }
        Invoke-Step '"앱 및 기능"에 등록(제거 버튼 → 설치 폴더의 uninstall.cmd)' {
            New-Item -Path $UninstallKey -Force | Out-Null
            $size = [int]((Get-ChildItem -LiteralPath $InstallDir -File | Measure-Object -Property Length -Sum).Sum / 1KB)
            $values = [ordered]@{
                DisplayName = 'Glide for Windows'
                DisplayVersion = $version
                Publisher = 'devbattery'
                URLInfoAbout = 'https://github.com/devbattery/windows-mouse-gesture'
                InstallLocation = $InstallDir
                DisplayIcon = "$core,0"
                UninstallString = "`"$(Join-Path $InstallDir 'uninstall.cmd')`""
            }
            foreach ($name in $values.Keys) {
                New-ItemProperty -Path $UninstallKey -Name $name -Value $values[$name] -PropertyType String -Force | Out-Null
            }
            # 수정·복구 버튼은 없고, 크기는 KB 단위
            foreach ($pair in @(@('NoModify', 1), @('NoRepair', 1), @('EstimatedSize', $size))) {
                New-ItemProperty -Path $UninstallKey -Name $pair[0] -Value $pair[1] -PropertyType DWord -Force | Out-Null
            }
        }
    } catch {
        Write-Log "설치 실패: $($_.Exception.Message)"
        # 되돌리기: 이번에 만든 인증서, 새로 만든 설치 폴더와 "앱 및 기능" 등록
        if ($script:newThumbprint) {
            Remove-Item -LiteralPath "Cert:\LocalMachine\My\$($script:newThumbprint)" -DeleteKey -ErrorAction SilentlyContinue
            Remove-GlideRootCertificate $script:newThumbprint
        }
        if ($isNewInstall -and -not $DryRun) { Remove-Item -Recurse -Force -LiteralPath $InstallDir -ErrorAction SilentlyContinue }
        if (-not $hadUninstallKey -and -not $DryRun) { Remove-Item -Recurse -Force -LiteralPath $UninstallKey -ErrorAction SilentlyContinue }
        throw
    }
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

# ── 일반 권한 단계 ──
function Stop-RunningGlide {
    foreach ($process in Get-Process -Name 'glide-core', 'glide-settings' -ErrorAction SilentlyContinue) {
        $path = Get-ProcessPath $process
        if ($path -and $path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase)) {
            Invoke-Step "설치된 Glide 종료($($process.ProcessName), PID $($process.Id))" { Stop-Process -Id $process.Id -Force; $process.WaitForExit(5000) | Out-Null }
        } else {
            $message = "다른 위치에서 실행 중인 Glide가 있습니다($path). 트레이 메뉴에서 종료한 뒤 다시 설치해 주세요."
            if ($DryRun) { Write-Log "[계획] 실제 설치라면 여기서 멈춤: $message" } else { throw $message }
        }
    }
}

function Install-ForUser {
    foreach ($file in 'bin\glide-core.exe', 'bin\glide-core-uiaccess.exe', 'bin\glide-settings.exe', 'uninstall.ps1', 'uninstall.cmd') {
        if (-not (Test-Path -LiteralPath (Join-Path $Package $file))) {
            throw "패키지에 $file 이 없습니다. 릴리스 zip을 먼저 압축 해제한 폴더(또는 tools\package.ps1로 만든 폴더)에서 실행해 주세요."
        }
    }
    Stop-RunningGlide

    if ($DryRun) {
        Install-Files
    } else {
        $log = Join-Path ([IO.Path]::GetTempPath()) "glide-install-$PID.log"
        $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Stage', 'admin', '-LogPath', "`"$log`"")
        if ($NoUiAccess) { $arguments += '-NoUiAccess' }
        Write-Log '관리자 권한 단계를 실행합니다(UAC 확인 창이 뜹니다).'
        try {
            $admin = Start-Process -FilePath 'powershell.exe' -ArgumentList $arguments -Verb RunAs -Wait -PassThru -WindowStyle Hidden
        } catch {
            throw '관리자 권한을 받지 못해 설치를 멈췄습니다.'
        }
        if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Encoding UTF8 | Write-Host; Remove-Item -LiteralPath $log }
        if ($admin.ExitCode -ne 0) { throw "관리자 단계가 실패했습니다(종료 코드 $($admin.ExitCode))." }
    }

    $core = Join-Path $InstallDir 'glide-core.exe'
    if ($NoAutoStart) {
        Invoke-Step '로그인 시 자동 시작 해제' { Remove-ItemProperty -Path $RunKey -Name 'Glide' -ErrorAction SilentlyContinue }
    } else {
        Invoke-Step '로그인 시 자동 시작 등록(현재 사용자)' { Set-ItemProperty -Path $RunKey -Name 'Glide' -Value "`"$core`"" }
    }
    Invoke-Step "시작 메뉴 바로 가기: $Shortcut" {
        $shell = New-Object -ComObject WScript.Shell
        $link = $shell.CreateShortcut($Shortcut)
        $link.TargetPath = Join-Path $InstallDir 'glide-settings.exe'
        $link.WorkingDirectory = $InstallDir
        $link.Description = 'Glide 마우스 제스처 설정'
        $link.Save()
    }
    # 이 창(일반 권한)에서 시작해야 관리자 권한이 아닌 일반 권한 + uiAccess로 실행된다.
    Invoke-Step 'Glide 시작' {
        try {
            Start-Process -FilePath $core
        } catch {
            throw "Glide를 시작하지 못했습니다: $($_.Exception.Message)`n관리자 창 지원판이 이 Windows에서 거부됐을 수 있습니다. -NoUiAccess로 다시 설치해 보세요."
        }
    }
    Write-Log ''
    if ($DryRun) { Write-Log '미리보기를 마쳤습니다. 아무것도 바꾸지 않았습니다.'; return }
    Write-Log "설치했습니다: $InstallDir"
    Write-Log $(if ($NoUiAccess) { '관리자 권한 창 지원: 꺼짐(-NoUiAccess)' } else { '관리자 권한 창 지원: 켜짐(트레이 메뉴 상태 줄에서 확인)' })
    Write-Log "설정은 $env:APPDATA\Glide\settings.json 에 그대로 남아 있습니다."
}

try {
    if ($Stage -eq 'admin') { Install-Files } else { Install-ForUser }
    exit 0
} catch {
    Write-Log "오류: $($_.Exception.Message)"
    exit 1
}
