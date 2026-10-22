#!/usr/bin/env bash
# Builds a .deb from a release binary, for the Ubuntu (or Debian) release it
# runs on: the library versions in Depends come from this system.
# Usage: scripts/package-deb.sh [path/to/squirreldisk] [output.deb]
set -euo pipefail
umask 022
BIN="${1:-target/release/squirreldisk}"
VERSION="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
ARCH="$(dpkg --print-architecture)"
OUT="${2:-target/squirreldisk_${VERSION}_${ARCH}.deb}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
# inside the smallest source package tree, which dpkg-shlibdeps needs
ROOT="$WORK/debian/squirreldisk"
mkdir -p "$ROOT/DEBIAN"
printf 'Source: squirreldisk\n\nPackage: squirreldisk\nArchitecture: any\n' > "$WORK/debian/control"

install -Dm755 "$BIN" "$ROOT/usr/bin/squirreldisk"
install -Dm644 packaging/linux/squirreldisk.desktop "$ROOT/usr/share/applications/squirreldisk.desktop"
for s in 16 32 48 64 128 256 512; do
  install -Dm644 "assets/icon/icon-$s.png" "$ROOT/usr/share/icons/hicolor/${s}x${s}/apps/squirreldisk.png"
done
DOC="$ROOT/usr/share/doc/squirreldisk"
install -Dm644 NOTICE.md "$DOC/NOTICE.md"
# AGPL-3.0 isn't in /usr/share/common-licenses, so the full text goes here
{
  cat <<EOF
Format: https://www.debian.org/doc/packaging-manuals/copyright-format/1.0/
Upstream-Name: SquirrelDisk
Source: https://github.com/Randy420Marsh/squirreldisk
Comment: A modified version of https://github.com/adileo/squirreldisk.
 NOTICE.md in this folder lists the changes.

Files: *
Copyright: Adileo Barone and contributors
License: AGPL-3.0-only

License: AGPL-3.0-only
EOF
  sed -e 's/^$/./' -e 's/^/ /' LICENSE
} > "$DOC/copyright"
chmod 644 "$DOC/copyright"

# Linked libraries (ALSA, libc)
SHLIBS="$(cd "$WORK" && dpkg-shlibdeps -O debian/squirreldisk/usr/bin/squirreldisk | sed -n 's/^shlibs:Depends=//p')"

# Loaded at run time with dlopen, so dpkg-shlibdeps can't see them: OpenGL
# (EGL on Wayland, GLX or EGL on X11) and xkbcommon are always needed, the
# rest for one display server or the other.
cat > "$ROOT/DEBIAN/control" <<EOF
Package: squirreldisk
Version: $VERSION
Architecture: $ARCH
Maintainer: Randy420Marsh <https://github.com/Randy420Marsh/squirreldisk/issues>
Installed-Size: $(du -sk "$ROOT/usr" | cut -f1)
Depends: $SHLIBS, libxkbcommon0, libegl1 | libgl1
Recommends: libxkbcommon-x11-0, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxrender1, libwayland-client0, libwayland-egl1, libdbus-1-3, xdg-utils, openssh-client
Suggests: rclone
Section: utils
Priority: optional
Homepage: https://github.com/Randy420Marsh/squirreldisk
Description: disk usage analyzer with an animated sunburst
 SquirrelDisk scans a disk, a folder, a server over SSH or cloud storage
 through rclone and shows what takes the space, as a sunburst or a treemap.
 It finds duplicate files and can move items to the trash, delete them or
 back them up.
 .
 This build makes no network requests of its own.
EOF

mkdir -p "$(dirname "$OUT")"
dpkg-deb --build --root-owner-group "$ROOT" "$OUT" >/dev/null
echo "$OUT"
