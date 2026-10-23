#!/usr/bin/env bash
# Package a Linux release tarball: binary + desktop entry + icon + README.
#   package.sh <binary> <out-dir> [version]   → <out-dir>/TiptapStudio-<v>-linux-<arch>.tar.gz
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKG="$(cd "$HERE/.." && pwd)"
[ $# -ge 2 ] || { echo "usage: package.sh <binary> <out-dir> [version]" >&2; exit 1; }
bin="$1"; out="$2"
v="${3:-$("$PKG/macos/bundle.sh" version)}"
arch="$(uname -m)"
name="TiptapStudio-$v-linux-$arch"
[ -f "$bin" ] || { echo "missing binary: $bin" >&2; exit 1; }

mkdir -p "$out"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
mkdir "$stage/$name"
install -m 755 "$bin" "$stage/$name/tiptap-studio"
install -m 644 "$HERE/tiptap-studio.desktop" "$stage/$name/"
install -m 644 "$PKG/icon/icon-1024.png" "$stage/$name/tiptap-studio.png"
install -m 644 "$PKG/README.dist.txt" "$stage/$name/README.txt"
tar -C "$stage" -czf "$out/$name.tar.gz" "$name"
echo "$out/$name.tar.gz"
