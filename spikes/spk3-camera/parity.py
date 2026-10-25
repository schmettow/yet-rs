#!/usr/bin/env python3
"""SPK-3 Python reference for quadrant means.

Creates a small odd-sized fixture with four distinct quadrants (if missing)
and prints the same JSON shape as the Rust spike. Run from this directory:

    python3 parity.py
    ../target/release/spk3-camera --fixture fixtures/quadrants.png
"""

from __future__ import annotations

import json
from pathlib import Path

from PIL import Image


FIXTURE = Path("fixtures/quadrants.png")


def make_fixture() -> None:
    FIXTURE.parent.mkdir(parents=True, exist_ok=True)
    width, height = 7, 5
    image = Image.new("RGB", (width, height))
    mid_x, mid_y = width // 2, height // 2
    colors = {
        "nw": (255, 0, 0),
        "ne": (0, 255, 0),
        "sw": (0, 0, 255),
        "se": (128, 128, 128),
    }
    for y in range(height):
        for x in range(width):
            if x < mid_x and y < mid_y:
                color = colors["nw"]
            elif x >= mid_x and y < mid_y:
                color = colors["ne"]
            elif x < mid_x and y >= mid_y:
                color = colors["sw"]
            else:
                color = colors["se"]
            image.putpixel((x, y), color)
    image.save(FIXTURE)


def quadrant_means(path: Path) -> dict[str, float]:
    image = Image.open(path).convert("L")
    width, height = image.size
    mid_x, mid_y = width // 2, height // 2
    regions = {
        "nw": (0, 0, mid_x, mid_y),
        "ne": (mid_x, 0, width, mid_y),
        "sw": (0, mid_y, mid_x, height),
        "se": (mid_x, mid_y, width, height),
    }
    pixels = image.load()
    result: dict[str, float] = {}
    for name, (x0, y0, x1, y1) in regions.items():
        values = [
            pixels[x, y]
            for y in range(y0, y1)
            for x in range(x0, x1)
        ]
        result[name] = sum(values) / len(values)
    return result


def main() -> None:
    if not FIXTURE.exists():
        make_fixture()
    print(json.dumps(quadrant_means(FIXTURE), sort_keys=True))


if __name__ == "__main__":
    main()
