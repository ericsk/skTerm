#!/usr/bin/env bash
set -euo pipefail

echo "=== Building skTerm for macOS (Release) ==="
cargo build --release -p skterm-app

APP_NAME="skTerm.app"
BUNDLE_DIR="dist/macos/${APP_NAME}"
CONTENTS_DIR="${BUNDLE_DIR}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

rm -rf dist/macos
mkdir -p "${MACOS_DIR}" "${RESOURCES_DIR}"

# Copy binary and AppIcon
cp target/release/skterm "${MACOS_DIR}/skterm"
chmod +x "${MACOS_DIR}/skterm"
if [ -f "assets/AppIcon.icns" ]; then
    cp "assets/AppIcon.icns" "${RESOURCES_DIR}/AppIcon.icns"
fi

# Create Info.plist
cat << 'PLIST' > "${CONTENTS_DIR}/Info.plist"
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
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSSupportsAutomaticGraphicsSwitching</key>
    <true/>
</dict>
</plist>
PLIST

# Ad-hoc sign bundle for local execution
echo "=== Signing macOS App Bundle ==="
codesign --force --deep --sign - "${BUNDLE_DIR}"

if [ -d "/Applications/${APP_NAME}" ]; then
    echo "=== Updating /Applications/${APP_NAME} ==="
    rm -rf "/Applications/${APP_NAME}"
    cp -R "${BUNDLE_DIR}" "/Applications/${APP_NAME}"
fi

echo "Successfully built macOS App Bundle at ${BUNDLE_DIR}"
