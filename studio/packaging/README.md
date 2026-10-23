# Packaging and releases

Release builds come from one workflow, [`.github/workflows/studio-release.yml`](../../.github/workflows/studio-release.yml):

| Job | Runner | Output |
| --- | --- | --- |
| macOS universal (arm64 + x86_64, `lipo`) | `macos-15` | `TiptapStudio-<v>-macos-universal.dmg` (signed + notarized), or `…-unsigned.dmg` without secrets |
| Windows x86_64 (MSVC) | `windows-2022` | `TiptapStudio-<v>-windows-x86_64.zip` (unsigned `.exe`, GUI subsystem) |
| Linux x86_64 | `ubuntu-22.04` | `TiptapStudio-<v>-linux-x86_64.tar.gz` (binary, `.desktop`, icon); smoke-run 10 s under Xvfb |

Triggers: dispatched by `release-plz.yml` on the `studio-v*` tag it creates (publishes the GitHub Release with `SHA256SUMS.txt`), a tag pushed by hand (same), or **Run workflow** on a branch (artifacts only, 14 days).

## Files

| Path | What |
| --- | --- |
| `macos/Info.plist` | bundle template: id `dev.tiptap.studio`, name "Tiptap Studio", `@VERSION@` from `studio/Cargo.toml`, macOS 10.15.7+ (gpui-pre's Metal minimum), Retina, opens `.json` as Viewer |
| `macos/entitlements.plist` | hardened runtime, intentionally empty (no sandbox, no JIT) |
| `macos/bundle.sh` | `version`, `app`, `import-cert`, `sign`, `notarize`, `dmg`, `zip`, `verify`, `cleanup` (one per CI step; bash 3.2 compatible) |
| `linux/package.sh`, `linux/tiptap-studio.desktop` | Linux tarball |
| `icon/make-icon.py`, `icon/icon-1024.png`, `icon/icon.ico` | app icon: the Tiptap logo (`studio/assets/tiptap-logo.svg`) in white on `#1B1C1F`, rendered with `rsvg-convert`; `.icns` is built on the runner with `sips` + `iconutil`, the `.ico` is embedded by `studio/build.rs` (`winresource`) |
| `README.dist.txt` | README shipped inside every archive |

## Cut a release

Releases are automated by release-plz (`release-plz.toml`), the same flow as the crate:

1. Land conventional commits on `main`. `feat(studio): …` / `fix(studio): …` bump the studio; a parser bump bumps the studio too (path dependency). release-plz opens or refreshes the "chore: release" PR with the new `studio/Cargo.toml` version and a `studio/CHANGELOG.md` entry.
2. Merge the release PR. release-plz tags `studio-v<version>`, opens a **draft** GitHub Release with the changelog as body, and dispatches this workflow on the tag.
3. The three builds run; the `release` job attaches them plus `SHA256SUMS.txt` and **publishes** the release. The crate and npm packages go out in the same run.

Manual fallback: push a `studio-v<version>` tag that equals `studio/Cargo.toml` (the `version` job checks); the workflow creates the release itself. Pre-releases: a `-beta.1` suffix in the version.

Local macOS build (same steps as CI):

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
MACOSX_DEPLOYMENT_TARGET=10.15.7 cargo build --release -p tiptap-rusty-studio \
  --target aarch64-apple-darwin --target x86_64-apple-darwin
studio/packaging/macos/bundle.sh app dist \
  target/aarch64-apple-darwin/release/tiptap-studio target/x86_64-apple-darwin/release/tiptap-studio
APPLE_SIGNING_IDENTITY="Developer ID Application: … (TEAMID)" studio/packaging/macos/bundle.sh sign "dist/Tiptap Studio.app"
studio/packaging/macos/bundle.sh dmg "dist/Tiptap Studio.app" dist/TiptapStudio.dmg
```

## GitHub secrets (Settings → Secrets and variables → Actions)

Without `APPLE_CERTIFICATE` the macOS job still builds an unsigned `.dmg`. Signing needs the certificate pair; notarization needs **one** of the two credential sets (the API key wins when both exist). Nothing secret lives in the repo.

| Secret | Required | Value |
| --- | --- | --- |
| `APPLE_CERTIFICATE` | to sign | base64 of the Developer ID Application `.p12` |
| `APPLE_CERTIFICATE_PASSWORD` | to sign | password chosen when exporting the `.p12` |
| `APPLE_SIGNING_IDENTITY` | optional | e.g. `Developer ID Application: Tiptap GmbH (ABCDE12345)`; auto-detected from the `.p12` when unset |
| `APPLE_API_KEY_ID` | notarize (A) | App Store Connect API key ID, e.g. `2X9R4HXF34` |
| `APPLE_API_ISSUER` | notarize (A) | issuer ID (UUID) shown above the keys list |
| `APPLE_API_KEY_P8` | notarize (A) | base64 of `AuthKey_<KEYID>.p8` |
| `APPLE_ID` | notarize (B) | Apple ID email of a team member |
| `APPLE_TEAM_ID` | notarize (B) | 10-character team ID (developer.apple.com → Membership) |
| `APPLE_APP_PASSWORD` | notarize (B) | app-specific password from account.apple.com |

### Developer ID Application certificate → `APPLE_CERTIFICATE`

Only the Account Holder can create Developer ID certificates.

1. Keychain Access → Certificate Assistant → *Request a Certificate From a Certificate Authority…* → save to disk (`.certSigningRequest`).
2. developer.apple.com → Certificates, IDs & Profiles → Certificates → **+** → *Developer ID Application* (G2 Sub-CA) → upload the CSR → download and double-click the `.cer`.
3. Keychain Access → *login* → My Certificates → right-click *Developer ID Application: …* (it must expand to show its private key) → Export → `.p12`, set a strong password (→ `APPLE_CERTIFICATE_PASSWORD`).
4. `base64 -i DeveloperID.p12 | pbcopy` → paste as `APPLE_CERTIFICATE`. Delete the `.p12` file afterwards.
5. Identity string, if you want to pin it: `security find-identity -v -p codesigning`.

### App Store Connect API key (preferred) → `APPLE_API_*`

1. appstoreconnect.apple.com → Users and Access → Integrations → App Store Connect API → Team Keys → **+**, role *Developer*.
2. Note the **Key ID** (`APPLE_API_KEY_ID`) and the **Issuer ID** (`APPLE_API_ISSUER`).
3. Download `AuthKey_<KEYID>.p8` (one download only), then `base64 -i AuthKey_<KEYID>.p8 | pbcopy` → `APPLE_API_KEY_P8`.

### Apple ID fallback → `APPLE_ID`, `APPLE_TEAM_ID`, `APPLE_APP_PASSWORD`

account.apple.com → Sign-In and Security → App-Specific Passwords → generate one named `tiptap-studio-notary`.

With `gh`: `gh secret set APPLE_CERTIFICATE < cert.b64` (repeat per secret).

## What the macOS job does when signing

1. `bundle.sh import-cert`: decode the `.p12` into a temporary keychain (random password, 6 h lock), allow `codesign` without prompts, resolve the identity.
2. `bundle.sh sign`: `codesign --force --options runtime --timestamp --entitlements entitlements.plist` on `Contents/MacOS/tiptap-studio`, then on the bundle; `codesign --verify --deep --strict`.
3. `bundle.sh notarize "Tiptap Studio.app"`: zip with `ditto`, `xcrun notarytool submit --wait`, print the notary log on failure, `xcrun stapler staple` the `.app`.
4. `bundle.sh dmg`: `hdiutil` UDZO image with an `/Applications` link, signed.
5. `bundle.sh notarize <dmg>` + staple, then `bundle.sh verify`: `codesign --verify --deep --strict`, `spctl -a -vv` for the app and the dmg, `stapler validate`.
6. `bundle.sh cleanup` (always): delete the keychain and the decoded `.p8`.

## Verify a download (macOS)

```sh
shasum -a 256 -c SHA256SUMS.txt --ignore-missing
spctl -a -t open --context context:primary-signature -vv TiptapStudio-*.dmg   # accepted, source=Notarized Developer ID
spctl -a -vv "/Applications/Tiptap Studio.app"                                 # accepted, source=Notarized Developer ID
codesign --verify --deep --strict --verbose=2 "/Applications/Tiptap Studio.app"
xcrun stapler validate "/Applications/Tiptap Studio.app"
```

## Open an unsigned build (macOS)

Files named `-unsigned` are not notarized, so Gatekeeper says the app "is damaged" or "cannot be opened". Clear the quarantine flag once after copying it to Applications:

```sh
xattr -dr com.apple.quarantine "/Applications/Tiptap Studio.app"
```

Or right-click the app → Open → Open. The CLI form works from the bundle: `"/Applications/Tiptap Studio.app/Contents/MacOS/tiptap-studio" doc.json`.

## Known gaps

- Opening a `.json` from Finder launches the app, but the app reads files from `argv` only, so the document is not loaded yet (needs a gpui `on_open_urls` hook in `main.rs`).
- Windows `.exe` is unsigned. No Linux AppImage.
- `Cargo.lock` is committed and release builds use `--locked`, so every artifact is built from the same dependency versions. After changing dependencies, commit the updated lockfile.
