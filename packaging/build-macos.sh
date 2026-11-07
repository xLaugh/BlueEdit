#!/usr/bin/env bash
# Builds "Blue Edit.app" for macOS (Apple Silicon and Intel) and a .dmg.
# Needs: Xcode command line tools (xcode-select --install) and Rust (rustup).
# To use it: ffmpeg (brew install ffmpeg) for video import and export.
set -euo pipefail
cd "$(dirname "$0")/.."

# The project's .cargo/config.toml points at a Windows compiler; use the
# normal one here (environment variables take precedence over that file).
export RUSTC="${RUSTC:-rustc}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

NAME="Blue Edit"
SHORT="Blue"
BIN="blue"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
DIST="dist/macos"
APP="$DIST/$NAME.app"

targets=()
for t in aarch64-apple-darwin x86_64-apple-darwin; do
  if rustup target list --installed 2>/dev/null | grep -q "$t"; then targets+=("$t"); fi
done
if [ ${#targets[@]} -eq 0 ]; then targets=("$(rustc -vV | sed -n 's/host: //p')"); fi

bins=()
for t in "${targets[@]}"; do
  echo "==> cargo build --release --target $t"
  cargo build --release --locked --target "$t"
  bins+=("target/$t/release/$BIN")
done

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
if [ ${#bins[@]} -gt 1 ]; then
  lipo -create -output "$APP/Contents/MacOS/$BIN" "${bins[@]}"   # universal binary
else
  cp "${bins[0]}" "$APP/Contents/MacOS/$BIN"
fi

# Icon: drawn by the program itself, converted to .icns
ICONSET="$DIST/icon.iconset"
rm -rf "$ICONSET"; mkdir -p "$ICONSET"
"$APP/Contents/MacOS/$BIN" --write-icon "$DIST/icon_1024.png" 1024
for s in 16 32 128 256 512; do
  sips -z $s $s "$DIST/icon_1024.png" --out "$ICONSET/icon_${s}x${s}.png" >/dev/null
  d=$((s * 2))
  sips -z $d $d "$DIST/icon_1024.png" --out "$ICONSET/icon_${s}x${s}@2x.png" >/dev/null
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/AppIcon.icns"
rm -rf "$ICONSET" "$DIST/icon_1024.png"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>$SHORT</string>
  <key>CFBundleDisplayName</key><string>$NAME</string>
  <key>CFBundleIdentifier</key><string>io.github.xlaugh.blueedit</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>$BIN</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.video</string>
  <key>CFBundleDocumentTypes</key>
  <array>
    <dict>
      <key>CFBundleTypeName</key><string>Projet Blue Edit</string>
      <key>CFBundleTypeExtensions</key><array><string>blue</string><string>bmproj</string></array>
      <key>CFBundleTypeRole</key><string>Editor</string>
    </dict>
  </array>
</dict>
</plist>
PLIST

# Optional: BUNDLE_FFMPEG=1 copies ffmpeg/ffprobe into the app (the app looks in
# Contents/Resources). Only for self-contained (static) builds: Homebrew's ffmpeg
# depends on Homebrew libraries and would not start on another Mac. Otherwise the
# app finds ffmpeg in Homebrew/MacPorts/PATH at run time.
if [ "${BUNDLE_FFMPEG:-0}" = "1" ]; then
  for tool in ffmpeg ffprobe; do
    if p="$(command -v $tool)"; then
      cp "$p" "$APP/Contents/Resources/$tool"
      echo "==> $tool copied into the app"
    else
      echo "!! $tool not found"
    fi
  done
fi

# Ad-hoc signature so Gatekeeper lets it start after "Open" (no Apple ID needed)
codesign --force --deep --sign - "$APP" 2>/dev/null || true

if [ ${#bins[@]} -gt 1 ]; then SUFFIX="universal"; else SUFFIX="$(uname -m)"; fi
DMG="$DIST/BlueEdit-$VERSION-macos-$SUFFIX.dmg"
rm -f "$DMG"
hdiutil create -volname "$NAME" -srcfolder "$APP" -ov -format UDZO "$DMG" >/dev/null && echo "==> $DMG"
echo "==> $APP"
