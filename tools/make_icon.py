"""Builds the app icon from a photo of a penny's face.

The source is the U.S. Mint's one cent obverse image (public domain):
https://commons.wikimedia.org/wiki/File:US_One_Cent_Obv.png

Run with: uv run --with pillow python tools/make_icon.py path/to/US_One_Cent_Obv.png
Writes assets/icon.png (256 px) and assets/icon.ico (16 to 256 px).
"""

import sys
from pathlib import Path

from PIL import Image

ASSETS = Path(__file__).resolve().parent.parent / "assets"
ICO_SIZES = [(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)]


def main():
    source = Image.open(sys.argv[1]).convert("RGBA")

    # Trim the transparent border so the coin fills the icon.
    coin = source.crop(source.getchannel("A").getbbox())
    side = max(coin.size)
    square = Image.new("RGBA", (side, side), (0, 0, 0, 0))
    square.paste(coin, ((side - coin.width) // 2, (side - coin.height) // 2))

    ASSETS.mkdir(exist_ok=True)
    icon = square.resize((256, 256), Image.LANCZOS)
    icon.save(ASSETS / "icon.png", optimize=True)
    icon.save(ASSETS / "icon.ico", sizes=ICO_SIZES)


if __name__ == "__main__":
    main()
