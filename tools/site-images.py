"""Builds the website's derived images from docs/assets (needs Pillow).

    python tools/site-images.py

From the app icon and the screenshots it writes:
- docs/favicon.ico (16, 32, 48 px) and docs/apple-touch-icon.png (180 px)
- docs/assets/icon-52.png and icon-208.png/.webp: the icon at twice the size
  it is shown (26 px in the top bar, 104 px in the hero)
- docs/assets/<screenshot>-<width>.webp: smaller WebP copies for srcset
- docs/assets/og.png: the 1200 x 630 link-preview image

Run it again whenever docs/assets/icon.png or a screenshot changes.
"""
from pathlib import Path

from PIL import Image

ASSETS = Path(__file__).resolve().parent.parent / "docs" / "assets"
DOCS = ASSETS.parent
BG = (10, 15, 20)  # the site's night background
# Screenshot -> widths to offer in srcset (the largest is the original).
SHOTS = {"developer-junk": [640, 1100, 1710], "autopilot-focus": [640, 1100, 1710], "tray-panel": [360, 540]}


def resized(im, width):
    return im if im.width == width else im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)


def main():
    icon = Image.open(ASSETS / "icon.png").convert("RGBA")
    icon.save(DOCS / "favicon.ico", sizes=[(16, 16), (32, 32), (48, 48)])
    touch = Image.new("RGBA", (180, 180), BG + (255,))
    touch.alpha_composite(icon.resize((180, 180), Image.LANCZOS))
    touch.convert("RGB").save(DOCS / "apple-touch-icon.png", optimize=True)
    icon.resize((52, 52), Image.LANCZOS).save(ASSETS / "icon-52.png", optimize=True)
    hero = icon.resize((208, 208), Image.LANCZOS)
    hero.save(ASSETS / "icon-208.png", optimize=True)
    hero.save(ASSETS / "icon-208.webp", quality=90, method=6)

    for name, widths in SHOTS.items():
        shot = Image.open(ASSETS / f"{name}.png").convert("RGB")
        for w in widths:
            resized(shot, w).save(ASSETS / f"{name}-{w}.webp", quality=82, method=6)

    # Link preview: the results screenshot centred on the night background.
    og = Image.new("RGB", (1200, 630), BG)
    shot = Image.open(ASSETS / "results.png").convert("RGB")
    shot = resized(shot, round(shot.width * 550 / shot.height))
    og.paste(shot, ((1200 - shot.width) // 2, (630 - shot.height) // 2))
    og.save(ASSETS / "og.png", optimize=True)

    for f in sorted({DOCS / "favicon.ico", DOCS / "apple-touch-icon.png", *ASSETS.glob("icon-*"), *ASSETS.glob("*-*.webp"), ASSETS / "og.png"}):
        print(f"{f.relative_to(DOCS)}  {f.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
