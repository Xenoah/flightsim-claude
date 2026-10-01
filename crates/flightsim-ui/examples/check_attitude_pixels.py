#!/usr/bin/env python3
"""Check the 18-case offscreen attitude matrix, including outside-dial leakage.

Usage: python3 check_attitude_pixels.py screenshot.png [--shape square] [--expect-leaks]
Requires Pillow. The shader/legacy matrices deliberately use the same colors,
positions, camera, and output size. A blank/failed render is always an error.
"""

import argparse
import json

from PIL import Image


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("image")
    parser.add_argument("--expect-leaks", action="store_true")
    parser.add_argument("--shape", choices=("circle", "square"), default="circle")
    args = parser.parse_args()
    image = Image.open(args.image).convert("RGB")
    if image.size != (1024, 560):
        raise SystemExit(f"Unexpected size: {image.size}, expected (1024, 560)")
    windows = [(48 + 160 * col, 80 + 160 * row) for row in range(3) for col in range(6)]
    colors = ((51, 107, 178), (89, 66, 41))
    inside = [0] * len(windows)
    outside = 0
    for y in range(image.height):
        for x in range(image.width):
            pixel = image.getpixel((x, y))
            if not any(max(abs(a - b) for a, b in zip(pixel, color)) <= 4 for color in colors):
                continue
            matched = False
            for index, (left, top) in enumerate(windows):
                # One-pixel fringe allows normal rasterization antialiasing.
                within = (left - 1 <= x < left + 65 and top - 1 <= y < top + 65)
                if args.shape == "circle":
                    within = (x + 0.5 - left - 32) ** 2 + (y + 0.5 - top - 32) ** 2 <= 33 ** 2
                if within:
                    inside[index] += 1
                    matched = True
                    break
            outside += not matched
    print(json.dumps({"image": args.image, "shape": args.shape, "outside_dial_pixels": outside, "inside_dial_pixels": inside}, indent=2))
    if sum(inside) < 1024:
        raise SystemExit("No reliable sky/ground render detected; a blank image is not a pass")
    if args.expect_leaks:
        if outside == 0:
            raise SystemExit("The original clipping failure was not reproduced")
    elif outside:
        raise SystemExit("Attitude sky/ground leaked beyond the instrument bounds")
    elif any(count < (2900 if args.shape == "circle" else 3500) for count in inside):
        raise SystemExit("One or more fixed instruments are not fully covered")


if __name__ == "__main__":
    main()
