#!/usr/bin/env bash
# Builds the AppImage from a release binary, as the release workflow does.
# Build on the oldest system the AppImage should run on: it needs the glibc
# it was built against or newer (CI uses an ubuntu:22.04 container).
# Usage: scripts/build-appimage.sh [path/to/squirreldisk] [output.AppImage]
# APPIMAGETOOL=path uses that appimagetool instead of downloading one.
set -euo pipefail
umask 022
BIN="${1:-target/release/squirreldisk}"
ARCH="$(uname -m)"
OUT="${2:-target/SquirrelDisk-$ARCH.AppImage}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
TOOL="${APPIMAGETOOL:-}"
if [ -z "$TOOL" ]; then
  TOOL="$WORK/appimagetool"
  curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
  chmod +x "$TOOL"
fi

APP="$WORK/AppDir"
install -Dm755 "$BIN" "$APP/usr/bin/squirreldisk"
# winit loads xkbcommon with dlopen, so it is not a linked dependency;
# bundle it for systems without it (minimal X11 setups). AppRun only uses
# it when the system has none.
for lib in libxkbcommon.so.0 libxkbcommon-x11.so.0 libxcb-xkb.so.1; do
  install -Dm644 "$(readlink -f "/usr/lib/$ARCH-linux-gnu/$lib")" "$APP/usr/lib/$lib"
done
ln -s libxkbcommon.so.0 "$APP/usr/lib/libxkbcommon.so"
ln -s libxkbcommon-x11.so.0 "$APP/usr/lib/libxkbcommon-x11.so"
install -m755 packaging/linux/AppRun "$APP/AppRun"
install -Dm644 packaging/linux/squirreldisk.desktop "$APP/squirreldisk.desktop"
install -Dm644 packaging/linux/squirreldisk.desktop "$APP/usr/share/applications/squirreldisk.desktop"
for s in 16 32 48 64 128 256 512; do
  install -Dm644 "assets/icon/icon-$s.png" "$APP/usr/share/icons/hicolor/${s}x${s}/apps/squirreldisk.png"
done
cp assets/icon/icon-256.png "$APP/squirreldisk.png"
cp assets/icon/icon-256.png "$APP/.DirIcon"
install -Dm644 -t "$APP/usr/share/doc/squirreldisk" NOTICE.md LICENSE THIRD-PARTY-LICENSES.txt

# AppImageUpdate and Gear Lever update it from this fork's releases
ARCH="$ARCH" "$TOOL" \
  -u "gh-releases-zsync|Randy420Marsh|squirreldisk|latest|SquirrelDisk-$ARCH.AppImage.zsync" \
  "$APP" "$OUT"
# a bare name would be looked up in PATH; extract-and-run needs no FUSE
APPIMAGE_EXTRACT_AND_RUN=1 "$(realpath "$OUT")" --version
