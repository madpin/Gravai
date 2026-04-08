#!/usr/bin/env python3
"""
Generate a 1024x1024 Gravai app icon — waveform bars on violet background.
Mirrors the system-tray waveform design from make_tray_icon() in lib.rs.
"""
import struct
import zlib
import sys
import os

SIZE = 1024

# Violet background (#6366f1)
BG = (99, 102, 241, 255)
# White bars
BAR = (255, 255, 255, 255)

# Padding from edges (as fraction of SIZE)
PAD = 0.18
CORNER_R = 0.22  # rounded corner radius fraction (for reference — macOS clips automatically)

# Tray proportions (from 22px grid):
#   bars at x: 1,5,9,13,17  width=3  heights=6,12,18,12,6  in 22px
# Scale to icon canvas with padding
canvas = SIZE
usable = int(canvas * (1 - 2 * PAD))
offset = int(canvas * PAD)

num_bars = 5
bar_heights_rel = [6 / 22, 12 / 22, 18 / 22, 12 / 22, 6 / 22]  # relative to 22px

# Fit 5 bars + 4 gaps in usable width
# bar_w : gap = 3 : 2 (from original: bars 3px, gaps 1px at 22px scale → adjust to look nicer)
bar_w = usable // 7   # ~14% each, gaps fill the rest
gap = (usable - 5 * bar_w) // 4

bar_x = [offset + i * (bar_w + gap) for i in range(5)]
bar_h = [int(h * usable) for h in bar_heights_rel]

# Build pixel buffer (RGBA)
pixels = list(BG * SIZE * SIZE)  # flat RGBA list

def set_pixel(x, y, color):
    idx = (y * SIZE + x) * 4
    pixels[idx:idx + 4] = list(color)

# Draw rounded background (macOS clips app icons automatically, skip manual rounding)
# Draw bars
for i, (bx, bh) in enumerate(zip(bar_x, bar_h)):
    top = offset + (usable - bh) // 2
    for y in range(top, top + bh):
        for x in range(bx, bx + bar_w):
            if 0 <= x < SIZE and 0 <= y < SIZE:
                set_pixel(x, y, BAR)

# Add subtle rounded ends to each bar (pill shape)
import math
def draw_circle(cx, cy, r, color):
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            if dx * dx + dy * dy <= r * r:
                x, y = cx + dx, cy + dy
                if 0 <= x < SIZE and 0 <= y < SIZE:
                    set_pixel(x, y, color)

half_w = bar_w // 2
for i, (bx, bh) in enumerate(zip(bar_x, bar_h)):
    cx = bx + half_w
    top = offset + (usable - bh) // 2
    draw_circle(cx, top, half_w, BAR)
    draw_circle(cx, top + bh, half_w, BAR)

# Write PNG
def write_png(filename, pixels, width, height):
    def make_chunk(chunk_type, data):
        c = chunk_type + data
        return struct.pack('>I', len(data)) + c + struct.pack('>I', zlib.crc32(c) & 0xffffffff)

    raw_rows = b''
    for y in range(height):
        row = bytes([0])  # filter type None
        for x in range(width):
            idx = (y * width + x) * 4
            row += bytes(pixels[idx:idx + 4])
        raw_rows += row

    ihdr_data = struct.pack('>IIBBBBB', width, height, 8, 2 | 4, 0, 0, 0)  # bit depth=8, color type=6 (RGBA)
    # Actually color type 6 = RGBA
    ihdr_data = struct.pack('>II', width, height) + bytes([8, 6, 0, 0, 0])
    idat_data = zlib.compress(raw_rows, 9)

    with open(filename, 'wb') as f:
        f.write(b'\x89PNG\r\n\x1a\n')
        f.write(make_chunk(b'IHDR', ihdr_data))
        f.write(make_chunk(b'IDAT', idat_data))
        f.write(make_chunk(b'IEND', b''))

out = sys.argv[1] if len(sys.argv) > 1 else '/tmp/gravai-icon.png'
write_png(out, pixels, SIZE, SIZE)
print(f"Written {out}")
