#!/usr/bin/env bash
# Packs a built Linux launcher as a .tar.gz and an AppImage.
#
#   package.sh <version> <launcher binary> <output folder>
#
# The AppImage needs two pinned downloads, both checked against their SHA-256 before use:
# appimagetool and the type 2 runtime it puts in front of the image. Their versions and hashes
# are set here; to update them, change all three lines together.
set -euo pipefail

APPIMAGETOOL_VERSION=1.9.1
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_VERSION=20251108
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

version=${1:?version}
binary=$(realpath "${2:?launcher binary}")
out=$(mkdir -p "${3:?output folder}" && realpath "$3")
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../.." && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

fetch() { # url sha256 file
  curl -fsSL --retry 3 -o "$3" "$1"
  echo "$2  $3" | sha256sum -c -
}

name="AnyPS5-Launcher-$version-linux-x86_64"

# 1. The tarball: the binary, the desktop entry and the icon, in one folder.
folder="$work/$name"
mkdir -p "$folder"
install -m 755 "$binary" "$folder/anyps5-launcher"
install -m 644 "$here/anyps5-launcher.desktop" "$here/anyps5-launcher.png" "$root/LICENSE" "$folder/"
tar -C "$work" --owner=0 --group=0 -czf "$out/$name.tar.gz" "$name"

# 2. The AppImage.
fetch "https://github.com/AppImage/appimagetool/releases/download/$APPIMAGETOOL_VERSION/appimagetool-x86_64.AppImage" \
  "$APPIMAGETOOL_SHA256" "$work/appimagetool"
fetch "https://github.com/AppImage/type2-runtime/releases/download/$RUNTIME_VERSION/runtime-x86_64" \
  "$RUNTIME_SHA256" "$work/runtime-x86_64"
chmod +x "$work/appimagetool"

appdir="$work/AnyPS5.AppDir"
mkdir -p "$appdir/usr/bin"
install -m 755 "$binary" "$appdir/usr/bin/anyps5-launcher"
install -m 644 "$here/anyps5-launcher.desktop" "$here/anyps5-launcher.png" "$appdir/"
ln -s anyps5-launcher.png "$appdir/.DirIcon"
cat > "$appdir/AppRun" <<'RUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/bin/anyps5-launcher" "$@"
RUN
chmod 755 "$appdir/AppRun"

# appimagetool is an AppImage itself; extract-and-run avoids needing FUSE on the build machine.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$work/appimagetool" --no-appstream \
  --runtime-file "$work/runtime-x86_64" "$appdir" "$out/AnyPS5-Launcher-$version-x86_64.AppImage"
chmod 755 "$out/AnyPS5-Launcher-$version-x86_64.AppImage"
ls -l "$out"
