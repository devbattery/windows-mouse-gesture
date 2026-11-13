<#
.SYNOPSIS
  Glide for Windows를 제거한다.

.DESCRIPTION
  - 일반 권한: 설치된 Glide 종료, 자동 시작 등록·시작 메뉴 바로 가기 제거
  - 관리자 권한: 신뢰 루트에서 Glide 인증서 제거(install.json의 지문, 주체 이름 확인), "앱 및 기능" 등록 제거,
    설치 폴더 삭제
  설정(%APPDATA%\Glide\settings.json)은 지우지 않는다.

  설치 폴더 안의 사본("앱 및 기능"의 제거 버튼)으로 실행해도 된다. 스크립트는 시작할 때 읽혀 파일을 붙잡지
  않으므로, 작업 폴더만 설치 폴더 밖으로 옮기면 자기 폴더를 지울 수 있다.

.PARAMETER DryRun
  아무것도 바꾸지 않고 할 일만 출력한다.
#>
[CmdletBinding()]
param(
    [switch] $DryRun,
    [ValidateSet('user', 'admin')] [string] $Stage = 'user',
    [string] $LogPath
)

$ErrorActionPreference = 'Stop'
# 설치 스크립트와 같은 규칙: 시스템이 알려 주는 Program Files 아래의 Glide 폴더만 다룬다.
$ProgramFiles = [Environment]::GetFolderPath('ProgramFiles')
if (-not $ProgramFiles -or -not [IO.Path]::IsPathRooted($ProgramFiles) -or -not (Test-Path -LiteralPath $ProgramFiles)) {
    Write-Host '오류: Program Files 폴더를 찾지 못했습니다.'; exit 1
}
$InstallDir = Join-Path $ProgramFiles 'Glide'
$CertSubjectPrefix = 'CN=Glide Local UIAccess Signing'
$RunKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$Shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'Glide 설정.lnk'
$UninstallKey = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\Glide'
# 작업 폴더가 설치 폴더면 폴더를 지울 수 없으므로 임시 폴더로 옮긴다.
Set-Location -LiteralPath ([IO.Path]::GetTempPath())

function Write-Log([string] $Text) {
    Write-Host $Text
    if ($LogPath) { Add-Content -LiteralPath $LogPath -Value $Text -Encoding UTF8 }
}

function Invoke-Step([string] $Text, [scriptblock] $Action) {
    if ($DryRun) { Write-Log "[계획] $Text"; return }
    Write-Log "- $Text"
    & $Action
}

function Remove-Files {
    $info = Join-Path $InstallDir 'install.json'
    if (Test-Path -LiteralPath $info) {
        $thumbprint = (Get-Content -Raw -LiteralPath $info | ConvertFrom-Json).certificateThumbprint
        if ($thumbprint) {
            $path = "Cert:\LocalMachine\Root\$thumbprint"
            $cert = Get-Item -LiteralPath $path -ErrorAction SilentlyContinue
            # 다른 인증서를 지우지 않도록 Glide가 만든 인증서인지 주체 이름으로 확인한다.
            if ($cert -and $cert.Subject.StartsWith($CertSubjectPrefix)) {
                Invoke-Step "신뢰 루트에서 Glide 인증서 제거($thumbprint)" { Remove-Item -LiteralPath $path }
            }
        }
    }
    if (Test-Path -LiteralPath $UninstallKey) {
        Invoke-Step '"앱 및 기능" 등록 제거' { Remove-Item -Recurse -Force -LiteralPath $UninstallKey }
    }
    if (Test-Path -LiteralPath $InstallDir) {
        Invoke-Step "설치 폴더 삭제: $InstallDir" { Remove-Item -Recurse -Force -LiteralPath $InstallDir }
    }
}

# 실행 중인 프로세스의 실행 파일 경로(install.ps1과 같은 방법). uiAccess로 실행 중인 코어는 .NET(Process.Path)이
# 요구하는 조회 권한으로 열 수 없어 경로가 비므로, 제한된 조회 권한으로 다시 읽는다.
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

function Uninstall-ForUser {
    foreach ($process in Get-Process -Name 'glide-core', 'glide-settings' -ErrorAction SilentlyContinue) {
        $path = Get-ProcessPath $process
        if ($path -and $path.StartsWith($InstallDir, [StringComparison]::OrdinalIgnoreCase)) {
            Invoke-Step "설치된 Glide 종료($($process.ProcessName), PID $($process.Id))" { Stop-Process -Id $process.Id -Force; $process.WaitForExit(5000) | Out-Null }
        }
    }
    $run = (Get-ItemProperty -Path $RunKey -Name 'Glide' -ErrorAction SilentlyContinue).Glide
    if ($run -and $run.Contains($InstallDir)) {
        Invoke-Step '로그인 시 자동 시작 해제' { Remove-ItemProperty -Path $RunKey -Name 'Glide' }
    }
    if (Test-Path -LiteralPath $Shortcut) {
        Invoke-Step "시작 메뉴 바로 가기 삭제: $Shortcut" { Remove-Item -LiteralPath $Shortcut }
    }
    if ($DryRun) {
        Remove-Files
    } elseif ((Test-Path -LiteralPath $InstallDir) -or (Test-Path -LiteralPath $UninstallKey)) {
        # 설치 폴더를 손으로 지웠어도 "앱 및 기능" 등록이 남아 있으면 지우러 관리자 단계로 간다.
        $log = Join-Path ([IO.Path]::GetTempPath()) "glide-uninstall-$PID.log"
        Write-Log '관리자 권한 단계를 실행합니다(UAC 확인 창이 뜹니다).'
        try {
            $admin = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -WindowStyle Hidden `
                -WorkingDirectory ([IO.Path]::GetTempPath()) `
                -ArgumentList @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Stage', 'admin', '-LogPath', "`"$log`"")
        } catch {
            throw '관리자 권한을 받지 못해 제거를 멈췄습니다.'
        }
        if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log -Encoding UTF8 | Write-Host; Remove-Item -LiteralPath $log }
        if ($admin.ExitCode -ne 0) { throw "관리자 단계가 실패했습니다(종료 코드 $($admin.ExitCode))." }
    }
    Write-Log ''
    if ($DryRun) { Write-Log '미리보기를 마쳤습니다. 아무것도 바꾸지 않았습니다(설치돼 있지 않으면 할 일이 없습니다).'; return }
    Write-Log '제거했습니다.'
    Write-Log "설정은 남겨 두었습니다: $env:APPDATA\Glide (필요 없으면 직접 지워 주세요)"
}

try {
    if ($Stage -eq 'admin') { Remove-Files } else { Uninstall-ForUser }
    exit 0
} catch {
    Write-Log "오류: $($_.Exception.Message)"
    exit 1
}
