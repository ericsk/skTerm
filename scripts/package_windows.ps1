Write-Host "=== Building skTerm for Windows (Release) ==="
cargo build --release -p skterm-app

# 1. Ensure Windows Icon exists
$IcoPath = "assets\app_icon.ico"
if (-not (Test-Path $IcoPath)) {
    Write-Host "Generating app_icon.ico..."
    & pwsh -NoProfile -ExecutionPolicy Bypass -File "scripts\generate_ico.ps1"
}

# 2. Package Portable Executable & Directory
$DistDir = "dist\windows\skterm-v0.1.0-windows-x64"
if (Test-Path $DistDir) { Remove-Item -Recurse -Force $DistDir }
New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

Copy-Item "target\release\skterm.exe" "$DistDir\skterm.exe"
Copy-Item "README.md" "$DistDir\README.md"
Copy-Item "LICENSE" "$DistDir\LICENSE"
if (Test-Path $IcoPath) {
    Copy-Item $IcoPath "$DistDir\app_icon.ico"
}

$ZipPath = "dist\windows\skterm-v0.1.0-windows-x64.zip"
if (Test-Path $ZipPath) { Remove-Item -Force $ZipPath }
Compress-Archive -Path "$DistDir\*" -DestinationPath $ZipPath -Force
Write-Host "Successfully packaged portable skTerm at $ZipPath"

# 3. Build Windows Installer (Inno Setup)
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
    & $IsccPath "scripts\installer_windows.iss"
    $InstallerPath = "dist\windows\skterm-setup-v0.1.0-windows-x64.exe"
    if (Test-Path $InstallerPath) {
        Write-Host "Successfully packaged Windows Installer at $InstallerPath"
    }
} else {
    Write-Warning "Inno Setup (ISCC.exe) not found. To build installer, please install Inno Setup 6."
}

Write-Host "=== Packaging Summary ==="
Get-ChildItem -Path "dist\windows" -File | Select-Object Name, Length, LastWriteTime | Format-Table -AutoSize
