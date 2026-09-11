#!/usr/bin/env bash
set -euo pipefail

echo "=== Building skTerm for Linux (Release) ==="
cargo build --release -p skterm-app

PKG_DIR="dist/linux/skterm-0.1.0"
rm -rf dist/linux
mkdir -p "${PKG_DIR}/usr/bin" "${PKG_DIR}/usr/share/applications" "${PKG_DIR}/usr/share/pixmaps" "${PKG_DIR}/DEBIAN"

cp target/release/skterm "${PKG_DIR}/usr/bin/skterm"
chmod 755 "${PKG_DIR}/usr/bin/skterm"

if [ -f "assets/icon_glass_fullbleed.png" ]; then
    cp assets/icon_glass_fullbleed.png "${PKG_DIR}/usr/share/pixmaps/skterm.png"
fi

cat << 'DESKTOP' > "${PKG_DIR}/usr/share/applications/skterm.desktop"
[Desktop Entry]
Name=skTerm
Comment=High-Performance GPU-Accelerated Terminal Emulator
Exec=/usr/bin/skterm
Icon=skterm
Terminal=false
Type=Application
Categories=System;TerminalEmulator;
StartupNotify=true
DESKTOP

cat << 'CONTROL' > "${PKG_DIR}/DEBIAN/control"
Package: skterm
Version: 0.1.0
Section: utils
Priority: optional
Architecture: amd64
Maintainer: skTerm Developers
Description: High-Performance GPU-Accelerated Terminal Emulator in Rust
 Ultra-fast GPU-accelerated terminal emulator with VT100/xterm compliance,
 multi-tab and BSP split pane multiplexing, Sixel and Kitty graphics.
CONTROL

if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb --build "${PKG_DIR}" "dist/linux/skterm_0.1.0_amd64.deb"
    echo "Built .deb package at dist/linux/skterm_0.1.0_amd64.deb"
fi

# Also create portable standalone tarball
PORTABLE_DIR="dist/linux/skterm-v0.1.0-linux-x86_64"
mkdir -p "${PORTABLE_DIR}"
cp target/release/skterm "${PORTABLE_DIR}/skterm"
chmod 755 "${PORTABLE_DIR}/skterm"
if [ -f "assets/icon_glass_fullbleed.png" ]; then
    cp assets/icon_glass_fullbleed.png "${PORTABLE_DIR}/skterm.png"
fi
cp "${PKG_DIR}/usr/share/applications/skterm.desktop" "${PORTABLE_DIR}/"
cp README.md LICENSE "${PORTABLE_DIR}/"
cat << 'RUNNER' > "${PORTABLE_DIR}/run.sh"
#!/usr/bin/env bash
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" >/dev/null 2>&1 && pwd)"
exec "${DIR}/skterm" "$@"
RUNNER
chmod +x "${PORTABLE_DIR}/run.sh"
tar -czf "dist/linux/skterm-v0.1.0-linux-x86_64.tar.gz" -C dist/linux "skterm-v0.1.0-linux-x86_64"
echo "Built portable tarball at dist/linux/skterm-v0.1.0-linux-x86_64.tar.gz"

echo "Successfully built Linux package bundle at ${PKG_DIR}"
