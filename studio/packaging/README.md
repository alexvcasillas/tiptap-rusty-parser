# Packaging and releases

macOS is built, signed and notarized **locally** with [`macos/release.sh`](macos/release.sh): the Developer ID key and the notarization credentials stay in the login keychain of the release Mac, nothing is stored in GitHub. Windows and Linux (no signing) come from [`.github/workflows/studio-release.yml`](../../.github/workflows/studio-release.yml), which the tag push triggers.

| Build | Where | Output |
| --- | --- | --- |
| macOS universal (arm64 + x86_64, `lipo`) | local Mac | `TiptapStudio-<v>-macos-universal.dmg`, Developer ID signed, notarized, stapled |
| Windows x86_64 (MSVC) | `windows-2022` runner | `TiptapStudio-<v>-windows-x86_64.zip` (unsigned `.exe`, GUI subsystem) |
| Linux x86_64 | `ubuntu-22.04` runner | `TiptapStudio-<v>-linux-x86_64.tar.gz` (binary, `.desktop`, icon); smoke-run 10 s under Xvfb |

Every file gets a `<file>.sha256` next to it. The three builds land in one **draft** GitHub Release under the `studio-v<version>` tag; publish it by hand.

## Files

| Path | What |
| --- | --- |
| `macos/release.sh` | the whole macOS release: build → sign → notarize → dmg → verify → tag → draft release |
| `macos/bundle.sh` | the steps: `version`, `app`, `sign`, `notarize`, `dmg`, `zip`, `verify`, `cleanup` (bash 3.2 compatible) |
| `macos/Info.plist` | bundle template: id `dev.tiptap.studio`, name "Tiptap Studio", `@VERSION@` from `studio/Cargo.toml`, macOS 10.15.7+ (gpui-pre's Metal minimum), Retina, opens `.json` as Viewer |
| `macos/entitlements.plist` | hardened runtime, intentionally empty (no sandbox, no JIT) |
| `linux/package.sh`, `linux/tiptap-studio.desktop` | Linux tarball |
| `icon/icon-1024.png`, `icon/make-icon.py` | placeholder icon (tree glyph, `#5B9CF6` on `#1B1C1F`); `.icns` is built with `sips` + `iconutil` |
| `release-notes.md` | body of the draft release (used by the script and the workflow) |
| `README.dist.txt` | README shipped inside every archive |

## One-time setup on the release Mac

1. Xcode command line tools (`xcrun notarytool`, `codesign`, `stapler`), `rustup`, `gh` logged in (`gh auth login`).
2. A **Developer ID Application** certificate with its private key in the login keychain (Keychain Access → My Certificates; it must expand to show the key). Only the Account Holder can create one: developer.apple.com → Certificates → **+** → Developer ID Application, or import the `.p12` backup. Check: `security find-identity -v -p codesigning`.
3. Notarization credentials stored in the keychain under the profile name `tiptap-studio` (what `bundle.sh` looks for by default):
   ```sh
   # App Store Connect API key (appstoreconnect.apple.com → Users and Access → Integrations →
   # App Store Connect API → Team Keys → +, role Developer; the .p8 downloads once):
   xcrun notarytool store-credentials tiptap-studio --key AuthKey_<KEYID>.p8 --key-id <KEYID> --issuer <ISSUER-UUID>
   # or an Apple ID + app-specific password (account.apple.com → App-Specific Passwords):
   xcrun notarytool store-credentials tiptap-studio --apple-id <email> --team-id <TEAMID>
   ```
   Delete the `.p8` file afterwards; keep a copy in the password manager. Check: `xcrun notarytool history --keychain-profile tiptap-studio`.

Back up the certificate (`.p12` export) and the API key in the password manager: without the certificate nothing can be signed until the Account Holder issues a new one.

## Cut a release

1. Bump `version` in `studio/Cargo.toml` (e.g. `0.2.0`; a `-beta.1` suffix marks the release as a pre-release). Commit and push the branch.
2. On the release Mac, from a clean tree:
   ```sh
   studio/packaging/macos/release.sh
   ```
   It builds both targets (`--locked`), bundles, signs with the hardened runtime, notarizes the `.app` (waits for Apple), builds and notarizes the `.dmg`, verifies (`codesign`, `spctl`, `stapler`), then tags `studio-v<version>` at HEAD, pushes the tag and creates the draft release with the `.dmg`. If the draft already exists it adds the file to it.
3. The tag push runs the workflow; its Windows and Linux archives join the same draft. The `version` job fails if the tag does not equal `studio-v<Cargo version>`.
4. Open the draft under **Releases**, check the notes, publish.

Options: `--no-notarize` (sign only, for a quick local build; refuses to upload), `--no-upload` (stop at `dist/`), `--skip-build` (reuse the binaries).

Override the identity or profile with `APPLE_SIGNING_IDENTITY="Developer ID Application: … (TEAMID)"` and `APPLE_KEYCHAIN_PROFILE=<name>`. The `APPLE_API_*` / `APPLE_ID` env vars from the old CI setup still work as a fallback when no profile exists.

Single steps, when needed:

```sh
studio/packaging/macos/bundle.sh app dist target/release/tiptap-studio        # arm64-only dev bundle
studio/packaging/macos/bundle.sh sign "dist/Tiptap Studio.app"
studio/packaging/macos/bundle.sh dmg "dist/Tiptap Studio.app" dist/TiptapStudio.dmg
```

## Verify a download (macOS)

```sh
shasum -a 256 -c TiptapStudio-*.dmg.sha256
spctl -a -t open --context context:primary-signature -vv TiptapStudio-*.dmg   # accepted, source=Notarized Developer ID
spctl -a -vv "/Applications/Tiptap Studio.app"                                 # accepted, source=Notarized Developer ID
codesign --verify --deep --strict --verbose=2 "/Applications/Tiptap Studio.app"
xcrun stapler validate "/Applications/Tiptap Studio.app"
```

## Open a build that is not notarized (macOS)

A `--no-notarize` build (`-signed-not-notarized` in the name) is blocked by Gatekeeper on other Macs. Clear the quarantine flag once after copying it to Applications:

```sh
xattr -dr com.apple.quarantine "/Applications/Tiptap Studio.app"
```

Or right-click the app → Open → Open.

## Known gaps

- Windows `.exe` is unsigned (SmartScreen warns on first run) and has no embedded icon. No Linux AppImage.
- No auto-update. A Sparkle feed would need a hosting bucket (R2) and an appcast; not planned yet.
- `Cargo.lock` is committed and release builds use `--locked`, so every artifact is built from the same dependency versions. After changing dependencies, commit the updated lockfile.
