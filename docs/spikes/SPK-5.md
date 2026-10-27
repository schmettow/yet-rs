# SPK-5 — Camera capture and calibration on the lab machine

**Status:** Linux camera and graphical calibration work on both the Yet camera
and the StreamCam. The Yet camera is the GEMBIRD/AppoTech device on
`/dev/video2`. CV-7 still needs a **human** calibration RMSE; macOS and Windows
runtime remain untested.

**Question:** Does the Sprint 5 camera adapter capture reliably, which camera
and format should the lab configuration use, and does the calibration flow work
with a real target display?

## Hardware and display layout

- OS: Linux 6.18.7-76061807-generic, 2026-10-25.
- **Yet camera:** GEMBIRD `1908:2311` Generic UVC, AppoTech AX2311, on
  `/dev/video2`. `/dev/video3` is its metadata node and exposes no capture
  format. It advertises **YUYV only** at 640×480.
- **Other camera:** Logitech StreamCam on `/dev/video0` (`/dev/video1` is
  metadata); YUYV and MJPG at 640×480.
- Console monitor: DP-1, 2560×1600 at desktop origin `+0+662`.
- Presenter monitor: DP-3, **1440×2560 portrait** at desktop origin `+2560+0`.
- Adapter: `linuxvideo` 0.3.5 through `yet_core::camera`; no bindgen, no
  libclang, no C compiler. The camera is opened, read and dropped on the
  capture thread.
- Release builds are required for MJPEG; debug decoding is CPU-bound.

## Capture measurements

Command:

```sh
cargo run --release -p yet-zero -- camera examples/demo --seconds 3
```

`examples/demo` points at the GEMBIRD. For the StreamCam, pass
`--device 0` and temporarily set `format = "mjpeg"` in the config (or use a
separate experiment directory).

| Camera | Node | Format | Frames / time | fps | corrupt | dropped | errors |
|---|---|---|---:|---:|---:|---:|---:|
| GEMBIRD (Yet) | video2 | YUYV | 77 / 3.00 s | 25.6 | 1 | 0 | 0 |
| StreamCam | video0 | MJPG | 78 / 3.00 s | 26.0 | 6 | 0 | 0 |
| StreamCam | video0 | YUYV | 48 / 3.00 s | 16.0 | 35 | 0 | 0 |
| StreamCam | video0 | MJPG (debug) | 24 / 2.00 s | 12.0 | 5 | 0 | 0 |

`corrupt` counts V4L2 error-flagged frames, truncated buffers, undecodable
MJPEG and sampled all-black frames. They are skipped, not fatal. Several MJPG
corrupt frames occur during warm-up. The rate includes warm-up.

Last-frame quadrant means for the centered ROI `x=160 y=120 320×240`:

| Camera / format | NW | NE | SW | SE |
|---|---:|---:|---:|---:|
| GEMBIRD YUYV | 215.16 | 216.48 | 93.05 | 108.89 |
| StreamCam MJPG | 139.06 | 103.29 | 80.74 | 65.43 |
| StreamCam YUYV | 131.88 | 100.32 | 84.47 | 69.02 |

These are single frames under uncontrolled illumination; they are **not** a
calibration dataset or a controlled format comparison.

## Graphical calibration window

`yet-zero calibrate <dir>` now opens a borderless window on the presenter
monitor, draws the 3×3 grid, and collects frames automatically:

- Space starts; Escape cancels.
- Each target is shown for 800 ms, then frames are collected until 20 samples
  or 1.5 s; the next target follows.
- After the grid, the full RMSE is shown for 2 s.
- `--quick` then adds a center-target sample and refits without restarting.
- Unless `--skip-live`, a red dot follows the predicted gaze for
  `--live-seconds`.

The window targets the presenter monitor with eframe 0.37's
`ViewportBuilder::with_monitor` (MSRV 1.95). The S5 window used eframe 0.33 and
`display.presenter_origin`; S6 removed the origin and moved every window to
0.37. On this machine:

```toml
[display]
presenter_monitor = 1
console_monitor = 0
width = 1440
height = 2560
fullscreen = true
```

`xwininfo` confirms the resulting window is `1440x2560+2560+0`, exactly on
DP-3. `with_monitor` requests borderless fullscreen on the indexed monitor and
is also the reliable path on Wayland.

**S6 presenter finding:** a new eframe viewport paints once at 800×600 before
the window manager applies borderless fullscreen. The presenter therefore
reports "ready" only when the viewport fills `ViewportInfo::monitor_size`, and
it recomposes the current scene if the surface changes later. The first ready
paint measured 1440×2560 at 1.167 px/pt. A four-image debug run then presented
at 60.0 Hz with ~7% coalesced frames and sub-millisecond trial lateness.

Calibration targets are fitted in the **window's physical pixels**, not the
configured monitor size. This keeps the model, the drawn targets and the live
dot in one coordinate system even if the window manager constrains the window.

## Validation runs

A scripted smoke test sent a synthetic Space via XTest, used
`--samples 5 --quick --live-seconds 2`, and captured the presenter monitor at
several phases. Observed:

- Prelude grid on DP-3; all nine targets and the instruction visible;
- nine target records, a full fit and the RMSE report screen;
- quick recalibration with ten samples;
- 60 live predictions and the red-dot live screen.

The RMSE values from this run (374 px full, 481 px quick in one run; 270/273 px
in an earlier run) are **not calibration measurements**: no human fixated the
targets. They validate the state machine, the geometry and the rendering only.

Feature drift is visible even in these static runs: NW moved from 178.23 to
177.66 over nine targets, and earlier from 164.67 to 170.61 under different
lighting. This confirms that absolute features plus quick recalibration are the
right design.

The terminal flow remains available with `--terminal` for headless contexts.
It prints target coordinates but does not draw them.

## Known limitation: camera identity

`usb_index` is a zero-based device index, not a USB ID. `/dev/videoN` ordering
can change across reboots or USB reconnects. The Yet camera is currently
`video2`; verify before a session:

```sh
for v in /sys/class/video4linux/video*; do
  printf '%s %s\n' "$(basename "$v")" "$(cat "$v/name")"
done
```

`yet-zero camera --device N` overrides the configured index for a single run. A
name/USB-ID selector is a candidate for S6/S7.

## Cross-platform

- Windows: `cargo check --target x86_64-pc-windows-msvc --workspace
  --all-targets` and the same target with Clippy `-D warnings` pass with
  `nokhwa` `input-msmf`. Runtime and the privacy prompt are untested.
- macOS: not cross-checked here. SPK-3 showed that `objc_exception` needs a
  macOS SDK and Xcode Command Line Tools; runtime, TCC permission and the
  `nokhwa_initialize` wait are untested.
- The camera protocol types are asserted `Send` in `tests/camera.rs`.

## Pending

- A valid **human** calibration on the lab machine, recording RMSE and notes
  (CV-7).
- macOS and Windows camera runtime, permission prompts and first-frame
  behavior.
- Appending a real eye-ROI frame to the CV-4 parity fixtures with
  `camera --save-roi`.
- Revisiting per-monitor selection: eframe 0.37 has `with_monitor` but raises
  the MSRV to 1.95.
