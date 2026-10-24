#!/usr/bin/env bash
# Writes the licence file of a release: the licences of every third-party crate in the launcher
# and in the net service it carries (about.toml here and in anyplay-gg/network).
#
#   notices.sh <output file>
#
# Run it from a launcher checkout with anyplay-gg/network beside it. cargo-about is a pinned
# download, checked against its SHA-256; to update it, change both lines together. CARGO_ABOUT
# names a cargo-about binary to use instead, for a machine that is not x86-64 Linux.
set -euo pipefail

CARGO_ABOUT_VERSION=0.9.2
CARGO_ABOUT_SHA256=9099a59e820c38a68b9d65f300662a567d56562f9a10f6aa4c7e86c17c2566af

out=$(realpath -m "${1:?output file}")
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/.." && pwd)
network=$(cd "$root/../network" && pwd)
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

about=${CARGO_ABOUT:-}
if [ -z "$about" ]; then
  name="cargo-about-$CARGO_ABOUT_VERSION-x86_64-unknown-linux-musl"
  curl -fsSL --retry 3 -o "$work/about.tar.gz" \
    "https://github.com/EmbarkStudios/cargo-about/releases/download/$CARGO_ABOUT_VERSION/$name.tar.gz"
  echo "$CARGO_ABOUT_SHA256  $work/about.tar.gz" | sha256sum -c -
  tar -C "$work" -xzf "$work/about.tar.gz"
  about="$work/$name/cargo-about"
fi

# --fail stops on a crate whose licence cannot be read: a release never ships an incomplete file.
"$about" generate --format json --locked --fail --workspace \
  -c "$root/about.toml" -m "$root/Cargo.toml" -o "$work/launcher.json"
"$about" generate --format json --locked --fail \
  -c "$network/about.toml" -m "$network/crates/net/Cargo.toml" -o "$work/net.json"
mkdir -p "$(dirname "$out")"
node "$here/notices.mjs" "$out" "$work/launcher.json" "$work/net.json"
