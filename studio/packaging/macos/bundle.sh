#!/usr/bin/env bash
# Build, sign, notarize and package "Tiptap Studio.app". One subcommand per step;
# release.sh runs them in order. macOS builds are made locally (see README.md).
#
#   bundle.sh version                       print studio/Cargo.toml version
#   bundle.sh app <out-dir> <bin>...        .app; >1 binary → universal via lipo
#   bundle.sh sign <app>                    codesign binary, then bundle (hardened runtime)
#   bundle.sh notarize <app|dmg|zip>        notarytool submit --wait, then staple
#   bundle.sh dmg <app> <out.dmg>           compressed dmg with /Applications link (signed if identity set)
#   bundle.sh zip <app> <out.zip>           ditto zip (keeps xattrs, symlinks)
#   bundle.sh verify <app> [dmg]            codesign --verify, spctl, stapler validate
#   bundle.sh cleanup                       delete a decoded API key, if any
#
# Env: APPLE_SIGNING_IDENTITY (default: first "Developer ID Application" identity in the
# keychain). Notarization, first match wins: APPLE_KEYCHAIN_PROFILE (a `notarytool
# store-credentials` profile; default `tiptap-studio` when it exists), or APPLE_API_KEY_ID +
# APPLE_API_ISSUER + APPLE_API_KEY_P8 (base64 .p8), or APPLE_ID + APPLE_TEAM_ID +
# APPLE_APP_PASSWORD. Nothing secret is echoed.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STUDIO="$(cd "$HERE/../.." && pwd)"
APP_NAME="Tiptap Studio"
EXE="tiptap-studio"
ENTITLEMENTS="$HERE/entitlements.plist"
ICON_PNG="$STUDIO/packaging/icon/icon-1024.png"
TMP_ROOT="${TMPDIR:-/tmp}"
API_KEY_DIR="$TMP_ROOT/tiptap-studio-notary"
DEFAULT_PROFILE="tiptap-studio"

die() { echo "bundle.sh: $*" >&2; exit 1; }
log() { echo "==> $*" >&2; }
is_mac() { [ "$(uname -s)" = Darwin ]; }

