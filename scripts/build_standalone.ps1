# ==============================================================================
# skTerm Windows Standalone Single-File Packager
# Builds a completely self-contained, portable .exe that requires no runtime,
# no installation, and no external DLLs (statically links CRT).
# ==============================================================================

[CmdletBinding()]
param()

$ErrorActionPreference = "Stop"

# Ensure we operate from repository root
$RepoRoot = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path "$RepoRoot\Cargo.toml")) {
    $RepoRoot = (Get-Location).Path
}
Set-Location $RepoRoot

Write-Host "==========================================================" -ForegroundColor Cyan
Write-Host "         skTerm Windows Standalone Packager               " -ForegroundColor Cyan
Write-Host "==========================================================" -ForegroundColor Cyan

# 1. Parse Version from Cargo.toml
$Version = "0.1.0"
if (Test-Path "Cargo.toml") {
    $CargoContent = Get-Content "Cargo.toml" -Raw
    if ($CargoContent -match '\[workspace\.package\][\s\S]*?version\s*=\s*"([^"]+)"') {
        $Version = $matches[1]
    }
}
Write-Host "Target Version: v$Version" -ForegroundColor Gray

# 2. Ensure Windows Icon exists and is up to date
$IcoPath = "assets\app_icon.ico"
$SrcPngPath = "assets\icon_glass_fullbleed.png"
$GenScriptPath = "scripts\generate_ico.ps1"
$NeedGenerateIco = (-not (Test-Path $IcoPath))
if (-not $NeedGenerateIco) {
    $IcoTime = (Get-Item $IcoPath).LastWriteTimeUtc
    if ((Test-Path $SrcPngPath) -and ((Get-Item $SrcPngPath).LastWriteTimeUtc -gt $IcoTime)) {
        $NeedGenerateIco = $true
    }
    if ((Test-Path $GenScriptPath) -and ((Get-Item $GenScriptPath).LastWriteTimeUtc -gt $IcoTime)) {
        $NeedGenerateIco = $true
    }
}

if ($NeedGenerateIco) {
    Write-Host "`n>> [1/4] Generating app_icon.ico..." -ForegroundColor Yellow
    if (Get-Command pwsh -ErrorAction SilentlyContinue) {
        & pwsh -NoProfile -ExecutionPolicy Bypass -File "scripts\generate_ico.ps1"
    } else {
        & powershell -NoProfile -ExecutionPolicy Bypass -File "scripts\generate_ico.ps1"
    }
} else {
    Write-Host "`n>> [1/4] App icon found: $IcoPath" -ForegroundColor Green
}

# 3. Configure Static CRT (vcruntime140.dll embedded statically)
Write-Host "`n>> [2/4] Setting RUSTFLAGS for static C Runtime (+crt-static)..." -ForegroundColor Yellow
$env:RUSTFLAGS = "-C target-feature=+crt-static"

# 4. Build Release Executable
Write-Host "`n>> [3/4] Building skTerm standalone executable (Release)..." -ForegroundColor Yellow
cargo build --release -p skterm-app
if ($LASTEXITCODE -ne 0) {
    Write-Error "Cargo build failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}

# 5. Output Standalone Executable to dist\standalone and dist\windows
Write-Host "`n>> [4/4] Packaging standalone .exe to dist\standalone & dist\windows..." -ForegroundColor Yellow
$DistDir = "dist\standalone"
if (-not (Test-Path $DistDir)) {
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
}
New-Item -ItemType Directory -Force -Path "dist\windows" | Out-Null

$SourceExe = "target\release\skterm.exe"
if (-not (Test-Path $SourceExe)) {
    Write-Error "Executable not found at $SourceExe"
    exit 1
}

$TargetExe = "$DistDir\skterm.exe"
$TargetVersionedExe = "$DistDir\skterm-v$Version-windows-x64.exe"
$TargetWindowsExe = "dist\windows\skterm-v$Version-windows-x64.exe"

Copy-Item -Path $SourceExe -Destination $TargetExe -Force
Copy-Item -Path $SourceExe -Destination $TargetVersionedExe -Force
Copy-Item -Path $SourceExe -Destination $TargetWindowsExe -Force

$Now = Get-Date
(Get-Item $TargetExe).LastWriteTime = $Now
(Get-Item $TargetVersionedExe).LastWriteTime = $Now
(Get-Item $TargetWindowsExe).LastWriteTime = $Now

try {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class ShellIconCacheRefreshStandalone {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
}
"@ -ErrorAction SilentlyContinue
    [ShellIconCacheRefreshStandalone]::SHChangeNotify(0x08000000, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero)
} catch {}

$FileInfo = Get-Item $TargetExe
$SizeMb = [math]::Round($FileInfo.Length / 1MB, 2)

Write-Host "`n==========================================================" -ForegroundColor Green
Write-Host " Build succeeded! Standalone executable is ready:" -ForegroundColor Green
Write-Host "==========================================================" -ForegroundColor Green
Write-Host " 1. Portable:  $TargetExe ($SizeMb MB)" -ForegroundColor White
Write-Host " 2. Versioned: $TargetVersionedExe" -ForegroundColor White
Write-Host " 3. Windows:   $TargetWindowsExe" -ForegroundColor White
Write-Host ""
Write-Host "Note: You can directly distribute any .exe file above." -ForegroundColor Cyan
Write-Host "Anyone on Windows 10/11 can download and run it with zero dependencies." -ForegroundColor Cyan
