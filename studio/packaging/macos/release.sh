#!/usr/bin/env bash
# Local macOS release: universal build → sign → notarize → .dmg → verify → draft GitHub Release.
# Signing uses the Developer ID identity in the login keychain; notarization the notarytool
# keychain profile (see README.md). No credentials leave this Mac.
#
#   release.sh [--no-notarize] [--no-upload] [--skip-build]
#
#   --no-notarize   sign only (quick local build; Gatekeeper will still block it elsewhere)
#   --no-upload     stop after dist/, no tag, no release
#   --skip-build    reuse target/*/release/tiptap-studio from a previous run
#
# Upload: creates tag studio-v<version> at HEAD if missing (pushed), then creates the draft
# release or adds the .dmg to the existing one. The tag push also triggers the Windows/Linux
# workflow, which attaches its archives to the same draft. Publish the draft on GitHub.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../../.." && pwd)"
BUNDLE="$HERE/bundle.sh"
TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
export MACOSX_DEPLOYMENT_TARGET="${MACOSX_DEPLOYMENT_TARGET:-10.15.7}"

notarize=true upload=true build=true
for a in "$@"; do
  case "$a" in
    --no-notarize) notarize=false ;;
    --no-upload) upload=false ;;
    --skip-build) build=false ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "release.sh: unknown option $a" >&2; exit 2 ;;
  esac
done

die() { echo "release.sh: $*" >&2; exit 1; }
log() { echo "==> $*" >&2; }

cd "$ROOT"
[ "$(uname -s)" = Darwin ] || die "macOS only"
VERSION="$("$BUNDLE" version)"
TAG="studio-v$VERSION"
APP="dist/Tiptap Studio.app"
suffix=""; $notarize || suffix="-signed-not-notarized"
DMG="dist/TiptapStudio-$VERSION-macos-universal$suffix.dmg"

if $upload; then
  command -v gh >/dev/null || die "gh not installed"
  [ -z "$(git status --porcelain)" ] || die "working tree not clean; commit first"
fi
# Fail early, before a long build, when credentials are missing.
APPLE_SIGNING_IDENTITY="${APPLE_SIGNING_IDENTITY:-$(security find-identity -v -p codesigning \
  | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -n1)}"
[ -n "$APPLE_SIGNING_IDENTITY" ] || die "no 'Developer ID Application' identity in the keychain"
export APPLE_SIGNING_IDENTITY
log "Tiptap Studio $VERSION, signing as: $APPLE_SIGNING_IDENTITY"

if $build; then
  for t in "${TARGETS[@]}"; do rustup target list --installed | grep -qx "$t" || rustup target add "$t"; done
  log "building release (${TARGETS[*]})"
  args=(); for t in "${TARGETS[@]}"; do args+=(--target "$t"); done
  cargo build --release --locked -p tiptap-rusty-studio "${args[@]}"
fi
bins=(); for t in "${TARGETS[@]}"; do bins+=("target/$t/release/tiptap-studio"); done

mkdir -p dist
"$BUNDLE" app dist "${bins[@]}" >/dev/null
"$BUNDLE" sign "$APP"
if $notarize; then "$BUNDLE" notarize "$APP"; fi
"$BUNDLE" dmg "$APP" "$DMG"
if $notarize; then
  "$BUNDLE" notarize "$DMG"
  "$BUNDLE" verify "$APP" "$DMG"
else
  codesign --verify --strict --verbose=2 "$DMG"
fi
"$BUNDLE" cleanup
(cd dist && shasum -a 256 "$(basename "$DMG")" >"$(basename "$DMG").sha256")
log "built $DMG"

$upload || exit 0
$notarize || die "refusing to upload a build that is not notarized (drop --no-notarize)"

if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
  [ "$(git rev-parse "$TAG^{commit}")" = "$(git rev-parse HEAD)" ] || die "tag $TAG exists but is not at HEAD"
else
  log "tagging $TAG"
  git tag -a "$TAG" -m "tiptap-studio $VERSION"
fi
git ls-remote --exit-code --tags origin "refs/tags/$TAG" >/dev/null 2>&1 || git push origin "$TAG"

pre=(); case "$VERSION" in *-*) pre=(--prerelease) ;; esac
if gh release view "$TAG" >/dev/null 2>&1; then
  log "adding to existing release $TAG"
  gh release upload "$TAG" "$DMG" "$DMG.sha256" --clobber
else
  log "creating draft release $TAG"
  gh release create "$TAG" "$DMG" "$DMG.sha256" --draft "${pre[@]}" \
    --title "Tiptap Studio $VERSION" --notes-file "$HERE/../release-notes.md"
fi
log "done: $(gh release view "$TAG" --json url -q .url)"
