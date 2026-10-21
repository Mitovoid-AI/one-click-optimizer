"""Draws the OneClick Optimizer icon: a yellow lightning bolt inside a white
speed-gauge arc on a teal-to-blue tile. Writes multi-size .ico files into assets/.

    python tools/make_icon.py

Every size is drawn at 8x and downsampled, and the smallest sizes use a
simplified drawing (a bigger bolt, no gauge) so they stay readable at 16 px.
"""
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"

TEAL = (20, 184, 166)
BLUE = (37, 99, 235)
BOLT = (250, 204, 21)
STATUS = {"normal": (34, 197, 94), "high": (245, 158, 11), "critical": (239, 68, 68)}


def gradient_tile(n: int) -> Image.Image:
    """Diagonal teal->blue gradient clipped to a rounded square."""
    grad = Image.new("RGB", (n, n))
    px = grad.load()
    for y in range(n):
        for x in range(n):
            t = (x + y) / (2 * (n - 1))
            px[x, y] = tuple(round(a + (b - a) * t) for a, b in zip(TEAL, BLUE))
    mask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, n - 1, n - 1), radius=round(n * 0.22), fill=255)
    tile = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    tile.paste(grad, (0, 0), mask)
    return tile


def draw_icon(size: int, status: str | None = None) -> Image.Image:
    ss = 8
    n = size * ss
    s = n / 1024  # design grid is 1024 x 1024
    simple = size <= 24
    img = gradient_tile(n)
    d = ImageDraw.Draw(img)

    # Speed-gauge arc (open at the bottom), skipped at tiny sizes.
    if not simple:
        cx, cy, r = 512, 560, 330
        w = round(64 * s)
        d.arc(((cx - r) * s, (cy - r) * s, (cx + r) * s, (cy + r) * s), start=140, end=40, fill="white", width=w)
        # Round caps on the arc ends.
        import math
        for ang in (140, 40):
            ex = cx + (r - 32) * math.cos(math.radians(ang))
            ey = cy + (r - 32) * math.sin(math.radians(ang))
            d.ellipse(((ex - 32) * s, (ey - 32) * s, (ex + 32) * s, (ey + 32) * s), fill="white")

    # Lightning bolt: the "one click" boost.
    bolt = [(585, 210), (330, 590), (488, 590), (430, 850), (700, 455), (540, 455), (628, 210)]
    if simple:  # bigger bolt, centred, when there is no gauge
        bolt = [(600, 120), (270, 600), (480, 600), (410, 930), (760, 440), (550, 440), (660, 120)]
    d.polygon([(x * s, y * s) for x, y in bolt], fill=BOLT)

    # Optional tray status dot (bottom-right, with a white ring).
    if status:
        c, rad = 800 * s, 210 * s
        d.ellipse((c - rad - 40 * s, c - rad - 40 * s, c + rad + 40 * s, c + rad + 40 * s), fill="white")
        d.ellipse((c - rad, c - rad, c + rad, c + rad), fill=STATUS[status])

    return img.resize((size, size), Image.LANCZOS)


def save_ico(path: Path, sizes, status=None):
    imgs = [draw_icon(sz, status) for sz in sizes]
    imgs[-1].save(path, format="ICO", sizes=[(sz, sz) for sz in sizes], append_images=imgs[:-1])


def main():
    ASSETS.mkdir(exist_ok=True)
    save_ico(ASSETS / "app.ico", [16, 20, 24, 32, 40, 48, 64, 128, 256])
    for name in STATUS:
        save_ico(ASSETS / f"tray-{name}.ico", [16, 20, 24, 32, 40, 48], status=name)
    draw_icon(512).save(ASSETS / "icon-512.png")
    # Preview sheet: big icon + small sizes + tray variants on light and dark.
    sheet = Image.new("RGBA", (900, 330), (245, 247, 250, 255))
    sheet.alpha_composite(draw_icon(256), (20, 37))
    x = 300
    for sz in (64, 48, 32, 24, 16):
        sheet.alpha_composite(draw_icon(sz), (x, 60 + (64 - sz) // 2))
        x += sz + 24
    dark = Image.new("RGBA", (560, 120), (32, 32, 32, 255))
    for i, name in enumerate(STATUS):
        for j, sz in enumerate((32, 24, 16)):
            dark.alpha_composite(draw_icon(sz, name), (20 + i * 180 + j * 50, 40 + (32 - sz) // 2))
    sheet.alpha_composite(dark, (300, 180))
    sheet.save(ASSETS / "preview.png")
    print("wrote", ", ".join(p.name for p in sorted(ASSETS.iterdir())))


if __name__ == "__main__":
    main()
