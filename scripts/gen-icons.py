#!/usr/bin/env python3
"""从 assets/icon/typex.svg 的几何规格导出全平台图标（04 §2.2）。

不依赖 SVG 光栅化库——图形只是圆角矩形组合，直接用 PIL 按同一几何绘制。
产物：src-tauri/icons/{32x32,128x128,128x128@2x,icon}.png、icon.icns、icon.ico、tray.png。
"""
from pathlib import Path
from PIL import Image, ImageDraw
import subprocess
import sys

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "src-tauri" / "icons"
OUT.mkdir(parents=True, exist_ok=True)

# 1024 网格几何（typex.svg 同源；源自应用内 mini glyph）
BARS = [(196.5, 266.5, 158), (334.5, 207.5, 276), (472.5, 148.5, 394), (610.5, 207.5, 276), (748.5, 266.5, 158)]
BAR_W = 79
STEM = (472.5, 600.5, 79, 276)
APP_TILE_INSET = 72
APP_TILE_SCALE = (1024 - 2 * APP_TILE_INSET) / 1024


def fit_rect(x, y, w, h, scale=1.0):
    if scale == 1.0:
        return x, y, w, h
    return (
        512 + (x - 512) * scale,
        512 + (y - 512) * scale,
        w * scale,
        h * scale,
    )


def rounded(draw: ImageDraw.ImageDraw, x, y, w, h, s, fill):
    r = min(w, h) / 2
    draw.rounded_rectangle([x * s, y * s, (x + w) * s, (y + h) * s], radius=r * s, fill=fill)


def render(size: int, bg, fg, tile_scale=1.0) -> Image.Image:
    """按 04 §2.2 绘制：五柱 + 短竖笔，无底衬。"""
    scale = 8  # 超采样抗锯齿
    canvas = size * scale
    s = canvas / 1024
    img = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if bg:
        inset = (1 - tile_scale) * canvas / 2
        d.rounded_rectangle(
            [inset, inset, canvas - inset, canvas - inset],
            radius=(canvas - 2 * inset) * 0.27,
            fill=bg,
        )
    for (x, y, h) in BARS:
        x, y, w, h = fit_rect(x, y, BAR_W, h, tile_scale)
        rounded(d, x, y, w, h, s, fg)
    rounded(d, *fit_rect(*STEM, tile_scale), s, fg)
    return img.resize((size, size), Image.LANCZOS)


BLACK = (0, 0, 0, 255)
WHITE = (255, 255, 255, 255)

# --- 应用图标（纸版：白底黑 glyph，Dock 默认）---
for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (512, "icon.png")]:
    render(size, WHITE, BLACK, tile_scale=APP_TILE_SCALE).save(OUT / name)

# --- .icns（macOS）---
iconset = OUT / "typex.iconset"
iconset.mkdir(exist_ok=True)
for pt in (16, 32, 128, 256, 512):
    render(pt, WHITE, BLACK, tile_scale=APP_TILE_SCALE).save(iconset / f"icon_{pt}x{pt}.png")
    render(pt * 2, WHITE, BLACK, tile_scale=APP_TILE_SCALE).save(iconset / f"icon_{pt}x{pt}@2x.png")
subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(OUT / "icon.icns")], check=True)

# --- .ico（Windows）---
ico_sizes = [16, 24, 32, 48, 64, 256]
imgs = [render(sz, WHITE, BLACK, tile_scale=APP_TILE_SCALE) for sz in ico_sizes]
imgs[-1].save(OUT / "icon.ico", format="ICO", sizes=[(sz, sz) for sz in ico_sizes], append_images=imgs[:-1])

# --- 托盘图标：去竖笔的五柱 glyph，macOS template image（纯黑 + alpha）---
def render_tray(size: int) -> Image.Image:
    scale = 8
    canvas = size * scale
    img = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # 五柱整体在正方形中垂直居中放大（托盘无竖笔）
    top = min(y for _, y, _ in BARS)
    bottom = max(y + h for _, y, h in BARS)
    glyph_h = bottom - top
    pad = 1024 * 0.18
    s = (canvas - 2 * pad * canvas / 1024) / 1024
    sy = (1024 - glyph_h) / 2 - top  # 垂直居中偏移
    off = pad * canvas / 1024
    for (x, y, h) in BARS:
        r = BAR_W / 2 * s
        d.rounded_rectangle(
            [off + x * s, off + (y + sy) * s, off + (x + BAR_W) * s, off + (y + sy + h) * s],
            radius=r, fill=BLACK)
    return img.resize((size, size), Image.LANCZOS)

render_tray(44).save(OUT / "tray.png")  # 22pt @2x
render_tray(22).save(OUT / "tray-22.png")

import shutil
shutil.rmtree(iconset)
print(f"icons written to {OUT}")
