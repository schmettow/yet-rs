# SPK-5 — Camera capture and calibration on the lab machine

**Status:** Linux camera path works. MJPG is the stable choice on this camera.
CV-6's flow is implemented and exercised; a human calibration RMSE is still
required to close CV-7. Windows compiles; macOS and Windows runtime remain
untested.

**Question:** Does the Sprint 5 camera adapter capture reliably at the target
resolution, which format should the lab configuration use, and does the
calibration flow work end to end?

## Hardware and build

- OS: Linux 6.18.7-76061807-generic, 2026-10-25.
- Camera: Logitech StreamCam on `/dev/video0`; the user is in the `video` group.
- Driver reports 640×480 at 30 fps; formats advertised: YUYV, MJPG, NV12.
- Adapter: `linuxvideo` 0.3.5 through `yet_core::camera`; no bindgen, no
  libclang, no C compiler.
- Capture runs on a dedicated thread into a one-slot mailbox. Frames are
  decoded to `GrayFrame` before publication.
- Measurements use a release build. MJPEG decoding is CPU-bound in a debug
  build and does not sustain the frame rate there.

## Capture measurements

Command:

```sh
cargo run --release -p yet-app -- camera examples/demo --seconds N
```

| Build | Format | Frames published | Elapsed | fps | corrupt | dropped | errors |
|---|---|---:|---:|---:|---:|---:|---:|
| release | MJPG | 78 | 3.00 s | 26.0 | 6 | 0 | 0 |
| release | YUYV | 48 | 3.00 s | 16.0 | 35 | 0 | 0 |
| debug | MJPG | 24 | 2.00 s | 12.0 | 5 | 0 | 0 |

`corrupt` counts frames with the V4L2 error flag, truncated buffers, undecodable
MJPEG and sampled all-black frames. They are skipped, not fatal. Several of the
six MJPG corrupt frames occur during warm-up. The rate is computed from
published frames over the whole interval, so warm-up is included.

YUYV is markedly worse on this camera than in SPK-3 (16 fps and 35 corrupt
frames versus 17.7–24.1 fps and 6–21). MJPG is the configured default in
`examples/demo/config.toml`. `format = "auto"` enumerates Gray, YUYV, MJPG; this
camera exposes no Gray format, so auto selects YUYV. The lab configuration must
set `format = "mjpeg"`.

Last-frame quadrant means for the centered ROI
`x=160 y=120 320×240`:

| Format | NW | NE | SW | SE |
|---|---:|---:|---:|---:|
| MJPG | 139.06 | 103.29 | 80.74 | 65.43 |
| YUYV | 131.88 | 100.32 | 84.47 | 69.02 |

These are single frames at different times under uncontrolled illumination;
they are **not** a format comparison and must not be used as calibration data.

## Calibration flow plumbing

Command, with ten newlines piped to answer the prompts (no human fixations):

```sh
printf '\n\n\n\n\n\n\n\n\n\n' |
  cargo run --release -p yet-app -- calibrate examples/demo \
    --samples 3 --quick --live-seconds 1
```

Observed:

- full grid: 9 samples, RMSE **447.19 px**;
- quick recalibration: 10 samples, RMSE **431.54 px**;
- 9 live predictions printed.

**The RMSE values are not calibration measurements.** No participant fixated
the targets and the same scene was visible throughout. The run validates the
camera, frame averaging, sample recording, fit, refit without reset and the live
prediction printout. It also exposes a relevant behavior: quadrant means drifted
from NW 164.67 to 170.61 over the run under auto-exposure. This is exactly why
the model uses absolute features, why quick recalibration matters and why a
valid RMSE requires a human in the loop.

The command does not draw the targets yet; it prints their monitor coordinates.
Sprint 6 adds the presenter, Sprint 7 the drawn calibration UX. Until then, run
the command with the presenter monitor visible and look at the printed points.

## Cross-platform

- Windows: `cargo check --target x86_64-pc-windows-msvc --workspace
  --all-targets` and the same target with Clippy `-D warnings` pass with
  `nokhwa` `input-msmf`. Runtime and the privacy prompt are untested. The
  camera protocol types are asserted `Send` in `tests/camera.rs`.
- macOS: not cross-checked here. SPK-3 showed that `objc_exception` needs a
  macOS SDK and Xcode Command Line Tools; runtime, TCC permission and the
  `nokhwa_initialize` wait are untested.
- CI runs the Linux, macOS and Windows builds; only Linux has camera hardware in
  this environment.

## Pending

- A valid human calibration on the lab machine, recording RMSE and notes (CV-7).
- macOS and Windows camera runtime, permission prompts and first-frame behavior.
- Appending a real eye-ROI frame to the CV-4 parity fixtures with
  `camera --save-roi`.
