#!/usr/bin/env bash
# Builds Blue Edit ("Blue") for Linux into dist/linux:
#   blue-edit-<version>-linux-<arch>.tar.gz   folder with install.sh (per-user install)
#   blue-edit_<version>_<debarch>.deb         Debian/Ubuntu package (when dpkg-deb exists)
#   BlueEdit-<version>-<arch>.AppImage        (when appimagetool exists, or APPIMAGETOOL=path)
# Build dependencies (Debian/Ubuntu):
#   sudo apt install build-essential pkg-config libasound2-dev libgtk-3-dev \
#        libxkbcommon-dev libgl1-mesa-dev libwayland-dev ffmpeg
# Fedora: sudo dnf install gcc pkgconf alsa-lib-devel gtk3-devel libxkbcommon-devel mesa-libGL-devel ffmpeg
set -euo pipefail
cd "$(dirname "$0")/.."

export RUSTC="${RUSTC:-rustc}"
export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"

for pc in alsa gtk+-3.0; do
  if ! pkg-config --exists "$pc" 2>/dev/null; then
    echo "!! missing development package for '$pc' (see the list at the top of this script)"
  fi
done

BIN="blue"
APPID="blue-edit"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
ARCH="$(uname -m)"
cargo build --release --locked

OUT="dist/linux/blue-edit-$VERSION-$ARCH"
rm -rf "$OUT"
mkdir -p "$OUT/usr/bin" "$OUT/usr/share/applications" "$OUT/usr/share/icons/hicolor/256x256/apps"
cp "target/release/$BIN" "$OUT/usr/bin/$BIN"
strip "$OUT/usr/bin/$BIN" 2>/dev/null || true
"$OUT/usr/bin/$BIN" --write-icon "$OUT/usr/share/icons/hicolor/256x256/apps/$APPID.png" 256

cat > "$OUT/usr/share/applications/$APPID.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Blue Edit
X-GNOME-FullName=Blue Edit
GenericName=Video Editor
GenericName[fr]=Logiciel de montage vidéo
Comment=Track-based video editor
Comment[fr]=Montage vidéo à pistes
Exec=$BIN %F
Icon=$APPID
Terminal=false
Categories=AudioVideo;Video;AudioVideoEditing;
MimeType=video/mp4;video/quicktime;video/x-matroska;video/webm;audio/x-wav;
StartupWMClass=blue-edit
DESKTOP

# Simple installer for the current user
cat > "$OUT/install.sh" <<'INSTALL'
#!/usr/bin/env bash
set -e
here="$(cd "$(dirname "$0")" && pwd)"
mkdir -p ~/.local/bin ~/.local/share/applications ~/.local/share/icons/hicolor/256x256/apps
cp "$here/usr/bin/blue" ~/.local/bin/
cp "$here/usr/share/applications/blue-edit.desktop" ~/.local/share/applications/
cp "$here/usr/share/icons/hicolor/256x256/apps/blue-edit.png" ~/.local/share/icons/hicolor/256x256/apps/
command -v ffmpeg >/dev/null || echo "Installez ffmpeg (ex. sudo apt install ffmpeg) pour importer et exporter des vidéos."
echo "Blue Edit est installé (menu des applications, ou commande : blue)."
INSTALL
chmod +x "$OUT/install.sh"

tar -C dist/linux -czf "dist/linux/blue-edit-$VERSION-linux-$ARCH.tar.gz" "$(basename "$OUT")"
echo "==> dist/linux/blue-edit-$VERSION-linux-$ARCH.tar.gz"

# Debian / Ubuntu package
if command -v dpkg-deb >/dev/null; then
  case "$ARCH" in x86_64) DEBARCH=amd64 ;; aarch64) DEBARCH=arm64 ;; *) DEBARCH="$ARCH" ;; esac
  DEB="dist/linux/deb"
  rm -rf "$DEB"; mkdir -p "$DEB/DEBIAN"
  cp -r "$OUT/usr" "$DEB/"
  SIZE="$(du -sk "$DEB/usr" | cut -f1)"
  cat > "$DEB/DEBIAN/control" <<CONTROL
Package: $APPID
Version: $VERSION
Architecture: $DEBARCH
Maintainer: xLaugh <70723697+xLaugh@users.noreply.github.com>
Installed-Size: $SIZE
Depends: libc6, libgtk-3-0 | libgtk-3-0t64, libasound2 | libasound2t64, libxkbcommon0, libgl1, ffmpeg
Section: video
Priority: optional
Homepage: https://github.com/xLaugh/BlueEdit
Description: Track-based video editor
 Blue Edit is a multi-track video editor written in Rust: timeline with
 envelopes and keyframes, transitions, effects, colour grading, audio mixer,
 scopes, subtitles and rendering with ffmpeg.
CONTROL
  dpkg-deb --root-owner-group --build "$DEB" "dist/linux/${APPID}_${VERSION}_${DEBARCH}.deb"
  rm -rf "$DEB"
  echo "==> dist/linux/${APPID}_${VERSION}_${DEBARCH}.deb"
fi

# AppImage
TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [ -n "$TOOL" ]; then
  APPDIR="dist/linux/BlueEdit.AppDir"
  rm -rf "$APPDIR"; cp -r "$OUT" "$APPDIR"; rm -f "$APPDIR/install.sh"
  cp "$APPDIR/usr/share/applications/$APPID.desktop" "$APPDIR/"
  cp "$APPDIR/usr/share/icons/hicolor/256x256/apps/$APPID.png" "$APPDIR/$APPID.png"
  printf '#!/bin/sh\nHERE="$(dirname "$(readlink -f "$0")")"\nexec "$HERE/usr/bin/%s" "$@"\n' "$BIN" > "$APPDIR/AppRun"
  chmod +x "$APPDIR/AppRun"
  ARCH="$ARCH" "$TOOL" "$APPDIR" "dist/linux/BlueEdit-$VERSION-$ARCH.AppImage"
  rm -rf "$APPDIR"
  echo "==> dist/linux/BlueEdit-$VERSION-$ARCH.AppImage"
fi
rm -rf "$OUT"
