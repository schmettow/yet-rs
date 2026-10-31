# yet-core

Portable, `no_std` gaze-tracking primitives for the
[yet-rs](https://github.com/schmettow/yet-rs) eye-tracking toolkit.

## Status

yet-rs is still early-stage. Its long-term goal is a complete research toolkit
and applications for eye tracking. The first external milestone — feature
parity and a drop-in replacement for the Python `yet_zero` prototype — is
achieved. `yet-core` is the portable foundation of that toolkit.

## What it does

`yet-core` defines the vocabulary shared by a tracker, a stimulus presenter and
a future microcontroller implementation. It is `#![no_std]` (with `alloc`) and
knows nothing about windows, image decoding, files or audio:

- `SessionTime` and the `Clock`/`TestClock` seam for deterministic timing.
- `CameraRole`, `CameraFrame` and the borrowed `FrameSet` for timestamped,
  multi-camera-ready frames.
- `GrayFrame`, `EyeRegion`, `quad_brightness`, Rec. 601 `rec601_luma` and the
  `FeatureExtractor`/`QuadExtractor` seam.
- `GazePredictor`, `LinearPredictor`, `GazeInput`, `GazePoint`,
  `ModelParameters` and the versioned model format.
- `QUAD_LAYOUT` for the four Quadbright features and `OCTA_LAYOUT` for the
  future eight-feature binocular layout.

The non-default `yet-zero` feature adds the host-side reference trainer and
calibration buffer (`LinearGazeModel`, `CalibrationBuffer`, `CalibrationReport`)
using `nalgebra`. A `no_std` consumer that only deploys a pre-trained model does
not need it.

## Install

```toml
[dependencies]
yet-core = "0.1"
```

For host-side training and calibration:

```toml
[dependencies]
yet-core = { version = "0.1", features = ["yet-zero"] }
```

The crate has no default features, so the plain dependency is already the
portable configuration.

## Example

```rust
use yet_core::{
    GazeInput, GazePredictor, LinearPredictor, ModelParameters, MAX_FEATURES,
};

let mut coefficients = [[0.0; MAX_FEATURES]; 2];
coefficients[0][0] = 0.5;
coefficients[1][1] = 0.5;

let parameters = ModelParameters::new(2, coefficients, [0.0, 0.0]).unwrap();
let predictor = LinearPredictor::new(parameters);
let point = predictor.predict(GazeInput::glance(&[10.0, 20.0])).unwrap();

assert_eq!((point.x, point.y), (5.0, 10.0));
```

The prediction path is `f32`, allocation-free and does not require `std`; it is
the shape a future ESP32-C implementation can use unchanged.

## Who should use it

Use `yet-core` if you are building a tracker, a feature extractor, a replay tool
or an embedded deployment and need the shared types without a UI or media stack.
If you want to run an experiment, use the
[`yet-zero`](https://crates.io/crates/yet-zero) application instead.

## Relationship to the other crates

```text
yet-zero -> yet-egui -> yet-stim -> yet-core
```

- [`yet-stim`](https://crates.io/crates/yet-stim) — engine, media and protocols.
- [`yet-egui`](https://crates.io/crates/yet-egui) — presenter, console and overlay.
- [`yet-zero`](https://crates.io/crates/yet-zero) — the ready-to-run application.

## License

GPL-3.0-only. Portions of the logic are derived from the Apache-2.0 Python
`yet_zero` prototype. See `LICENSE`, `NOTICE` and `THIRD_PARTY_LICENSES.md`.
