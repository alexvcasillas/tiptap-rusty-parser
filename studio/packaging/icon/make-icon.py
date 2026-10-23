#!/usr/bin/env python3
"""Render the app icon from studio/assets/tiptap-logo.svg.

White logo on the dark rounded square of the macOS icon grid (824 px inside a
1024 canvas). Writes icon-1024.png (macOS .icns + Linux) and icon.ico (Windows,
embedded by studio/build.rs). Needs rsvg-convert (brew install librsvg):

    python3 studio/packaging/icon/make-icon.py
"""

import re
import struct
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
LOGO = HERE.parent.parent / "assets" / "tiptap-logo.svg"
BG = "#1B1C1F"
EDGE = "#2A2C31"
LOGO_SIZE = 560  # logo box inside the 1024 canvas
ICO_SIZES = (16, 24, 32, 48, 64, 128, 256)


def composed_svg() -> str:
    paths = re.sub(r'fill="black"', 'fill="white"', LOGO.read_text())
    paths = re.sub(r"^<svg[^>]*>|</svg>\s*$", "", paths.strip())
    scale = LOGO_SIZE / 410
    offset = (1024 - LOGO_SIZE) / 2
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" width="1024" height="1024" viewBox="0 0 1024 1024">'
        f'<rect x="100" y="100" width="824" height="824" rx="185" fill="{EDGE}"/>'
        f'<rect x="106" y="106" width="812" height="812" rx="180" fill="{BG}"/>'
        f'<g transform="translate({offset} {offset}) scale({scale})">{paths}</g></svg>'
    )


def render(svg: Path, size: int, out: Path) -> None:
    subprocess.run(["rsvg-convert", "-w", str(size), "-h", str(size), "-o", str(out), str(svg)], check=True)


def write_ico(pngs: list[tuple[int, bytes]], out: Path) -> None:
    # ICONDIR + one ICONDIRENTRY per PNG-compressed image (Vista+ format).
    header = struct.pack("<HHH", 0, 1, len(pngs))
    entries, data, offset = b"", b"", 6 + 16 * len(pngs)
    for size, png in pngs:
        dim = 0 if size >= 256 else size
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), offset)
        data += png
        offset += len(png)
    out.write_bytes(header + entries + data)


def main() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        svg = Path(tmp) / "icon.svg"
        svg.write_text(composed_svg())
        render(svg, 1024, HERE / "icon-1024.png")
        pngs = []
        for s in ICO_SIZES:
            p = Path(tmp) / f"{s}.png"
            render(svg, s, p)
            pngs.append((s, p.read_bytes()))
        write_ico(pngs, HERE / "icon.ico")
    print("wrote icon-1024.png and icon.ico")


if __name__ == "__main__":
    main()
