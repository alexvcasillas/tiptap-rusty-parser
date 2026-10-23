Tiptap Studio: desktop inspector, structural editor and differ for Tiptap/ProseMirror JSON.

- macOS (universal, 10.15.7+): `.dmg`, Developer ID signed and notarized.
- Windows x86_64: `.zip`, unsigned exe (SmartScreen: More info → Run anyway).
- Linux x86_64: `.tar.gz` (needs Vulkan, libxkbcommon, fontconfig).

Verify: `shasum -a 256 -c <file>.sha256`; on macOS `spctl -a -vv "/Applications/Tiptap Studio.app"`.
