#!/usr/bin/env bash
set -euo pipefail

echo "=== Building skTerm for Linux (Release) ==="
cargo build --release -p skterm-app

PKG_DIR="dist/linux/skterm-0.1.0"
rm -rf dist/linux
mkdir -p "${PKG_DIR}/usr/bin" "${PKG_DIR}/usr/share/applications" "${PKG_DIR}/DEBIAN"

cp target/release/skterm "${PKG_DIR}/usr/bin/skterm"
chmod 755 "${PKG_DIR}/usr/bin/skterm"

cat << 'DESKTOP' > "${PKG_DIR}/usr/share/applications/skterm.desktop"
[Desktop Entry]
Name=skTerm
Comment=High-Performance GPU-Accelerated Terminal Emulator
Exec=/usr/bin/skterm
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

echo "Successfully built Linux package bundle at ${PKG_DIR}"
