# yet-egui

Egui presenter, experimenter console and gaze overlay for the
[yet-rs](https://github.com/schmettow/yet-rs) eye-tracking toolkit.

## Status

yet-rs is still early-stage. Its long-term goal is a complete research toolkit
and applications for eye tracking. The first external milestone — feature
parity and a drop-in replacement for the Python `yet_zero` prototype — is
achieved.

The main addition over the Python prototype is the experimenter console, which
can run on a multi-monitor desk: the console stays on the experimenter's screen
while the stimulus is a borderless fullscreen window on the participant's
monitor. Windows and macOS runtime validation of the presenter and overlay is
still pending.

## What it does

- `EguiPresenter` — a deferred, monitor-targeted stimulus viewport with images,
  pre-decoded PNG sequences, text, shapes, cached blur, texture reuse and
  frame/audio statistics.
- `console` — the experimenter console: controls, progress, presenter monitor
  selection, calibration panel, camera preview, gaze-prediction viewer and a
  bounded log.
- `EguiOverlay` — a transparent, click-through gaze overlay. It is native on
  Windows/macOS and best-effort on X11; the Wayland layer-shell overlay lives in
  `yet-stim`.
- Markdown rendering for the optional `Intro.md` and `Debrief.md` pages.
- `show_viewport` and `viewport_id` helpers for hosting the presenter inside
  your own eframe application.

All experiment logic stays in `yet-stim`; all tracker primitives stay in
`yet-core`. This is the only crate in the workspace that depends on
egui/eframe/winit.

## Install

```toml
[dependencies]
yet-egui = "0.1"
```

Expect a sizable build because eframe and wgpu are part of the dependency graph.

## Who should use it

Use `yet-egui` if you are building an egui/eframe front end for `yet-stim` or
need the console and presenter as components. If you want the ready-made
application, use [`yet-zero`](https://crates.io/crates/yet-zero).

## Multi-monitor behavior

- `display.presenter_monitor` and `display.console_monitor` are zero-based
  platform monitor indices; the console labels them from 1.
- The presenter uses `ViewportBuilder::with_monitor`, the reliable
  monitor-targeting mechanism on Wayland that also works on X11.
- If the console and presenter target the same monitor, the application warns
  and may move the presenter to another available monitor.
- The presenter is a deferred viewport, so console repaints cannot cause
  stimulus timing jitter.

## Relationship to the other crates

```text
yet-zero -> yet-egui -> yet-stim -> yet-core
```

- [`yet-core`](https://crates.io/crates/yet-core) — portable `no_std` gaze primitives.
- [`yet-stim`](https://crates.io/crates/yet-stim) — engine, media and protocols.
- [`yet-zero`](https://crates.io/crates/yet-zero) — the ready-to-run application.

## License

GPL-3.0-only. Portions of the logic are derived from the Apache-2.0 Python
`yet_zero` prototype. See `LICENSE`, `NOTICE` and `THIRD_PARTY_LICENSES.md`.
