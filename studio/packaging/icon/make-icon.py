#!/usr/bin/env python3
"""Regenerate the placeholder app icon (studio/packaging/icon/icon-1024.png).

Tree glyph in accent #5B9CF6 on #1B1C1F, on the macOS icon grid (824 px
rounded square inside a 1024 canvas). Needs Pillow. Run from anywhere:

    python3 studio/packaging/icon/make-icon.py

The .icns (macOS) is built from this PNG on the runner by
`studio/packaging/macos/bundle.sh` with `sips` + `iconutil`.
"""

from pathlib import Path

from PIL import Image, ImageDraw

SS = 4  # supersampling factor for anti-aliasing
N = 1024 * SS
BG = (0x1B, 0x1C, 0x1F, 255)
EDGE = (0x2A, 0x2C, 0x31, 255)
ACCENT = (0x5B, 0x9C, 0xF6, 255)


def s(v: float) -> int:
    return int(v * SS)


def main() -> None:
    img = Image.new("RGBA", (N, N), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)

    # Rounded square, macOS grid: 100 px inset, ~185 px radius.
    d.rounded_rectangle((s(100), s(100), s(924), s(924)), radius=s(185), fill=EDGE)
    d.rounded_rectangle((s(106), s(106), s(918), s(918)), radius=s(180), fill=BG)

    # Node tree: root, two children, three grandchildren (hollow leaves).
    root = (512, 290, 72)
    kids = [(330, 540, 60), (694, 540, 60)]
    leaves = [(330, 770, 48), (600, 770, 48), (788, 770, 48)]
    edges = [(root, kids[0]), (root, kids[1]), (kids[0], leaves[0]), (kids[1], leaves[1]), (kids[1], leaves[2])]
    for (ax, ay, _), (bx, by, _) in edges:
        d.line((s(ax), s(ay), s(bx), s(by)), fill=ACCENT, width=s(30))
    for x, y, r in [root, *kids]:
        d.ellipse((s(x - r), s(y - r), s(x + r), s(y + r)), fill=ACCENT)
    for x, y, r in leaves:
        d.ellipse((s(x - r), s(y - r), s(x + r), s(y + r)), fill=BG, outline=ACCENT, width=s(22))

    out = Path(__file__).with_name("icon-1024.png")
    img.resize((1024, 1024), Image.LANCZOS).save(out, optimize=True)
    print(f"wrote {out} ({out.stat().st_size} bytes)")


if __name__ == "__main__":
    main()
