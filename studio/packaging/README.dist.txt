Tiptap Studio
=============

Native inspector, structural editor and differ for Tiptap/ProseMirror JSON.

  tiptap-studio                  bundled sample document
  tiptap-studio doc.json         open a file
  tiptap-studio a.json b.json    A + B for Diff mode

macOS: open the .dmg and drag Tiptap Studio to Applications. A file named
"-unsigned" is not signed or notarized; clear the quarantine flag once:
  xattr -dr com.apple.quarantine "/Applications/Tiptap Studio.app"
Command line: "/Applications/Tiptap Studio.app/Contents/MacOS/tiptap-studio" doc.json

Windows: unzip and run tiptap-studio.exe. The build is unsigned, so
SmartScreen may warn: More info > Run anyway.

Linux: extract and run ./tiptap-studio. Needs a Vulkan driver plus libxkbcommon,
libxkbcommon-x11, libxcb, libwayland-client and fontconfig (present on most desktops).
Optional desktop entry: copy tiptap-studio.desktop to ~/.local/share/applications/
and tiptap-studio.png to ~/.local/share/icons/hicolor/1024x1024/apps/.

Docs: https://tiptap-studio.alexvcasillas.dev/docs/studio
Source: https://github.com/alexvcasillas/tiptap-studio
