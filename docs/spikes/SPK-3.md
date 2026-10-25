# SPK-3 — camera capture and quadrant parity

**Status:** Go with two corrections: explicit Rec. 601 luma and format-aware calibration. YUYV throughput is variable on the test camera; MJPG is the stable fallback.

**Question:** Do `linuxvideo` and `nokhwa` build without a C/C++ toolchain, can they capture at the target resolution, and do the Rust quadrant features match the Python prototype?

**Harness:** `spikes/spk3-camera` (Linux `linuxvideo`, macOS/Windows `nokhwa` compile paths) and `parity.py`.

## Build and device

- `linuxvideo` 0.3.5 builds with no `build.rs`, no `bindgen`, no `libclang`, and no `v4l2-sys`.
- Device: `/dev/video0`, driver `uvcvideo`, card `Logitech StreamCam`, USB.
- Advertised formats: `YUYV`, `MJPG`, `NV12`. NV12 is not required by CV R4 and was not implemented.
- Negotiated: `YUYV 640×480`, stride 1280, frame interval `1/30`.
- Compressed path: `MJPG 640×480`, stride 0, frame interval `1/30`; `zune-jpeg` decoded the camera's MJPG successfully, including its omitted Huffman tables.

## Capture results

| Format | Good frames | Corrupt/skipped | Elapsed | Measured fps | Notes |
|---|---:|---:|---:|---:|---|
| YUYV run A | 30 | 6 | 1.400 s | 21.4 | Timer before warm-up |
| YUYV run B | 30 | 7 | 1.203 s | 24.1 | Timer at first good frame |
| YUYV run C | 30 | 21 | 1.635 s | 17.7 | Same code, higher frame loss |
| MJPG | 30 | 1 | 1.003 s | 28.9 | Stable in this environment |

The harness writes a raw PNG per run. Raw camera images were deleted after measurement because they are personal data; the aggregate `spk3-quad.csv` and `spk3-quad-mjpg.csv` are retained.

The camera advertises 30 fps and accepted `1/30`, but YUYV did not sustain it. The 17.7–24.1 fps spread has possible causes in auto-exposure/lighting variation or USB scheduling; this spike did not isolate them. The tracker consumes the latest frame, so variable capture fps does not invalidate the design, but it does affect timing logs and any assumption of a fixed sample rate.

**Recommendation:** enumerate formats, prefer YUYV only if it sustains the target rate with few corrupt frames; otherwise prefer MJPG. Do not switch format mid-session.

## The Rec. 709 / Rec. 601 trap

The fixture is a 7×5 image with red, green, blue and gray quadrants. Python/PIL and OpenCV use **Rec. 601** luma. `image::DynamicImage::to_luma8()` uses **Rec. 709**.

| Quadrant (color) | Python/PIL (Rec. 601) | `image::to_luma8` (Rec. 709) | Rust explicit Rec. 601 |
|---|---:|---:|---:|
| NW (red) | 76 | 54 | **76** |
| NE (green) | 150 | 182 | **150** |
| SW (blue) | 29 | 18 | **29** |
| SE (gray) | 128 | 128 | **128** |

Using `to_luma8()` would have shifted every feature and silently broken calibration parity. The spike now uses:

```text
Y = round((299·R + 587·G + 114·B) / 1000)
```

for RGB→gray conversion (fixture and MJPEG). YUYV uses the camera's Y byte directly. **The S4 parity test must use the Rec. 601 fixture; do not add a `to_luma8()` shortcut.**

## Format-dependent features

The same scene, captured once as YUYV and once as MJPG (different moments), gave:

| Quadrant | YUYV mean | MJPG mean |
|---|---:|---:|
| NW | 100.4 | 103.7 |
| NE | 65.0 | 58.6 |
| SW | 76.7 | 75.5 |
| SE | 53.6 | 45.0 |

This is not a clean controlled comparison, but the differences are large enough to make the format part of calibration metadata. A format switch must trigger a refit; do not silently reinterpret a model fitted under another format.

## `nokhwa` cross-checks

- **Windows:** `cargo check --target x86_64-pc-windows-msvc -p spk3-camera` passes with `input-msmf`; no C build was observed.
- **macOS:** cross-check from Linux fails in `objc_exception`'s build script because there is no macOS SDK/compiler here (`cc: unrecognized command-line option '-arch'`). On a real Mac, Xcode Command Line Tools provide it. This is an Objective-C build step, so “Rust toolchain only” on macOS needs the CLT qualification. Runtime untested.

## Decision

**Go with corrections.**

- Use `linuxvideo` on Linux behind `CameraSource`; it satisfies the no-bindgen constraint.
- Use `nokhwa` with `input-msmf`/`input-avfoundation` on Windows/macOS; document macOS Xcode CLT.
- Convert RGB/MJPEG to luma with explicit Rec. 601; never use `to_luma8()`.
- Record camera format, resolution and ROI in calibration metadata; refit on format change.
- Keep YUYV as the default preference, with an automatic or configured MJPG fallback and a corrupt-frame counter.
- Skip warm-up corrupt frames instead of aborting the session.

## Follow-ups

- S5: implement the adapters, frame-loss policy and format fallback; measure sustained fps on the lab machine.
- S4/S5: add the Python parity fixture to the automated test suite.
- Compare YUYV Y against Python's `BGR2GRAY` on the same recorded frames, not just the RGB fixture.
- Run the macOS/Windows camera paths on real hardware; record permission prompts and first-frame behavior.
- Consider NV12 only if a target camera exposes no YUYV/MJPG; it is not required now.
