# yet-rs

A Rust port of the Python `yet_zero` eye-tracking experiment runner.

> **Status:** Sprints 0–3 (foundations, risk spikes, experiment model,
> scheduler and headless slice). The workspace runs a two-block experiment
> without a window and writes a yet_zero-compatible result CSV. There is no
> camera, no console and no presenter window yet.

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
  absolute trial deadlines, late-tick accounting, a `TestClock`, and the
  calibration data types.
- `yet-core` — experiment/config loading from `config.toml`, `Experiment.csv`
  and `Stimuli.csv`, typed validation, resolved media paths and deterministic
  per-block trial planning.
- `yet-core` — a calibration grid (3×3 at 0.125/0.5/0.875), stimulus placement
  and golden scenes, plus a yet_zero-compatible CSV data sink with flush per
  trial.
- `yet-app` — `yet-app plan <experiment-dir>` prints the resolved session plan.
- `yet-app` — `yet-app run --headless <experiment-dir>` drives a full session
  with a synthetic monotonic clock and writes the result CSV. Without a camera
  there are no gaze samples yet, so the file contains the header only.
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
cargo run -p yet-app -- run --headless examples/demo --participant demo01
```

## Headless runs

`yet-app run --headless` needs no window and no display. It walks the plan
with a 10 ms synthetic clock: each tick jumps to the next frame step or trial
deadline, so a five-trial demo finishes in milliseconds. Output:

```text
Result: examples/demo/yet1_demoMartin_demo01.csv
Trials: 5 of 5 completed
Samples: 0
```

Use `--output <csv>` to choose the result path. `--participant` defaults to
the Unix time at session start, as in the Python prototype. Until Sprint 5
adds the camera there is no gaze data, so the result file contains only the
schema header; the trial scheduler and protocol wiring are still exercised
end to end.

### Result CSV

Columns are exactly `Exp,Part,Stim,time,x,y,x_pro,y_pro`, matching yet_zero.
`Exp` concatenates the experiment id and the experimenter, `Part` is the
participant id, `x`/`y` are gaze coordinates in stimulus pixels, and
`x_pro`/`y_pro` are normalized to the stimulus size. Times are epoch seconds
with microsecond precision. Every trial boundary flushes the file, so an
aborted session keeps the completed trials.

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
