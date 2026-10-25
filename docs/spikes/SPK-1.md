# SPK-1 — eframe presentation timing and HiDPI geometry

**Status:** Go for the eframe/wgpu presenter path on X11. Wayland and Windows/macOS runtime tests remain.

**Question:** Can eframe 0.37 present a fullscreen stimulus on a chosen monitor with stable frame pacing and correct physical-pixel geometry?

**Harness:** `spikes/spk1-eframe` (disposable, excluded from the root workspace).

## Method

- Build: `cargo build --release -p spk1-eframe`.
- Run: `../target/release/spk1-eframe` from `spikes/spk1-eframe`.
- Environment: X11 `DISPLAY=:1`, two monitors, wgpu 30.0.1, eframe 0.37.0.
- Viewport: `with_monitor(1)`, `with_fullscreen(true)`, wgpu renderer.
- Content: a generated 64×64 checker texture, a filled circle, and text, drawn every frame.
- 180 frames; at frame 90 the app calls `ctx.set_zoom_factor(1.5)`.
- Logged per frame: elapsed, `dt`, `stable_dt`, `pixels_per_point`, inner size, monitor size.

## Findings

### eframe 0.37 API changed

`eframe::App::update(&Context, &mut Frame)` no longer exists. The required methods are:

```rust
fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame);
// optional: fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame);
```

`egui::CentralPanel::show` now takes `&mut Ui`, not `&Context`. The spike implements `ui`. The UI analysis' implementation notes need this correction.

### Monitor targeting settles after a few frames

The window was created at 1280×720 on monitor 0 for frames 0–4. From frame 5 it was fullscreen on monitor 1. A production presenter must wait for a stable `inner_rect`/`monitor_size`/`pixels_per_point` triple before starting a trial or logging timing.

### Geometry

| Phase | `pixels_per_point` | Inner size (points) | Monitor size (points) | Physical interpretation |
|---|---:|---:|---:|---|
| Initial window, monitor 0 | 1.083 | 1280.3×720.0 | 2363.1×1476.9 | Windowed 1280×720 |
| Fullscreen, monitor 1 | 1.167 | 1234.3×2194.3 | 1234.3×2194.3 | 1440×2560 physical |
| Zoom 1.5 on monitor 1 | 1.750 | 822.9×1462.9 | 822.9×1462.9 | 1440×2560 physical |

The conversion is consistent: `1234.3 × 1.167 ≈ 1440` and `822.9 × 1.75 ≈ 1440`. No coordinate drift was observed across the zoom change.

### Frame pacing

Measurements after the monitor transition (frame ≥ 10):

| Segment | Frames | Mean `dt` | p50 | p95 | p99 | Max | Frames > 20 ms |
|---|---:|---:|---:|---:|---:|---:|---:|
| All | 171 | 16.66 ms | 16.68 | 17.16 | 17.57 | 17.66 | 0 |
| Zoom 1.0 | 81 | 16.63 ms | 16.67 | 17.13 | 17.21 | 17.66 | 0 |
| Zoom 1.5 | 90 | 16.68 ms | 16.68 | 17.20 | 17.57 | 17.60 | 0 |

The frame interval is a stable ~60 Hz (`16.67 ms`). The zoom change at frame 91 did not produce a late frame. The first five frames are excluded from the timing summary because they were on the wrong monitor.

### Build-dependency note

`wayland-backend` declares `cc` as a build dependency, but its `log` feature is off in this graph, so no C shim is compiled. This was verified by the absence of `log_shim*` artifacts and by `cargo tree -e features`; `bindgen` and libclang are absent. The Linux C/C++ question is therefore not decided by eframe; the audio ALSA prerequisite is the real build constraint (SPK-4).

## Decision

**Go.** eframe/wgpu is adequate for R2 on X11, and monitor targeting plus HiDPI conversion work. The presenter must:

- wait for the first stable fullscreen configure before starting a trial;
- use physical pixels at the protocol boundary and convert at the eframe boundary;
- log `pixels_per_point`, inner size and monitor size at startup;
- treat `App::ui` as the rendering entry point.

## Follow-ups

- Repeat SPK-1 on Wayland (KDE or Sway) and on Windows/macOS hardware; X11 is not the target platform.
- Add a “presenter ready” state to `EguiPresenter` that suppresses early frames.
- Do not change zoom during a session; the spike used zoom only to test geometry.
- Verify `with_monitor` behavior on Windows/macOS, including mixed-DPI setups.
