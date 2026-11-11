#!/bin/sh
# Builds Inert.app: the browser and its Servo helper side by side in
# Contents/MacOS, an Info.plist, and the keyhole icon rendered by the
# browser itself (--write-icon). A real app bundle gets its own entry in
# macOS Privacy & Security (Full Disk Access for the Chrome import, Keychain
# prompts that name "Inert"). Usage: scripts/bundle.sh  → target/Inert.app
set -e
cd "$(dirname "$0")/.."
scripts/build.sh --release
APP=target/Inert.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/inert-browser target/release/inert-servo-host "$APP/Contents/MacOS/"
[ -x target/release/inert-dictate ] && cp target/release/inert-dictate "$APP/Contents/MacOS/"
ICON=target/icon.iconset
rm -rf "$ICON" && mkdir -p "$ICON"
target/release/inert-browser --write-icon target/icon-1024.png 1024
for s in 16 32 128 256 512; do
  sips -z $s $s target/icon-1024.png --out "$ICON/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2)); sips -z $d $d target/icon-1024.png --out "$ICON/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICON" -o "$APP/Contents/Resources/Inert.icns"
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' crates/inert-browser/Cargo.toml | head -1)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Inert</string>
  <key>CFBundleDisplayName</key><string>Inert</string>
  <key>CFBundleIdentifier</key><string>dev.inert.browser</string>
  <key>CFBundleExecutable</key><string>inert-browser</string>
  <key>CFBundleIconFile</key><string>Inert</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>${VERSION:-0.1.0}</string>
  <key>CFBundleVersion</key><string>${VERSION:-0.1.0}</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Inert listens while you hold Option+Space, to type what you say.</string>
  <key>NSSpeechRecognitionUsageDescription</key><string>Inert turns what you say while holding Option+Space into text.</string>
  <key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>
PLIST
# Ad-hoc signature so macOS keeps one identity for the permission entries.
codesign --force --deep --sign - "$APP" >/dev/null 2>&1 || true
echo "$APP"
