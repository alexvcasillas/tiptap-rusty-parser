#!/usr/bin/env bash
# Build, sign, notarize and package "Tiptap Studio.app". One subcommand per CI step.
#
#   bundle.sh version                       print studio/Cargo.toml version
#   bundle.sh app <out-dir> <bin>...        .app; >1 binary → universal via lipo
#   bundle.sh import-cert                   base64 .p12 → temp keychain (CI)
#   bundle.sh sign <app>                    codesign binary, then bundle (hardened runtime)
#   bundle.sh notarize <app|dmg|zip>        notarytool submit --wait, then staple
#   bundle.sh dmg <app> <out.dmg>           compressed dmg with /Applications link (signed if identity set)
#   bundle.sh zip <app> <out.zip>           ditto zip (keeps xattrs, symlinks)
#   bundle.sh verify <app> [dmg]            codesign --verify, spctl, stapler validate
#   bundle.sh cleanup                       delete temp keychain + API key
#
# Env: APPLE_CERTIFICATE (base64 .p12), APPLE_CERTIFICATE_PASSWORD, APPLE_SIGNING_IDENTITY
# (optional, auto-detected from the .p12), APPLE_API_KEY_ID + APPLE_API_ISSUER + APPLE_API_KEY_P8
# (base64 .p8) or APPLE_ID + APPLE_TEAM_ID + APPLE_APP_PASSWORD. Nothing secret is echoed.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
STUDIO="$(cd "$HERE/../.." && pwd)"
APP_NAME="Tiptap Studio"
EXE="tiptap-studio"
ENTITLEMENTS="$HERE/entitlements.plist"
ICON_PNG="$STUDIO/packaging/icon/icon-1024.png"
TMP_ROOT="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
KEYCHAIN="${APPLE_KEYCHAIN:-$TMP_ROOT/tiptap-studio-signing.keychain-db}"
API_KEY_DIR="$TMP_ROOT/tiptap-studio-notary"

die() { echo "bundle.sh: $*" >&2; exit 1; }
log() { echo "==> $*" >&2; }
is_mac() { [ "$(uname -s)" = Darwin ]; }
# Append KEY=VALUE to $GITHUB_ENV when running in Actions.
export_env() { if [ -n "${GITHUB_ENV:-}" ]; then echo "$1=$2" >>"$GITHUB_ENV"; fi; }

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

