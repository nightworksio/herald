#!/usr/bin/env python3
import re
import sys

from PIL import Image, ImageDraw

RECT = re.compile(r'<rect x="(\d+)" y="(\d+)" width="(\d+)" height="(\d+)" fill="(#[0-9a-fA-F]{6})"/>')
VIEWBOX = re.compile(r'viewBox="0 0 (\d+) (\d+)"')


def render(svg_path, png_path, width=None):
    text = open(svg_path).read()
    vw, vh = map(int, VIEWBOX.search(text).groups())
    img = Image.new("RGB", (vw, vh), "#000000")
    draw = ImageDraw.Draw(img)
    for x, y, w, h, fill in RECT.findall(text):
        x, y, w, h = int(x), int(y), int(w), int(h)
        draw.rectangle([x, y, x + w - 1, y + h - 1], fill=fill)
    if width and width != vw:
        height = round(vh * width / vw)
        resample = Image.NEAREST if width > vw else Image.BOX
        img = img.resize((width, height), resample)
    img.save(png_path)


if __name__ == "__main__":
    render(sys.argv[1], sys.argv[2], int(sys.argv[3]) if len(sys.argv) > 3 else None)
