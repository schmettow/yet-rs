# yet-rs

A Rust port of the Python `yet_zero` eye-tracking experiment runner.

> **Status:** Sprints 0–2 (foundations, risk spikes, experiment model). The
> workspace contains the UI-free engine, presentation protocols, experiment
> loading and a plan command. There is no runnable experiment session yet.

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
- `yet-core` — experiment/config loading from `config.toml`, `Experiment.csv`
  and `Stimuli.csv`, typed validation, resolved media paths and deterministic
  per-block trial planning.
- `yet-app` — `yet-app plan <experiment-dir>` prints the resolved session plan.
- CI for Linux, macOS and Windows, plus a Rust 1.88 MSRV check.
- Sprint 1 risk reports in [`docs/spikes/`](docs/spikes/) covering eframe
  presentation, the overlay matrix, camera capture/parity and media/audio.

## Build

Requires Rust 1.88 or newer.

```sh
cargo test --workspace
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps
cargo run -p yet-app -- plan examples/demo
```

## Experiment format

An experiment directory contains three files:

- `config.toml` — machine settings (monitors, camera, presenter, overlay, audio).
- `Experiment.csv` — optional key/value metadata: `id`, `experimenter`,
  `instructions`, `shuffle`, `seed`. If `shuffle` is true, `seed` is required.
- `Stimuli.csv` — rows with `id`, `kind` (`image` or `sequence`), `path`,
  `fps`, `duration_ms`, `loop`, `audio`, and an optional `block`.

Paths are resolved relative to the experiment directory. Images must exist and
must not set `fps`, `loop=true`, or `audio`; sequences require `fps`. Rows without a
`block` form a `default` block. Trials are shuffled within each block; block `i`
uses `seed + i`, so the plan is reproducible.

See [`examples/demo`](examples/demo) and run:

```sh
cargo run -p yet-app -- plan examples/demo
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