cmd_import_cert() {
  : "${APPLE_CERTIFICATE:?APPLE_CERTIFICATE (base64 .p12) not set}"
  : "${APPLE_CERTIFICATE_PASSWORD:?APPLE_CERTIFICATE_PASSWORD not set}"
  local kc_pass p12 identity
  kc_pass="$(openssl rand -base64 24)"
  p12="$TMP_ROOT/tiptap-studio-cert.p12"
  printf '%s' "$APPLE_CERTIFICATE" | base64 --decode >"$p12"

  log "creating temporary keychain"
  security delete-keychain "$KEYCHAIN" 2>/dev/null || true
  security create-keychain -p "$kc_pass" "$KEYCHAIN"
  security set-keychain-settings -lut 21600 "$KEYCHAIN"
  security unlock-keychain -p "$kc_pass" "$KEYCHAIN"
  security import "$p12" -k "$KEYCHAIN" -P "$APPLE_CERTIFICATE_PASSWORD" -f pkcs12 \
    -T /usr/bin/codesign -T /usr/bin/security
  rm -f "$p12"
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$kc_pass" "$KEYCHAIN" >/dev/null
  # Prepend to the search list so codesign finds the identity; keep the login keychain.
  # shellcheck disable=SC2046
  security list-keychains -d user -s "$KEYCHAIN" $(security list-keychains -d user | tr -d '"')

  identity="${APPLE_SIGNING_IDENTITY:-}"
  if [ -z "$identity" ]; then
    identity="$(security find-identity -v -p codesigning "$KEYCHAIN" \
      | sed -n 's/.*"\(Developer ID Application:[^"]*\)".*/\1/p' | head -n1)"
  fi
  [ -n "$identity" ] || die "no 'Developer ID Application' identity in the .p12"
  log "signing identity: $identity"
  export_env APPLE_SIGNING_IDENTITY "$identity"
  export_env APPLE_KEYCHAIN "$KEYCHAIN"
}

# codesign, scoped to the temp keychain when it exists (local runs use the login keychain).
# Written for bash 3.2 (macOS /bin/bash): no mapfile, no empty-array expansion.
cs() {
  if [ -e "$KEYCHAIN" ]; then codesign --keychain "$KEYCHAIN" "$@"; else codesign "$@"; fi
}

codesign_one() {
  cs --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" \
    --sign "$APPLE_SIGNING_IDENTITY" "$1"
}

cmd_sign() {
  [ $# -eq 1 ] || die "usage: sign <app>"
  : "${APPLE_SIGNING_IDENTITY:?APPLE_SIGNING_IDENTITY not set (run import-cert)}"
  local app="$1"
  log "signing $app"
  # Inside-out: the executable first, then the bundle (no --deep on sign; it is deprecated).
  codesign_one "$app/Contents/MacOS/$EXE"
  codesign_one "$app"
  codesign --verify --deep --strict --verbose=2 "$app"
  codesign --display --verbose=2 "$app" 2>&1 | grep -E 'Authority|TeamIdentifier|flags' >&2 || true
}

# Sets the NOTARY_AUTH array from whichever credential set is present.
notary_auth() {
  if [ -n "${APPLE_API_KEY_ID:-}" ] && [ -n "${APPLE_API_ISSUER:-}" ] && [ -n "${APPLE_API_KEY_P8:-}" ]; then
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
    die "no notarization credentials (API key or Apple ID set)"
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
  case "$target" in
    *.zip) log "zip archives cannot be stapled; ticket is online only" ;;
    *) staple "$target" ;;
  esac
}

# Staple with retries: the ticket lookup (CloudKit) drops connections now and then,
# and the ticket can take a moment to propagate after "Accepted".
staple() {
  local target="$1" attempt
  for attempt in 1 2 3 4 5 6; do
    log "stapling $target (attempt $attempt)"
    if xcrun stapler staple "$target"; then
      validate_staple "$target"
      return 0
    fi
    sleep $((attempt * 15))
  done
  die "stapling failed after $attempt attempts"
}

# `stapler validate` does the same online ticket lookup, so it flakes the same way.
validate_staple() {
  local target="$1" attempt
  for attempt in 1 2 3 4 5 6; do
    if xcrun stapler validate "$target"; then return 0; fi
    log "stapler validate failed for $target (attempt $attempt), retrying"
    sleep $((attempt * 15))
  done
  die "stapler validate failed after $attempt attempts"
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
    cs --force --timestamp --sign "$APPLE_SIGNING_IDENTITY" "$dmg"
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
  validate_staple "$app"
  if [ -n "$dmg" ]; then
    codesign --verify --strict --verbose=2 "$dmg"
    spctl -a -t open --context context:primary-signature -vv "$dmg"
    validate_staple "$dmg"
  fi
}

cmd_cleanup() {
  rm -rf "$API_KEY_DIR"
  if is_mac && [ -e "$KEYCHAIN" ]; then security delete-keychain "$KEYCHAIN" || true; fi
}

[ $# -ge 1 ] || die "usage: bundle.sh <version|app|import-cert|sign|notarize|dmg|zip|verify|cleanup> ..."
sub="$1"; shift
case "$sub" in
  version) version ;;
  app) cmd_app "$@" ;;
  import-cert) cmd_import_cert "$@" ;;
  sign) cmd_sign "$@" ;;
  notarize) cmd_notarize "$@" ;;
  dmg) cmd_dmg "$@" ;;
  zip) cmd_zip "$@" ;;
  verify) cmd_verify "$@" ;;
  cleanup) cmd_cleanup ;;
  *) die "unknown subcommand: $sub" ;;
esac
