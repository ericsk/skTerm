# 0. Determine Version from Cargo.toml (or VERSION env var)
$Version = $env:VERSION
if ([string]::IsNullOrWhiteSpace($Version)) {
    $CargoToml = Get-Content "Cargo.toml" -Raw
    if ($CargoToml -match '(?m)^version\s*=\s*"([^"]+)"') {
        $Version = $Matches[1]
    } else {
        $Version = "0.1.0"
    }
}
$Version = $Version.TrimStart('v')

# 1. Ensure Windows Icon exists and is up to date with source PNG / generator script
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
    Write-Host "Generating app_icon.ico..."
    & pwsh -NoProfile -ExecutionPolicy Bypass -File "scripts\generate_ico.ps1"
}

# 2. Build Release Executable (with static C Runtime for zero-dependency standalone execution)
Write-Host "=== Building skTerm v$Version for Windows (Release, Static CRT) ==="
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --release -p skterm-app
if ($LASTEXITCODE -ne 0) {
    Write-Error "Cargo build failed with exit code $LASTEXITCODE"
    exit $LASTEXITCODE
}

$SourceExe = "target\release\skterm.exe"
if (-not (Test-Path $SourceExe)) {
    Write-Error "Executable not found at $SourceExe"
    exit 1
}

# 3. Package Standalone Single-File Executable
New-Item -ItemType Directory -Force -Path "dist\windows" | Out-Null
New-Item -ItemType Directory -Force -Path "dist\standalone" | Out-Null

$StandaloneWindowsExe = "dist\windows\skterm-v$Version-windows-x64.exe"
$StandaloneDirExe = "dist\standalone\skterm.exe"
$StandaloneDirVersionedExe = "dist\standalone\skterm-v$Version-windows-x64.exe"

Copy-Item -Path $SourceExe -Destination $StandaloneWindowsExe -Force
Copy-Item -Path $SourceExe -Destination $StandaloneDirExe -Force
Copy-Item -Path $SourceExe -Destination $StandaloneDirVersionedExe -Force

$Now = Get-Date
(Get-Item $StandaloneWindowsExe).LastWriteTime = $Now
(Get-Item $StandaloneDirExe).LastWriteTime = $Now
(Get-Item $StandaloneDirVersionedExe).LastWriteTime = $Now
Write-Host "Successfully packaged standalone single-file executable at $StandaloneWindowsExe"

# 4. Package Portable Directory & ZIP Archive
$DistDir = "dist\windows\skterm-v$Version-windows-x64"
if (Test-Path $DistDir) { Remove-Item -Recurse -Force $DistDir }
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

Copy-Item $SourceExe "$DistDir\skterm.exe" -Force
(Get-Item "$DistDir\skterm.exe").LastWriteTime = $Now
Copy-Item "README.md" "$DistDir\README.md"
Copy-Item "LICENSE" "$DistDir\LICENSE"
if (Test-Path $IcoPath) {
    Copy-Item $IcoPath "$DistDir\app_icon.ico"
}

$ZipPath = "dist\windows\skterm-v$Version-windows-x64.zip"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path "$DistDir\*" -DestinationPath $ZipPath -Force
Write-Host "Successfully packaged portable skTerm archive at $ZipPath"

# 5. Build Windows Installer (Inno Setup)
$IsccCandidates = @(
    "ISCC.exe",
    "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe",
    "C:\Program Files (x86)\Inno Setup 6\ISCC.exe",
    "C:\Program Files\Inno Setup 6\ISCC.exe"
)

$IsccPath = $null
foreach ($c in $IsccCandidates) {
    if (Get-Command $c -ErrorAction SilentlyContinue) {
        $IsccPath = $c
        break
    }
    if (Test-Path $c) {
        $IsccPath = $c
        break
    }
}

if ($IsccPath) {
    Write-Host "Compiling Windows Installer using $IsccPath..."
    & $IsccPath "/DMyAppVersion=$Version" "scripts\installer_windows.iss"
    $InstallerPath = "dist\windows\skterm-setup-v$Version-windows-x64.exe"
    if (Test-Path $InstallerPath) {
        Write-Host "Successfully packaged Windows Installer at $InstallerPath"
    }
} else {
    Write-Warning "Inno Setup (ISCC.exe) not found. To build installer, please install Inno Setup 6."
}

# 6. Notify Windows Shell to refresh Explorer/Taskbar icon cache
try {
    Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public class ShellIconCacheRefresh {
    [DllImport("shell32.dll")]
    public static extern void SHChangeNotify(int wEventId, uint uFlags, IntPtr dwItem1, IntPtr dwItem2);
}
"@ -ErrorAction SilentlyContinue
    # SHCNE_ASSOCCHANGED = 0x08000000, SHCNF_IDLIST = 0x0000
    [ShellIconCacheRefresh]::SHChangeNotify(0x08000000, 0x0000, [IntPtr]::Zero, [IntPtr]::Zero)
} catch {}

Write-Host "=== Packaging Summary ==="
Get-ChildItem -Path "dist\windows" -File | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
