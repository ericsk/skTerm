#!/usr/bin/env bash
set -euo pipefail
umask 022

echo "=== Building skTerm for Linux (Release) ==="
cargo build --release -p skterm-app

PKG_DIR="dist/linux/skterm-0.1.0"
rm -rf dist/linux
mkdir -p "${PKG_DIR}/usr/bin" "${PKG_DIR}/usr/share/applications" "${PKG_DIR}/usr/share/pixmaps" "${PKG_DIR}/DEBIAN"
chmod 755 "${PKG_DIR}" "${PKG_DIR}/DEBIAN" "${PKG_DIR}/usr" "${PKG_DIR}/usr/bin" "${PKG_DIR}/usr/share" "${PKG_DIR}/usr/share/applications" "${PKG_DIR}/usr/share/pixmaps"

cp target/release/skterm "${PKG_DIR}/usr/bin/skterm"
chmod 755 "${PKG_DIR}/usr/bin/skterm"

# Install standard FreeDesktop hicolor icons
ICON_SIZES=(16 24 32 48 64 128 256 512)
for SIZE in "${ICON_SIZES[@]}"; do
    TARGET_ICON_DIR="${PKG_DIR}/usr/share/icons/hicolor/${SIZE}x${SIZE}/apps"
    mkdir -p "${TARGET_ICON_DIR}"
    SRC_ICON="assets/icons/hicolor/${SIZE}x${SIZE}/apps/skterm.png"
    if [ -f "${SRC_ICON}" ]; then
        cp "${SRC_ICON}" "${TARGET_ICON_DIR}/skterm.png"
    elif [ -f "assets/icon_glass_fullbleed.png" ] && command -v magick >/dev/null 2>&1; then
        magick "assets/icon_glass_fullbleed.png" -resize "${SIZE}x${SIZE}" "${TARGET_ICON_DIR}/skterm.png"
    elif [ -f "assets/icon_glass_fullbleed.png" ] && command -v convert >/dev/null 2>&1; then
        convert "assets/icon_glass_fullbleed.png" -resize "${SIZE}x${SIZE}" "${TARGET_ICON_DIR}/skterm.png"
    fi
    chmod 644 "${TARGET_ICON_DIR}/skterm.png"
done

# Legacy pixmaps fallback
if [ -f "assets/icon_256.png" ]; then
    cp assets/icon_256.png "${PKG_DIR}/usr/share/pixmaps/skterm.png"
elif [ -f "assets/icon_glass_fullbleed.png" ]; then
    cp assets/icon_glass_fullbleed.png "${PKG_DIR}/usr/share/pixmaps/skterm.png"
fi
chmod 644 "${PKG_DIR}/usr/share/pixmaps/skterm.png"

cat << 'DESKTOP' > "${PKG_DIR}/usr/share/applications/skterm.desktop"
[Desktop Entry]
Name=skTerm
GenericName=Terminal Emulator
Comment=High-Performance GPU-Accelerated Terminal Emulator
Exec=/usr/bin/skterm %U
Icon=skterm
Terminal=false
Type=Application
Categories=System;TerminalEmulator;
StartupNotify=true
StartupWMClass=skterm
Keywords=terminal;emulator;shell;prompt;command;commandline;skterm;
DESKTOP
chmod 644 "${PKG_DIR}/usr/share/applications/skterm.desktop"

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
chmod 644 "${PKG_DIR}/DEBIAN/control"

cat << 'POSTINST' > "${PKG_DIR}/DEBIAN/postinst"
#!/bin/sh
set -e

if [ "$1" = "configure" ]; then
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q /usr/share/applications || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
    fi
fi
POSTINST
chmod 755 "${PKG_DIR}/DEBIAN/postinst"

cat << 'POSTRM' > "${PKG_DIR}/DEBIAN/postrm"
#!/bin/sh
set -e

if [ "$1" = "remove" ] || [ "$1" = "purge" ]; then
    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q /usr/share/applications || true
    fi
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
    fi
fi
POSTRM
chmod 755 "${PKG_DIR}/DEBIAN/postrm"

if command -v dpkg-deb >/dev/null 2>&1; then
    dpkg-deb --root-owner-group --build "${PKG_DIR}" "dist/linux/skterm_0.1.0_amd64.deb"
    echo "Built .deb package at dist/linux/skterm_0.1.0_amd64.deb"
fi

# Also create portable standalone tarball
PORTABLE_DIR="dist/linux/skterm-v0.1.0-linux-x86_64"
mkdir -p "${PORTABLE_DIR}"
cp target/release/skterm "${PORTABLE_DIR}/skterm"
chmod 755 "${PORTABLE_DIR}/skterm"
if [ -f "assets/icon_256.png" ]; then
    cp assets/icon_256.png "${PORTABLE_DIR}/skterm.png"
elif [ -f "assets/icon_glass_fullbleed.png" ]; then
    cp assets/icon_glass_fullbleed.png "${PORTABLE_DIR}/skterm.png"
fi
if [ -d "assets/icons" ]; then
    cp -r assets/icons "${PORTABLE_DIR}/"
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
