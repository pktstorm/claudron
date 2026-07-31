#!/usr/bin/env python3
"""Apply the macOS icon shape to full-bleed artwork, then regenerate the icon set.

macOS does NOT round app icon corners for you -- unlike iOS, the shape is baked
into the .icns. Feeding `tauri icon` a full-bleed square therefore produces a
hard-edged square in the Dock. This applies Apple's squircle mask and the
standard inset so the icon sits correctly beside other Dock icons.

    python3 scripts/make-icon.py ~/Downloads/claudron-icon-rgba.png
    yarn tauri icon ./.icon-masked.png
    rm -rf src-tauri/icons/android src-tauri/icons/ios   # macOS-only app
"""
import sys
from PIL import Image, ImageDraw

CANVAS = 1024
INSET_RATIO = 0.824    # Apple's art occupies ~82.4% of the canvas
RADIUS_RATIO = 0.2237  # corner radius as a fraction of the ART size
SUPERSAMPLE = 4        # mask is drawn large then downsampled for clean edges


def main(src: str, out: str = ".icon-masked.png") -> None:
    art_size = int(CANVAS * INSET_RATIO)
    radius = int(art_size * RADIUS_RATIO)

    art = Image.open(src).convert("RGBA").resize((art_size, art_size), Image.LANCZOS)

    big = Image.new("L", (art_size * SUPERSAMPLE, art_size * SUPERSAMPLE), 0)
    ImageDraw.Draw(big).rounded_rectangle(
        [0, 0, art_size * SUPERSAMPLE - 1, art_size * SUPERSAMPLE - 1],
        radius=radius * SUPERSAMPLE,
        fill=255,
    )
    art.putalpha(big.resize((art_size, art_size), Image.LANCZOS))

    canvas = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    offset = (CANVAS - art_size) // 2
    canvas.paste(art, (offset, offset), art)
    canvas.save(out)
    print(f"wrote {out}: {CANVAS}x{CANVAS}, art {art_size}px, radius {radius}px")


if __name__ == "__main__":
    if len(sys.argv) < 2:
        sys.exit("usage: make-icon.py <full-bleed-artwork.png> [output.png]")
    main(*sys.argv[1:3])
