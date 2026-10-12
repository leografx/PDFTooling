#!/bin/bash
# Build "Impose Settings.app" (macOS) from this project.
#
#   gui/make-mac-app.sh            → target/release/Impose Settings.app
#   gui/make-mac-app.sh --install  → also copies it to ~/Applications
set -euo pipefail
cd "$(dirname "$0")/.."

cargo build --release -p impose-settings

APP="target/release/Impose Settings.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/impose-settings "$APP/Contents/MacOS/impose-settings"

# App icon: drawn by the program itself, scaled into an .icns
WORK=$(mktemp -d)
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
target/release/impose-settings --write-icon "$WORK/icon1024.png" 1024 >/dev/null
for s in 16 32 128 256 512; do
  sips -z $s $s "$WORK/icon1024.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$WORK/icon1024.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$WORK"

VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' gui/Cargo.toml | head -1)
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>               <string>Impose Settings</string>
  <key>CFBundleDisplayName</key>        <string>Impose Settings</string>
  <key>CFBundleIdentifier</key>         <string>local.pdftooling.impose-settings</string>
  <key>CFBundleExecutable</key>         <string>impose-settings</string>
  <key>CFBundleIconFile</key>           <string>AppIcon</string>
  <key>CFBundlePackageType</key>        <string>APPL</string>
  <key>CFBundleShortVersionString</key> <string>${VERSION}</string>
  <key>CFBundleVersion</key>            <string>${VERSION}</string>
  <key>LSMinimumSystemVersion</key>     <string>11.0</string>
  <key>NSHighResolutionCapable</key>    <true/>
  <key>LSApplicationCategoryType</key>  <string>public.app-category.graphics-design</string>
</dict>
</plist>
PLIST

# ad-hoc signature so Apple Silicon runs it without complaint
codesign --force --deep -s - "$APP" >/dev/null 2>&1 || true

echo "Built: $APP"
if [[ "${1:-}" == "--install" ]]; then
  mkdir -p "$HOME/Applications"
  rm -rf "$HOME/Applications/Impose Settings.app"
  cp -R "$APP" "$HOME/Applications/"
  echo "Installed: ~/Applications/Impose Settings.app"
fi
