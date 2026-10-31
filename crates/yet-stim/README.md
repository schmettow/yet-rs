# yet-stim

UI-free experiment engine, media and presentation protocols for the
[yet-rs](https://github.com/schmettow/yet-rs) eye-tracking toolkit.

## Status

yet-rs is still early-stage. Its long-term goal is a complete research toolkit
and applications for eye tracking. The first external milestone — feature
parity and a drop-in replacement for the Python `yet_zero` prototype — is
achieved. `yet-stim` owns the UI-free half of that application; the experimenter
console and the stimulus presenter live in `yet-egui` and `yet-zero`.

## What it does

- Loads both native yet-rs workspaces (`config.toml`, `Experiment.csv`,
  `Stimuli.csv`) and Python `yet_zero` workspaces (`Config.csv`,
  `Stimuli/Stimuli.csv`) without conversion.
- Builds a deterministic, seeded session plan and runs the pure
  `step(state, event, now) -> Vec<Effect>` engine.
- Implements the calibration flow: settle, collect, report, quick
  recalibration and cancel.
- Writes a yet_zero-compatible CSV:
  `Exp,Part,Stim,time,x,y,x_pro,y_pro`.
- Plays pre-decoded PNG sequences and WAV audio; the audio clock is master.
- Defines `Scene`/`DrawCommand` and the `Presenter`, `FrameSource`,
  `AudioPlayer` and `OverlayBackend` seams.
- Provides `RecordingPresenter` for headless runs and replay.
- Provides a Wayland layer-shell overlay and software rasterizer on Linux.

It deliberately does not depend on egui, eframe, winit or a camera driver. A
presenter implements the traits; the application supplies camera frames.

## Install

```toml
[dependencies]
yet-stim = "0.1"
```

## Example

```rust
use yet_stim::load_experiment;

let loaded = load_experiment("path/to/experiment").expect("load experiment");
println!(
    "{} workspace: {} trials in {} blocks",
    loaded.kind,
    loaded.plan.trial_count(),
    loaded.plan.blocks.len()
);
```

`WorkspaceKind` reports whether the directory uses the native layout or the
Python `yet_zero` layout. Both produce the same plan and CSV schema.

## Who should use it

Use `yet-stim` if you are embedding the engine, writing a different presenter,
building a replay or analysis tool, or testing experiment logic without a
window. If you want the ready-made application with the console, presenter and
camera integration, use [`yet-zero`](https://crates.io/crates/yet-zero).

## Media workflow

Video is never decoded at runtime. `tools/prepare_media.sh` in the repository
converts clips to PNG sequences and WAV with FFmpeg once; the application then
plays pre-decoded frames. WAV is used deliberately so the workspace does not
pull an MPL-2.0 decoder.

## Relationship to the other crates

```text
yet-zero -> yet-egui -> yet-stim -> yet-core
```

- [`yet-core`](https://crates.io/crates/yet-core) — portable `no_std` gaze primitives.
- [`yet-egui`](https://crates.io/crates/yet-egui) — presenter, console and overlay.
- [`yet-zero`](https://crates.io/crates/yet-zero) — the ready-to-run application.

## License

GPL-3.0-only. Portions of the logic are derived from the Apache-2.0 Python
`yet_zero` prototype. See `LICENSE`, `NOTICE` and `THIRD_PARTY_LICENSES.md`.
