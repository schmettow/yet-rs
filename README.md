# yet-rs

A Rust port of the Python `yet_zero` eye-tracking experiment runner.

> **Status:** Sprint 0 (foundations). The workspace contains the UI-free engine
> and the presentation protocols. There is no runnable experiment binary yet.

## What it will do

- Present static images and pre-decoded video clips on a chosen monitor.
- Calibrate a webcam-based tracker from four quadrant-brightness features.
- Drive experiments from a separate experimenter console.
- Draw a click-through gaze overlay over the desktop.
- Write trial and frame data to CSV for the existing R pipeline.
- Run on Windows, macOS and Linux without OpenCV or a C/C++ toolchain.

## What exists now

- `yet-present` — `SessionTime`, physical-pixel geometry, `Scene`/`DrawCommand`,
  presenter/overlay/media/audio traits, and a `RecordingPresenter`.
- `yet-core` — a pure `step` engine with start/pause/resume/abort/tick,
  a `TestClock`, and the calibration data types.
- CI for Linux, macOS and Windows, plus a Rust 1.88 MSRV check.

## Build

Requires Rust 1.88 or newer.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
```

## Design

- The engine is independent of any UI. `step(state, event, now)` returns
  effects; the application executes them.
- Protocol coordinates are monitor-local physical pixels.
- Protocol messages carry media identifiers and owned text, never image buffers.
- `yet-core` and `yet-present` must not depend on a windowing toolkit.

The reasoning behind these choices is in
[`DEVELOPMENT_PLAN.md`](DEVELOPMENT_PLAN.md),
[`UI_analysis.md`](UI_analysis.md) and [`CV_analysis.md`](CV_analysis.md).

## License

GPL-3.0-only. Logic ported from the Apache-2.0 `yet_zero` prototype retains its
notice.
