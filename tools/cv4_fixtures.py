#!/usr/bin/env python3
"""Generate small CV-4 parity fixtures with the Python oracle.

The fixtures are deliberately tiny, deterministic and synthetic: they contain
no personal data. They are consumed by
`crates/yet-core/tests/cv4_parity.rs`.

Requirements: numpy, opencv-python, scikit-learn. The yet_zero virtualenv has
all three:

    /path/to/yet_zero/.venv/bin/python tools/cv4_fixtures.py

Run from any directory; paths are resolved relative to this file.
"""

from __future__ import annotations

import csv
from pathlib import Path

import cv2
import numpy as np
from sklearn.linear_model import LinearRegression

ROOT = Path(__file__).resolve().parent.parent
FIXTURES = ROOT / "crates" / "yet-core" / "tests" / "fixtures" / "cv4"


def pattern(width: int, height: int) -> np.ndarray:
    """A deterministic, non-symmetric grayscale pattern."""
    rows, cols = np.indices((height, width), dtype=np.int64)
    values = (cols * 37 + rows * 53 + (cols * rows * 11) % 97) % 256
    return values.astype(np.uint8)


def quadrant_means(image: np.ndarray) -> tuple[float, float, float, float]:
    """Canonical (nw, ne, sw, se), with the remainder in east/south."""
    height, width = image.shape
    mid_x, mid_y = width // 2, height // 2
    return (
        float(image[0:mid_y, 0:mid_x].mean()),
        float(image[0:mid_y, mid_x:width].mean()),
        float(image[mid_y:height, 0:mid_x].mean()),
        float(image[mid_y:height, mid_x:width].mean()),
    )


def write_features() -> None:
    directory = FIXTURES / "features"
    directory.mkdir(parents=True, exist_ok=True)
    sizes = [(7, 5), (5, 7), (8, 6), (103, 77)]

    with (directory / "features.csv").open("w", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["file", "width", "height", "nw", "ne", "sw", "se"])
        for width, height in sizes:
            image = pattern(width, height)
            name = f"pattern_{width}x{height}.gray"
            image.tofile(directory / name)
            means = quadrant_means(image)
            writer.writerow(
                [name, width, height, *[f"{value:.17g}" for value in means]]
            )


FEATURES = np.array(
    [
        [200.0, 128.0, 128.0, 128.0],
        [128.0, 200.0, 128.0, 128.0],
        [128.0, 128.0, 200.0, 128.0],
        [128.0, 128.0, 128.0, 200.0],
        [100.0, 100.0, 100.0, 100.0],
        [150.0, 110.0, 190.0, 130.0],
        [110.0, 190.0, 130.0, 150.0],
        [190.0, 130.0, 150.0, 110.0],
        [130.0, 150.0, 110.0, 190.0],
        [170.0, 140.0, 120.0, 160.0],
        [120.0, 160.0, 180.0, 140.0],
        [160.0, 120.0, 160.0, 120.0],
    ],
    dtype=np.float32,
).astype(np.float64)

NOISE = np.array(
    [
        [0.3, -0.2],
        [-0.4, 0.1],
        [0.2, 0.35],
        [-0.1, -0.3],
        [0.25, 0.15],
        [-0.35, 0.25],
        [0.15, -0.15],
        [-0.2, 0.3],
        [0.1, 0.2],
        [0.0, 0.0],
        [0.0, 0.0],
        [0.0, 0.0],
    ],
    dtype=np.float64,
)


def write_regression() -> None:
    directory = FIXTURES / "regression"
    directory.mkdir(parents=True, exist_ok=True)

    slopes = np.array(
        [[3.0, -2.0, 1.5, -0.5], [-1.0, 2.5, 0.75, -1.25]], dtype=np.float64
    )
    intercept = np.array([100.0, 50.0], dtype=np.float64)
    targets = (FEATURES @ slopes.T + intercept + NOISE).astype(np.float32).astype(np.float64)

    training = slice(0, 9)
    model = LinearRegression().fit(FEATURES[training], targets[training])
    predictions = model.predict(FEATURES[training.stop :])

    with (directory / "calibration.csv").open("w", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["nw", "ne", "sw", "se", "target_x", "target_y"])
        for features, target in zip(FEATURES, targets):
            writer.writerow(
                [
                    *[f"{value:.17g}" for value in features],
                    *[f"{value:.17g}" for value in target],
                ]
            )

    with (directory / "predictions.csv").open("w", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["nw", "ne", "sw", "se", "predicted_x", "predicted_y"])
        for features, prediction in zip(FEATURES[training.stop :], predictions):
            writer.writerow(
                [
                    *[f"{value:.17g}" for value in features],
                    *[f"{value:.17g}" for value in prediction],
                ]
            )

    with (directory / "coefficients.csv").open("w", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["output", "intercept", "nw", "ne", "sw", "se"])
        for axis, name in enumerate(["x", "y"]):
            writer.writerow(
                [
                    name,
                    f"{model.intercept_[axis]:.17g}",
                    *[f"{value:.17g}" for value in model.coef_[axis]],
                ]
            )


def write_luma() -> None:
    directory = FIXTURES / "luma"
    directory.mkdir(parents=True, exist_ok=True)
    width, height = 7, 5

    rows, cols = np.indices((height, width), dtype=np.int64)
    red = (cols * 41 + rows * 7) % 256
    green = (cols * 13 + rows * 29) % 256
    blue = (cols * 97 + rows * 11) % 256
    rgb = np.stack([red, green, blue], axis=-1).astype(np.uint8)
    gray = cv2.cvtColor(rgb, cv2.COLOR_RGB2GRAY)

    rgb.tofile(directory / f"colors_{width}x{height}.rgb")
    gray.tofile(directory / f"colors_{width}x{height}.gray")
    with (directory / "meta.csv").open("w", newline="") as handle:
        writer = csv.writer(handle)
        writer.writerow(["rgb", "gray", "width", "height"])
        writer.writerow(
            [
                f"colors_{width}x{height}.rgb",
                f"colors_{width}x{height}.gray",
                width,
                height,
            ]
        )


def main() -> None:
    write_features()
    write_regression()
    write_luma()
    print(f"wrote CV-4 fixtures to {FIXTURES}")


if __name__ == "__main__":
    main()