version() {
  local v
  v="$(sed -n 's/^version *= *"\([^"]*\)".*/\1/p' "$STUDIO/Cargo.toml" | head -n1)"
  [ -n "$v" ] || die "no version in $STUDIO/Cargo.toml"
  echo "$v"
}

cmd_app() {
  [ $# -ge 2 ] || die "usage: app <out-dir> <binary>..."
  local out="$1"; shift
  local v bundle_v app
  v="${VERSION:-$(version)}"
  bundle_v="${v%%[-+]*}" # CFBundleVersion must be dotted digits
  app="$out/$APP_NAME.app"
  for b in "$@"; do [ -f "$b" ] || die "missing binary: $b"; done

  log "bundling $app ($v)"
  rm -rf "$app"
  mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
  if [ $# -gt 1 ]; then
    lipo -create -output "$app/Contents/MacOS/$EXE" "$@"
  else
    cp "$1" "$app/Contents/MacOS/$EXE"
  fi
  chmod 755 "$app/Contents/MacOS/$EXE"
  if command -v lipo >/dev/null; then lipo -info "$app/Contents/MacOS/$EXE" >&2; fi

  sed -e "s/@VERSION@/$v/g" -e "s/@BUNDLE_VERSION@/$bundle_v/g" \
    "$HERE/Info.plist" >"$app/Contents/Info.plist"
  printf 'APPL????' >"$app/Contents/PkgInfo"
  if command -v plutil >/dev/null; then plutil -lint "$app/Contents/Info.plist" >&2; fi

  if command -v iconutil >/dev/null && command -v sips >/dev/null; then
    local iconset="$TMP_ROOT/AppIcon.iconset" s
    rm -rf "$iconset"; mkdir -p "$iconset"
    for s in 16 32 128 256 512; do
      sips -z "$s" "$s" "$ICON_PNG" --out "$iconset/icon_${s}x${s}.png" >/dev/null
      sips -z $((s * 2)) $((s * 2)) "$ICON_PNG" --out "$iconset/icon_${s}x${s}@2x.png" >/dev/null
    done
    iconutil -c icns -o "$app/Contents/Resources/AppIcon.icns" "$iconset"
    rm -rf "$iconset"
  elif is_mac; then
    die "iconutil/sips not found"
  else
    log "not macOS: skipping .icns (dry run)"
  fi
  echo "$app"
}

# Resolve APPLE_SIGNING_IDENTITY: explicit, else the first Developer ID identity in the keychain.
signing_identity() {
  if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
    APPLE_SIGNING_IDENTITY="$(security find-identity -v -p codesigning \
      | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -n1)"
    [ -n "$APPLE_SIGNING_IDENTITY" ] || die "no 'Developer ID Application' identity in the keychain (set APPLE_SIGNING_IDENTITY)"
  fi
  export APPLE_SIGNING_IDENTITY
}

codesign_one() {
  codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" \
    --sign "$APPLE_SIGNING_IDENTITY" "$1"
}

cmd_sign() {
  [ $# -eq 1 ] || die "usage: sign <app>"
  signing_identity
  local app="$1"
  log "signing $app as: $APPLE_SIGNING_IDENTITY"
  # Inside-out: the executable first, then the bundle (no --deep on sign; it is deprecated).
  codesign_one "$app/Contents/MacOS/$EXE"
  codesign_one "$app"
  codesign --verify --deep --strict --verbose=2 "$app"
  codesign --display --verbose=2 "$app" 2>&1 | grep -E 'Authority|TeamIdentifier|flags' >&2 || true
}

# Sets the NOTARY_AUTH array from whichever credential set is present.
notary_auth() {
  local profile="${APPLE_KEYCHAIN_PROFILE:-}"
  if [ -z "$profile" ] && [ -z "${APPLE_API_KEY_ID:-}" ] && [ -z "${APPLE_ID:-}" ] \
    && security find-generic-password -s "com.apple.gke.notary.tool" -a "$DEFAULT_PROFILE" >/dev/null 2>&1; then
    profile="$DEFAULT_PROFILE"
  fi
  if [ -n "$profile" ]; then
    NOTARY_AUTH=(--keychain-profile "$profile")
    log "notary auth: keychain profile '$profile'"
  elif [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY_P8:-}" ]; then
    mkdir -p "$API_KEY_DIR"; chmod 700 "$API_KEY_DIR"
    local key="$API_KEY_DIR/AuthKey_${APPLE_API_KEY_ID}.p8"
    printf '%s' "$APPLE_API_KEY_P8" | base64 --decode >"$key"
    chmod 600 "$key"
    NOTARY_AUTH=(--key "$key" --key-id "$APPLE_API_KEY_ID" --issuer "$APPLE_API_ISSUER")
    log "notary auth: App Store Connect API key"
  elif [ -n "${APPLE_ID:-}" ] && [ -n "${APPLE_TEAM_ID:-}" ] && [ -n "${APPLE_APP_PASSWORD:-}" ]; then
    NOTARY_AUTH=(--apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD")
    log "notary auth: Apple ID (app-specific password)"
  else
    die "no notarization credentials: run 'xcrun notarytool store-credentials $DEFAULT_PROFILE' (see README.md)"
  fi
}

json_field() { python3 -I -c 'import json,sys; print(json.load(sys.stdin).get(sys.argv[1], ""))' "$1"; }

cmd_notarize() {
  [ $# -eq 1 ] || die "usage: notarize <app|dmg|zip>"
  local target="$1" upload result rc id status
  notary_auth
  upload="$target"
  if [ "${target%.app}" != "$target" ]; then
    upload="$TMP_ROOT/$(basename "$target" .app)-notarize.zip"
    ditto -c -k --sequesterRsrc --keepParent "$target" "$upload"
  fi
  log "submitting $(basename "$upload") to the notary service (waits)"
  set +e
  result="$(xcrun notarytool submit "$upload" "${NOTARY_AUTH[@]}" --wait --timeout 45m --output-format json)"
  rc=$?
  set -e
  echo "$result" >&2
  id="$(printf '%s' "$result" | json_field id || true)"
  status="$(printf '%s' "$result" | json_field status || true)"
  if [ "$rc" -ne 0 ] || [ "$status" != "Accepted" ]; then
    if [ -n "$id" ]; then xcrun notarytool log "$id" "${NOTARY_AUTH[@]}" >&2 || true; fi
    die "notarization failed (status: ${status:-unknown}, exit $rc)"
  fi
  [ "$upload" = "$target" ] || rm -f "$upload"
  log "stapling $target"
  xcrun stapler staple "$target"
  xcrun stapler validate "$target"
}

cmd_dmg() {
  [ $# -eq 2 ] || die "usage: dmg <app> <out.dmg>"
  local app="$1" dmg="$2" stage
  stage="$(mktemp -d "$TMP_ROOT/tiptap-dmg.XXXXXX")"
  ditto "$app" "$stage/$APP_NAME.app"
  ln -s /Applications "$stage/Applications"
  log "creating $dmg"
  rm -f "$dmg"
  hdiutil create -volname "$APP_NAME" -srcfolder "$stage" -fs HFS+ -format UDZO -ov "$dmg" >&2
  rm -rf "$stage"
  if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
    codesign --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg"
  fi
}

cmd_zip() {
  [ $# -eq 2 ] || die "usage: zip <app> <out.zip>"
  rm -f "$2"
  ditto -c -k --sequesterRsrc --keepParent "$1" "$2"
}

cmd_verify() {
  [ $# -ge 1 ] || die "usage: verify <app> [dmg]"
  local app="$1" dmg="${2:-}"
  codesign --verify --deep --strict --verbose=2 "$app"
  spctl -a -t exec -vv "$app"
  xcrun stapler validate "$app"
  if [ -n "$dmg" ]; then
    codesign --verify --strict --verbose=2 "$dmg"
    spctl -a -t open --context context:primary-signature -vv "$dmg"
    xcrun stapler validate "$dmg"
  fi
}

cmd_cleanup() { rm -rf "$API_KEY_DIR"; }

[ $# -ge 1 ] || die "usage: bundle.sh <version|app|sign|notarize|dmg|zip|verify|cleanup> ..."
sub="$1"; shift
case "$sub" in
  version) version ;;
  app) cmd_app "$@" ;;
  sign) cmd_sign "$@" ;;
  notarize) cmd_notarize "$@" ;;
  dmg) cmd_dmg "$@" ;;
  zip) cmd_zip "$@" ;;
  verify) cmd_verify "$@" ;;
  cleanup) cmd_cleanup ;;
  *) die "unknown subcommand: $sub" ;;
esac
