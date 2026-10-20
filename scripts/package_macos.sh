#!/usr/bin/env bash
set -euo pipefail

VERSION="0.1.0"
ARCH="$(uname -m)"
APP_NAME="skTerm.app"
BUNDLE_DIR="dist/macos/${APP_NAME}"
CONTENTS_DIR="${BUNDLE_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"
DMG_STAGING_DIR="dist/macos/dmg_staging"
DMG_NAME="skTerm-v${VERSION}-macos-${ARCH}.dmg"
DMG_PATH="dist/macos/${DMG_NAME}"
OTA_TAR_NAME="skTerm-v${VERSION}-macos-${ARCH}.app.tar.gz"
OTA_TAR_PATH="dist/macos/${OTA_TAR_NAME}"
SHA256_PATH="dist/macos/SHA256SUMS.txt"

echo "=== Building skTerm v${VERSION} for macOS (${ARCH}, Release) ==="
cargo build --release -p skterm-app

rm -rf dist/macos
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"

# Copy binary and AppIcon
cp target/release/skterm "${MACOS_DIR}/skterm"
chmod +x "${MACOS_DIR}/skterm"
if [ -f "assets/AppIcon.icns" ]; then
    cp "assets/AppIcon.icns" "${RESOURCES_DIR}/AppIcon.icns"
fi

# Create Info.plist
cat << PLIST > "${CONTENTS_DIR}/Info.plist"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>skterm</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundleIdentifier</key>
    <string>com.ericsk.skterm</string>
    <key>CFBundleName</key>
    <string>skTerm</string>
    <key>CFBundleDisplayName</key>
    <string>skTerm</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleLocalizations</key>
    <array>
        <string>zh-Hant</string>
        <string>zh-Hans</string>
        <string>zh_TW</string>
        <string>zh_CN</string>
        <string>en</string>
    </array>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
PLIST

# Clear extended attributes and sign bundle (supports CODESIGN_IDENTITY env var, defaults to ad-hoc '-')
echo "=== Signing macOS App Bundle ==="
xattr -cr "${BUNDLE_DIR}" 2>/dev/null || true
codesign --force --deep --sign "${CODESIGN_IDENTITY:--}" "${BUNDLE_DIR}"

if [ "${SKIP_LOCAL_INSTALL:-0}" != "1" ] && [ -d "/Applications/${APP_NAME}" ]; then
    echo "=== Updating /Applications/${APP_NAME} ==="
    rm -rf "/Applications/${APP_NAME}" 2>/dev/null || true
    cp -R "${BUNDLE_DIR}" "/Applications/${APP_NAME}" 2>/dev/null || true
fi

echo "=== Packaging Distributable DMG (${DMG_PATH}) ==="
rm -rf "${DMG_STAGING_DIR}" "${DMG_PATH}"
mkdir -p "${DMG_STAGING_DIR}"
cp -R "${BUNDLE_DIR}" "${DMG_STAGING_DIR}/${APP_NAME}"
ln -s /Applications "${DMG_STAGING_DIR}/Applications"

hdiutil create \
    -volname "skTerm" \
    -srcfolder "${DMG_STAGING_DIR}" \
    -ov \
    -format UDZO \
    "${DMG_PATH}"

rm -rf "${DMG_STAGING_DIR}"

echo "=== Packaging OTA App Archive (${OTA_TAR_PATH}) ==="
COPYFILE_DISABLE=1 tar -czf "${OTA_TAR_PATH}" -C "dist/macos" "${APP_NAME}"

echo "=== Generating SHA256SUMS.txt (${SHA256_PATH}) ==="
(
    cd dist/macos
    shasum -a 256 "${DMG_NAME}" "${OTA_TAR_NAME}" > SHA256SUMS.txt
)

echo "Successfully built macOS App Bundle at ${BUNDLE_DIR}"
echo "Successfully built macOS DMG Installer at ${DMG_PATH}"
echo "Successfully built macOS OTA Archive at ${OTA_TAR_PATH}"
echo "Successfully generated checksums at ${SHA256_PATH}"
