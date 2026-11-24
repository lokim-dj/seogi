"""Generates the 서기 (Seogi) app icon source, tray glyphs and the in-app mark.

Run:  python3 src-tauri/icon-src/make_icons.py
Then: bun tauri icon src-tauri/icon-src/app-icon.png   (regenerates src-tauri/icons/*)

Needs Pillow and a Pretendard ExtraBold font (falls back to Apple SD Gothic Neo).
"""
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

HERE = Path(__file__).resolve().parent
RES = HERE.parent / "resources"

FONT_CANDIDATES = [
    Path.home() / "Library/Fonts/Pretendard-ExtraBold.otf",
    Path("/Library/Fonts/Pretendard-ExtraBold.otf"),
    Path("C:/Windows/Fonts/malgunbd.ttf"),
    Path("/System/Library/Fonts/AppleSDGothicNeo.ttc"),
]


def font(size: int) -> ImageFont.FreeTypeFont:
    for path in FONT_CANDIDATES:
        if path.exists():
            if path.suffix == ".ttc":
                return ImageFont.truetype(str(path), size, index=6)  # Bold face
            return ImageFont.truetype(str(path), size)
    raise SystemExit("No Korean bold font found")


INK = (24, 32, 38, 255)        # near-black blue-green
PAPER = (247, 243, 234, 255)   # warm paper
ACCENT = (214, 84, 52, 255)    # brick red, the record dot


def centered_text(draw: ImageDraw.ImageDraw, box, text: str, fnt, fill):
    left, top, right, bottom = draw.textbbox((0, 0), text, font=fnt)
    w, h = right - left, bottom - top
    x = box[0] + (box[2] - box[0] - w) / 2 - left
    y = box[1] + (box[3] - box[1] - h) / 2 - top
    draw.text((x, y), text, font=fnt, fill=fill)


def app_icon(size: int = 1024) -> Image.Image:
    scale = 4
    s = size * scale
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    pad = int(s * 0.098)            # macOS icon grid: content inset
    radius = int(s * 0.185)
    d.rounded_rectangle((pad, pad, s - pad, s - pad), radius=radius, fill=INK)
    # The syllable 서 (from 서기, the one who takes the minutes), set large;
    # the record dot sits where a period would.
    centered_text(d, (pad, pad - int(s * 0.03), s - pad, s - pad - int(s * 0.03)), "서", font(int(s * 0.50)), PAPER)
    dot_r = int(s * 0.043)
    cx, cy = s - pad - int(s * 0.135), s - pad - int(s * 0.135)
    d.ellipse((cx - dot_r, cy - dot_r, cx + dot_r, cy + dot_r), fill=ACCENT)
    return img.resize((size, size), Image.LANCZOS)


def tray_glyph(color, state: str, size: int = 64) -> Image.Image:
    """Menu-bar glyph: 서 in a rounded frame. Recording fills a dot, transcribing draws lines."""
    scale = 8
    s = size * scale
    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    m = int(s * 0.10)
    stroke = int(s * 0.075)
    d.rounded_rectangle((m, m, s - m, s - m), radius=int(s * 0.22), outline=color, width=stroke)
    if state == "idle":
        centered_text(d, (m, m - int(s * 0.02), s - m, s - m - int(s * 0.02)), "서", font(int(s * 0.50)), color)
    elif state == "recording":
        r = int(s * 0.19)
        d.ellipse((s // 2 - r, s // 2 - r, s // 2 + r, s // 2 + r), fill=color)
    elif state == "transcribing":
        x0, x1 = int(s * 0.30), int(s * 0.70)
        for i, frac in enumerate((0.36, 0.50, 0.64)):
            y = int(s * frac)
            end = x1 if i < 2 else int(s * 0.56)
            d.rounded_rectangle((x0, y - stroke // 2, end, y + stroke // 2), radius=stroke // 2, fill=color)
    return img.resize((size, size), Image.LANCZOS)


def with_warning_badge(base: Image.Image) -> Image.Image:
    img = base.copy()
    d = ImageDraw.Draw(img)
    r = 11
    cx, cy = img.width - r - 1, r + 1
    d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=(226, 160, 40, 255))
    return img


def main():
    icon = app_icon()
    icon.save(HERE / "app-icon.png")
    # The mark shown beside the wordmark inside the app.
    icon.resize((160, 160), Image.LANCZOS).save(HERE.parent.parent / "src" / "assets" / "seogi-icon.png")

    white = (255, 255, 255, 255)
    black = (0, 0, 0, 255)
    # Upstream naming: tray_*.png is drawn light (for a dark menu bar), *_dark.png is drawn dark.
    for state in ("idle", "recording", "transcribing"):
        tray_glyph(white, state).save(RES / f"tray_{state}.png")
        tray_glyph(black, state).save(RES / f"tray_{state}_dark.png")
    with_warning_badge(tray_glyph(white, "idle")).save(RES / "tray_idle_warning.png")
    with_warning_badge(tray_glyph(black, "idle")).save(RES / "tray_idle_warning_dark.png")

    # Colored variants (Linux "colored" tray theme): the small app icon itself.
    small = app_icon(256).resize((64, 64), Image.LANCZOS)
    small.save(RES / "handy.png")
    with_warning_badge(small).save(RES / "handy_warning.png")
    rec = tray_glyph(ACCENT, "recording")
    rec.save(RES / "recording.png")
    tray_glyph(INK, "transcribing").save(RES / "transcribing.png")
    print("wrote", HERE / "app-icon.png", "and tray glyphs in", RES)


if __name__ == "__main__":
    main()
