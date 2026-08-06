Write-Host "=== Building skTerm for Windows (Release) ==="
cargo build --release -p skterm-app

$DistDir = "dist\windows\skterm-v0.1.0-windows-x64"
if (Test-Path $DistDir) { Remove-Item -Recurse -Force $DistDir }
New-Item -ItemType Directory -Force -Path $DistDir

Copy-Item "target\release\skterm.exe" "$DistDir\skterm.exe"
Copy-Item "README.md" "$DistDir\README.md"

Compress-Archive -Path "$DistDir\*" -DestinationPath "dist\windows\skterm-v0.1.0-windows-x64.zip" -Force
Write-Host "Successfully packaged skTerm for Windows at dist\windows\skterm-v0.1.0-windows-x64.zip"
