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

# 1024 网格几何（typex.svg 同源）
BARS = [(220, 148.5, 151), (348, 101.5, 245), (476, 44, 360), (604, 101.5, 245), (732, 148.5, 151)]
BAR_W = 72
STEM = (476, 460, 72, 480)
FOOT = (436, 884, 152, 56)


def rounded(draw: ImageDraw.ImageDraw, x, y, w, h, s, fill):
    r = min(w, h) / 2
    draw.rounded_rectangle([x * s, y * s, (x + w) * s, (y + h) * s], radius=r * s, fill=fill)


def render(size: int, bg, fg, small_threshold=32) -> Image.Image:
    """按 04 §2.2 绘制：≤32px 降为三柱 + 竖笔、去底衬。"""
    scale = 8  # 超采样抗锯齿
    canvas = size * scale
    s = canvas / 1024
    img = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    if bg:
        d.rounded_rectangle([0, 0, canvas, canvas], radius=canvas * 0.22, fill=bg)
    bars = BARS if size > small_threshold else BARS[1:4]
    for (x, y, h) in bars:
        rounded(d, x, y, BAR_W, h, s, fg)
    rounded(d, *STEM, s, fg)
    if size > small_threshold:
        rounded(d, *FOOT, s, fg)
    return img.resize((size, size), Image.LANCZOS)


BLACK = (0, 0, 0, 255)
WHITE = (255, 255, 255, 255)

# --- 应用图标（墨版：黑底白 glyph）---
for size, name in [(32, "32x32.png"), (128, "128x128.png"), (256, "128x128@2x.png"), (512, "icon.png")]:
    render(size, BLACK, WHITE).save(OUT / name)

# --- .icns（macOS）---
iconset = OUT / "typex.iconset"
iconset.mkdir(exist_ok=True)
for pt in (16, 32, 128, 256, 512):
    render(pt, BLACK, WHITE).save(iconset / f"icon_{pt}x{pt}.png")
    render(pt * 2, BLACK, WHITE).save(iconset / f"icon_{pt}x{pt}@2x.png")
subprocess.run(["iconutil", "-c", "icns", str(iconset), "-o", str(OUT / "icon.icns")], check=True)

# --- .ico（Windows）---
ico_sizes = [16, 24, 32, 48, 64, 256]
imgs = [render(sz, BLACK, WHITE) for sz in ico_sizes]
imgs[-1].save(OUT / "icon.ico", format="ICO", sizes=[(sz, sz) for sz in ico_sizes], append_images=imgs[:-1])

# --- 托盘图标：去竖笔的五柱 glyph，macOS template image（纯黑 + alpha）---
def render_tray(size: int) -> Image.Image:
    scale = 8
    canvas = size * scale
    img = Image.new("RGBA", (canvas, canvas), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    # 五柱整体在正方形中垂直居中放大（托盘无竖笔）
    top, bottom = 44, 404
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
