# yet-rs

A Rust port of the Python `yet_zero` eye-tracking experiment runner.

> **Status:** Sprints 0–9 implemented. Sprint 10 adds the naked-compositor
> launcher/profile, the GNOME/XWayland decision record, overlay coalescing
> counters and soak output, and the platform-matrix scaffold. The parity
> increment adds unchanged loading of Python `yet_zero` workspaces, a camera
> device selector, a presenter-only window and a purely visual follow circle.
> The workspace runs
> an experiment without a window or in a two-window console: the console controls
> the session, calibrates the tracker and shows a camera preview, while a
> deferred fullscreen viewport on the presenter monitor draws static images and
> pre-decoded clips with audio. A click-through gaze overlay is available as an
> eframe viewport on Windows/macOS and as a Wayland layer-shell surface on
> KDE/wlroots/Sway and naked compositors. Feature extraction and the linear gaze
> model are verified against NumPy, OpenCV and scikit-learn; the portable
> prediction path builds for Cortex-M without `std`. The pilot usability test
> (UX-4), the Windows/macOS runtime matrix and the 60-minute overlay soak are
> still pending.

## What it will do

- Present static images and pre-decoded video clips on a chosen monitor.
- Calibrate a webcam-based tracker from four quadrant-brightness features.
- Drive experiments from a separate experimenter console.
- Draw a click-through gaze overlay over the desktop.
- Write trial and frame data to CSV for the existing R pipeline.
- Run on Windows, macOS and Linux without OpenCV or a C/C++ toolchain.

## What exists now

The workspace is organized as three publishable libraries plus one binary. The
dependency direction is strictly `yet-zero -> yet-egui -> yet-stim -> yet-core`.

- `yet-core` — abstract, `#![no_std]` gaze essentials. Monotonic `SessionTime`,
  the `Clock` seam, camera roles and the borrowed `FrameSet`, `GrayFrame` and
  `EyeRegion`, canonical `quad_brightness` with the odd-dimension split rule,
  Rec. 601 luma matching OpenCV, the `FeatureExtractor`/`QuadExtractor` seam,
  `GazePredictor`, the versioned `ModelParameters` block and the
  `LinearPredictor`. The non-default `yet-zero` feature adds the reference
  Quadbright compressor plus the host-side linear trainer (centered SVD least
  squares via `nalgebra`), the calibration buffer and the Euclidean RMSE report.
  Without the feature the crate builds for `thumbv6m` and `thumbv7em`.
- `yet-stim` — stimulus, media and experiment protocols with no UI framework.
  The pure `step` engine, `Experiment.csv`/`Stimuli.csv`/`config.toml` loading,
  deterministic planning, the calibration flow (settle, collect, report, quick
  recalibration), a yet_zero-compatible CSV sink, physical-pixel geometry and
  `Scene`/`DrawCommand`, PNG sequences, WAV/rodio audio, playback, presenter and
  overlay traits, the `RecordingPresenter`, and the Wayland layer-shell overlay
  with its software rasterizer.
- `yet-egui` — every egui/eframe component. The monitor-targeted deferred
  presenter (images, pre-decoded sequences, text, circles, lines, cached blur,
  texture reuse, dropped-frame and audio-drift statistics), the experimenter
  console (controls, calibration panel, monitor selection, camera preview,
  bounded log), and `EguiOverlay`, a transparent click-through viewport for
  Windows/macOS and best-effort X11.
- `yet-zero` — the `yet-zero` binary and a test-only library target. It owns the
  platform camera adapters (`linuxvideo` on Linux, `nokhwa` on macOS/Windows),
  the latest-frame mailbox, the session driver, the headless runner, the CLI and
  the overlay-backend selection that binds the `yet-stim` protocols to the
  `yet-egui` implementations.
- Python `yet_zero` workspace compatibility: `Config.csv` plus
  `Stimuli/Stimuli.csv` loads unchanged; `SLIDE_TIME`, `SHUFFLE`/`SEED`,
  `STIM_FILE` and `File` map to the native model, with results in
  `Data/Yeta1_...csv`. An optional `config.toml` selects monitors and machine
  settings. Python workspaces open as a 900×900 window by default, matching the
  prototype.
- Console **Camera** selector with `Scan` and `yet-zero devices`: Linux V4L2,
  Windows/macOS `nokhwa`. Selecting another device reopens capture and drops
  the camera-specific calibration.
- `yet-zero present <workspace>`: a presenter-only window with no console.
  **F** toggles a purely visual gaze circle; **Space** starts/pauses; **F7/F9**
  calibrate; **F8/Esc** abort or quit. The console has the same **F** follow
  toggle.
- `tools/run-naked.sh` — renders the two-output Sway profile and starts a naked
  compositor session, with `--render-only`, output-name, participant and binary
  overrides. The profile disables XWayland and keeps the console on one output
  and the presenter fullscreen on the other.
- [`docs/overlay-decision.md`](docs/overlay-decision.md) — the GNOME/XWayland
  decision, alternatives and troubleshooting. The recording sheet for
  Windows/macOS/Linux and HiDPI/multi-monitor/soak results is
  [`docs/platform-matrix.md`](docs/platform-matrix.md).

Feature extraction and the linear gaze model are verified against NumPy,
OpenCV and scikit-learn; fixtures are generated by
[`tools/cv4_fixtures.py`](tools/cv4_fixtures.py) and documented in
[`docs/fixtures/CV-4.md`](docs/fixtures/CV-4.md). The portable prediction path
builds for Cortex-M without `std`.

## Build

Requires Rust 1.95 or newer. The MSRV follows eframe 0.37, which provides
`ViewportBuilder::with_monitor` for reliable multi-monitor fullscreen placement.

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo doc --workspace --no-deps --all-features

# Portable gaze essentials; the targets must be installed with rustup.
cargo check -p yet-core --no-default-features --target thumbv7em-none-eabi
cargo check -p yet-core --no-default-features --target thumbv6m-none-eabi

# Hardware smoke test; needs the YET camera (YET_TEST_DEVICE overrides the index).
cargo test -p yet-zero --all-features --lib -- --ignored the_yet_camera_delivers_frames

cargo run -p yet-zero -- plan examples/demo
cargo run -p yet-zero -- media examples/media-demo
cargo run -p yet-zero -- devices
# Overlay check; X11 auto-selects the best-effort egui path.
cargo run --release -p yet-zero -- overlay examples/demo-gui --seconds 10
cargo run --release -p yet-zero -- run examples/demo-gui --participant demo01
cargo run --release -p yet-zero -- present examples/demo-gui --participant demo01
cargo run --release -p yet-zero -- run examples/media-demo --participant demo01
cargo run -p yet-zero -- run --headless examples/demo --participant demo01
cargo run --release -p yet-zero -- camera examples/demo --seconds 3
cargo run --release -p yet-zero -- calibrate examples/demo --quick
cargo run --release -p yet-zero -- calibrate examples/demo --terminal
```

## Headless runs

`yet-zero run --headless` needs no window and no display. It walks the plan
with a 10 ms synthetic clock: each tick jumps to the next frame step or trial
deadline, so a five-trial demo finishes in milliseconds. Output:

```text
Result: examples/demo/yet1_demoMartin_demo01.csv
Trials: 5 of 5 completed
Samples: 0
```

Use `--output <csv>` to choose the result path. `--participant` defaults to
the Unix time at session start, as in the Python prototype. The headless runner
does not open the camera yet, so the result file contains only the schema
header; live capture is exercised by the `camera` and `calibrate` commands. The
trial scheduler and protocol wiring are still exercised end to end.

### Result CSV

Columns are exactly `Exp,Part,Stim,time,x,y,x_pro,y_pro`, matching yet_zero.
`Exp` concatenates the experiment id and the experimenter, `Part` is the
participant id, `x`/`y` are gaze coordinates in stimulus pixels, and
`x_pro`/`y_pro` are normalized to the stimulus size. Times are epoch seconds
with microsecond precision. Every trial boundary flushes the file, so an
aborted session keeps the completed trials.

## Console and presenter

`yet-zero run <dir>` opens two native windows from one eframe event loop:

- the **console** (root viewport) with controls, progress, presenter
  statistics and a bounded log;
- the **stimulus** (deferred viewport) borderless fullscreen on
  `display.presenter_monitor`. `ViewportBuilder::with_monitor` is the only
  reliable monitor-targeting mechanism on Wayland and works on X11 too.

The deferred viewport only repaints when a scene changes. The console repaints
about ten times per second for its clock, and that leaves the stimulus window
untouched; the console therefore cannot cause timing jitter in the stimulus.

The console targets a notebook's main screen. It opens at 1400×860 logical
points, clamped to 92% of the monitor, and scales text, padding and controls 2×.
A wide window uses two columns: controls, calibration, progress and presenter
statistics on the left, camera preview and the log on the right. Narrow windows
fall back to one column, and the left column scrolls independently. Log lines
wrap instead of being clipped. The camera preview is served at up to 480×320.

Controls: **Start** (F5), **Pause/Resume** (F6 toggles; only the valid action
is enabled), **Abort** (F8), **Calibrate** (F7, full 3×3 grid), **Quick** (F9,
center target; requires an existing fit), **Cancel calibration** (Esc), the
**Camera preview** toggle (F10), and **Follow** (F; draws a gaze circle over the
stimulus). The presenter selector lists the monitors reported by the platform,
labelled from 1 with resolution and output name; the configuration index stays
zero-based. On a two-monitor desk the presenter defaults to monitor 2 (index 1).
Changing the selection recreates the stimulus window borderless-fullscreen on
that monitor, including after the fullscreen box was unticked. If the configured
index is out of range, the console logs a warning and uses the last monitor
instead of leaving a windowed presenter. Presenter and camera settings are
locked while a session or calibration is active. If the console monitor matches
the presenter monitor, the console warns that the fullscreen stimulus will cover
it.

The **Camera** selector is populated from the platform device list (`Scan`
refreshes it). On Linux it filters V4L2 capture nodes; on Windows/macOS it uses
the `nokhwa` device query. Selecting another camera stops and reopens the
capture thread and invalidates the calibration, because the model is
camera-specific. If enumeration is unavailable, the control falls back to a
numeric index. `yet-zero devices` prints the same list from the CLI; `--device`
preselects an index.

`examples/demo-gui` is the static-image demo for this path: four images shared
with `examples/demo`, 3 s each, no sequence and no audio. The S6 full run on the
two-monitor workstation, before the camera was wired into the session, measured:

```text
finished 12.0 s · 4/4 trials · 720 presented · 60.0 Hz measured paint rate
~7% coalesced frames · all trials within 0.4 ms of their deadline
Samples: 0 (the camera was not wired into the session yet)
```

The S7 path keeps the same scheduler and presenter. The camera now runs on the
controller thread, so a calibrated session writes gaze rows; without a
calibration the CSV keeps its header and no gaze rows, and the console says so.

**Compositor dimming.** KWin's *Dim Inactive* effect multiplies an unfocused
window by 75%. While the console has focus, the stimulus is dimmed, which
changes its luminance and biases quadrant-brightness tracking. yet-rs asks the
stimulus viewport to take focus on Start/Resume and the console shows a yellow
warning when the stimulus is not focused, but some window managers refuse the
focus request. For luminance-sensitive work, disable *System Settings → Window
Management → Desktop Effects → Dim Inactive* or click the stimulus window before
the session. This is recorded in the S6 implementation note.

**Not yet:** the console window is placed by the desktop rather than by
`display.console_monitor`, the S7 interactive validation and pilot test (UX-4)
are still outstanding, and Windows/macOS media playback is compile-checked only.
If the console reports "waiting for fullscreen", confirm that the window manager
granted fullscreen to the stimulus viewport; an out-of-range monitor index is now
clamped and logged.

### Presenter-only mode

```sh
cargo run --release -p yet-zero -- present examples/demo --participant demo01
```

This opens only the stimulus window (or the 900×900 window of a Python
workspace); there is no console. It is the quickest way to test the tracker on
a single monitor. Keys: **F** toggles the follow circle, **Space** starts or
pauses, **F7** runs a full calibration, **F9** a quick recalibration, **F8**
aborts and **Esc** quits. Follow is a live tracker view: while idle or paused it
paints the circle over the current (or a neutral white) scene and keeps the
camera polling, so it is visible immediately after calibration without starting
a trial. The circle is purely visual: it is composed into a temporary scene for
painting and never changes the trial scene or a CSV row. A small status strip
shows calibration and camera errors outside a running trial. `--follow` starts
with the circle enabled, which is useful for an automated or scripted check.

### Calibrating in the console

The console is the supported tracking path. The standalone `calibrate` command
remains useful for camera checks without an experiment session.

1. Press **F10** (or tick *Camera preview*). The controller thread opens the
   configured camera and shows the cropped eye region at up to 240×160, the
   capture rate, a 120×96 gaze map and the current gaze in stimulus pixels and
   normalized coordinates. A camera error leaves the experiment runnable and
   offers **Retry**.
2. Press **F7**. The presenter shows the 3×3 grid, the active target in orange,
   and the console shows target progress and collected frames. Each target
   settles for 800 ms, then 20 frames are collected within a 500–1500 ms window.
   The report (sample count and RMSE) stays on the presenter for 2 s.
3. **Esc** cancels at any time. A cancelled or failed full calibration restores
   the previous model and buffer; a failed quick run keeps the last full report.
   Errors name the cause and the console states whether a calibration is still
   active.
4. **F9** reruns a single center target. It requires an existing fit (calibrate
   with F7 first). The console reports the new RMSE and the drift against the
   previous quick run, or against the full run for the first quick run.
5. **F5** starts the experiment. Gaze samples are mapped to stimulus pixels and
   normalized coordinates and written per tick; without a fit the CSV keeps its
   header and no gaze rows. Full calibration is available while idle or after a
   session; quick recalibration also while paused, and the session stays paused
   when it finishes.

The preview is a diagnostic view on the console. It never touches the stimulus
window, and preview updates are capped at about 15 Hz while the controller keeps
its 60 Hz tick.

### Pilot test (UX-4)

The pilot needs two or three people and is still outstanding. For each pilot
participant:

```sh
cargo run --release -p yet-zero -- run examples/demo-gui --participant pilot1
```

Record the time from console start to the first trial (the console logs it), the
calibration RMSE and drift, whether the participant needed help for F7/F9, and
the result CSV. Stop after the first session and note the top three usability
problems, then schedule fixes before the S11 pilot. Eye-region crops are personal
data: keep them on the recording machine and delete them after the session unless
the participant consented to storage.

## Gaze overlay

The overlay is a separate, click-through layer above the desktop. It shows the
latest gaze point as a small white-haloed red dot and never receives input.

### Backends

| Platform | Backend | Status |
|---|---|---|
| Windows, macOS | `egui` viewport | Implemented; runtime matrix pending ([`docs/platform-matrix.md`](docs/platform-matrix.md)) |
| Linux Wayland, KDE/Sway/wlroots | `layer-shell` | Implemented; protocol validated against a live compositor |
| Linux Wayland, naked compositor | `layer-shell` | Launcher and profile in [`docs/naked-compositor.md`](docs/naked-compositor.md) |
| Linux Wayland, GNOME | none | **Unsupported**; decision record in [`docs/overlay-decision.md`](docs/overlay-decision.md) |
| Linux X11 | `egui` (best effort) | May not composite and may take focus; X11 is deprecated |

`auto` selects layer-shell on Wayland, the egui viewport on Windows/macOS, and
the best-effort egui overlay on X11 with a warning. If no backend is available,
the session still runs; an explicit backend that cannot work is a startup
error.

Layer-shell selects outputs in the compositor's order, which is not guaranteed
to match eframe's monitor index. On a multi-output system, start with
`yet-zero overlay` and verify that the marker appears on the presenter monitor;
use a single-output naked compositor when the mapping must be unambiguous.

### Configuration

```toml
[presenter]
overlay = "auto"   # auto | off | egui | layer-shell

[overlay]
monitor = 1        # must equal display.presenter_monitor in v1
```

In v1 the overlay is bound to the presenter monitor and requires the presenter
to be fullscreen; changing the monitor or leaving fullscreen hides the overlay.
Restart to move it. `xwayland` is accepted by the parser but returns the
documented GNOME error.

### Manual validation

```sh
# Animated marker over the desktop; 10 s by default.
cargo run --release -p yet-zero -- overlay examples/demo-gui --seconds 10
```

On Windows/macOS and layer-shell Wayland the marker should follow a circle and
clicks should pass through to the window below. On X11 `auto` selects the
best-effort egui overlay and prints the warning shown above; it may not
composite and `with_active(false)` is not supported there. An explicit
`presenter.overlay = "egui"` gives the same path without the automatic fallback.

The command prints a summary when it stops:

```text
overlay stats: 600.0 s · requested=18000 painted=17995 superseded=4 outstanding=1 errors=0 · 30.0 painted/s
```

`superseded` is expected when the overlay coalesces gaze updates; a rising
`outstanding` or a non-zero `errors` is a failure. Use `--seconds 3600` for the
60-minute soak required by the [platform matrix](docs/platform-matrix.md).

### Troubleshooting

| Symptom | Cause and action |
|---|---|
| `layer-shell requires a Wayland session` | You are on X11 or have no Wayland socket. Use a Wayland session, or `overlay = "off"`; X11 is deprecated |
| `zwlr_layer_shell_v1 is unavailable` | GNOME/Mutter is unsupported. Use KDE, Sway/wlroots or the [naked-compositor profile](docs/naked-compositor.md) |
| Overlay on the wrong output | Layer-shell uses the compositor's output order. v1 requires `overlay.monitor == display.presenter_monitor`; verify with `yet-zero overlay` |
| Marker does not appear | The marker is shown only while a session runs and the presenter is fullscreen; check the console **Gaze overlay** checkbox |
| Clicks do not pass through | Layer-shell uses an empty input region; X11 is best effort. Record the platform in the matrix instead of adding a fallback |
| Window loses focus | Layer-shell has keyboard interactivity `None`; egui uses `with_active(false)`. Report a reproducible platform case |

The full rationale, alternatives and GNOME decision are in
[`docs/overlay-decision.md`](docs/overlay-decision.md).

During a session, the console has a **Gaze overlay** checkbox. The controller
feeds the layer-shell backend at gaze rate; the console viewport repaints at
about 30 Hz while the overlay is visible. If the overlay backend stops, the
console shows the error and unchecks the box.

v1 shows the marker while an experiment is **running**. Free viewing without
trials (for example, a website-navigation task) is a post-v1 mode; the overlay
and the gaze pipeline are already separate from trial logic, so it does not
require a redesign.

## Camera and calibration

The experiment config carries the display geometry and the camera:

```toml
[display]
presenter_monitor = 1                 # zero-based: 1 is monitor 2
console_monitor = 0
width = 1440
height = 2560
fullscreen = true

[camera]
usb_index = 2                 # the GEMBIRD/Yet camera on this workstation
width = 640
height = 480
fps = 30
format = "yuyv"               # auto, gray, yuyv or mjpeg
eye_region = [0.25, 0.25, 0.5, 0.5]   # normalized x, y, width, height
```

The Yet camera is the **GEMBIRD/AppoTech UVC** device, usually `/dev/video2`; it
advertises YUYV only at 640×480 and sustained 25.6 fps in the SPK-5 measurement.
`/dev/video0` is the StreamCam, which is faster in MJPG but is not the tracker
camera. Device indices can move across reboots. Use `yet-zero devices` or the
console **Camera** selector to see the current names and indices; `--device N`
preselects an index for a run.

```sh
# List cameras as index, name and device path.
cargo run --release -p yet-zero -- devices

# Measure the camera, print format/fps/statistics and the last quadrant means.
cargo run --release -p yet-zero -- camera examples/demo --seconds 3
cargo run --release -p yet-zero -- camera examples/demo --device 0 --seconds 3

# Also write the raw eye ROI as an 8-bit grayscale file for CV-4 parity.
cargo run --release -p yet-zero -- camera examples/demo --save-roi /tmp/eye.gray

# Graphical calibration on the presenter monitor (default).
cargo run --release -p yet-zero -- calibrate examples/demo --quick

# Terminal fallback for headless machines.
cargo run --release -p yet-zero -- calibrate examples/demo --terminal
```

The graphical calibration opens a borderless window sized to the configured
presenter area. Press **Space** to start and **Escape** to cancel. Each orange
target is shown for 800 ms, then frames are collected automatically; the RMSE
appears for 2 s, then `--quick` adds a center-target refit. With `--skip-live`
the window closes after the report; otherwise a red dot follows the prediction
for `--live-seconds`.

`presenter_monitor` is a zero-based index in the platform's monitor order; the
console shows one-based labels. The presenter uses
`ViewportBuilder::with_monitor`, which requests borderless fullscreen on that
monitor, and asks for fullscreen explicitly so that re-enabling it on an
existing viewport works. There is no absolute desktop position in the
configuration. On this workstation `xwininfo` confirms the stimulus window at
`1440x2560+2560+0` on DP-3, labelled **2** in the console. An out-of-range index
is clamped to the last monitor and logged. Calibration targets are fitted in the
window's actual physical pixels, so the model, the drawn grid and the live dot
stay consistent even if the window manager adjusts the window.

Platform notes:

- **Linux:** the user must be able to read `/dev/videoN` (usually the `video`
  group). `linuxvideo` needs no C toolchain.
- **Windows:** camera privacy settings must allow desktop apps; nokhwa uses the
  Media Foundation backend.
- **macOS:** install Xcode Command Line Tools, allow camera access when TCC
  prompts, and expect the permission dialog on first use. The macOS runtime is
  not yet tested here.

See [`docs/spikes/SPK-5.md`](docs/spikes/SPK-5.md) for the measured Linux
results, the two-monitor geometry and the open hardware checks.

## Pre-decoded sequences and audio

Video stimuli are never decoded at runtime. `tools/prepare_media.sh` calls
FFmpeg once at setup time and writes a PNG directory plus an optional WAV:

```sh
tools/prepare_media.sh clip.mp4 stimuli/clip 30
```

```text
stimuli/clip/frames/frame_00001.png ...
stimuli/clip/audio.wav                # only when the source has audio
```

The suggested `Stimuli.csv` row is `id,kind,path,fps,duration_ms,loop,audio,block`,
for example `clip,sequence,stimuli/clip/frames,30,3000,false,stimuli/clip/audio.wav,B1`.
Validate without opening a window:

```sh
cargo run -p yet-zero -- media examples/media-demo
```

That prints frame count, fps, duration, size and the WAV metadata, and warns when
the track is more than 100 ms longer or shorter than the frames. The trial's
`duration_ms` stays authoritative; a shorter clip repeats its last frame and a
looped clip wraps.

Playback runs on the controller thread. Each new frame is decoded before the
painting state is locked and uploaded into a reused texture, so the UI thread
never reads the disk. When a WAV is loaded, the audio clock is master and frames
are selected from `get_pos()`; otherwise experiment time is master. Pause and
Resume reach both clocks. Late frames are skipped, not queued, and the console
shows `frames dropped`, `media <s>`, `audio drift` and underruns. The default
`audio.output_latency_ms` is `0`; measure the device before changing it.

`examples/media-demo` uses the SPK-4 3 s, 30 fps clip with a 1 kHz beep at 1 s
and a matching 48 kHz WAV. In the S8 release run the console showed
`media 2.10 s · audio drift +24.9 ms` at 2.1 s of experiment time, so the beep
and frame 30 stay within one frame. Between 7 and 14 of 63 frames were dropped
on the portrait 1440×2560 presenter while the UI painted; they are counted and
do not stall the session. Pause froze both clocks and Resume finished the trial
14.7 ms late. Windows and macOS audio playback are compile-checked only.

WAV is decoded with `hound` and fed to `rodio::buffer::SamplesBuffer`; Symphonia
and its MPL-2.0 license are not used. On Linux, building rodio requires the ALSA
development package (`libasound2-dev` on Debian/Ubuntu). `audio.enabled = false`
selects a null player; a named `audio.device` must match a `cpal` output name.

```toml
[audio]
enabled = true
device = "default"
volume = 0.8
output_latency_ms = 0   # measure per device; 0 until then
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

See [`examples/demo`](examples/demo) for images plus a silent sequence,
[`examples/demo-gui`](examples/demo-gui) for the image-only console demo, and
[`examples/media-demo`](examples/media-demo) for a 3 s, 30 fps clip with a WAV
and a beep at 1 s. Run:

```sh
cargo run -p yet-zero -- plan examples/demo
cargo run -p yet-zero -- plan examples/demo-gui
cargo run -p yet-zero -- media examples/media-demo
```

### Python `yet_zero` workspaces

A workspace from the Python prototype is detected by `Config.csv` plus a
`Stimuli/` directory; no conversion is required:

```text
my-python-workspace/
  Config.csv
  Stimuli/
    Stimuli.csv
    18.jpg
    ...
```

- `Config.csv` supplies `USB`, `EXP_ID`, `EXPERIMENTER`, `WIDTH`, `HEIGHT`,
  `SLIDE_TIME`, `STIM_FILE` and `SHUFFLE`. Missing optional values fall back to
  the Python defaults; `SEED` is an accepted extension for a reproducible
  shuffle.
- `Stimuli/Stimuli.csv` uses the Python `File` column; every row is an image
  shown for `SLIDE_TIME`. The file name is the stimulus id, so result rows keep
  the Python `Stim` values.
- `STIM_FILE` may select `Stimuli_short.csv` or another table in `Stimuli/`.
- Without a `config.toml`, the presenter is a 900×900 window on monitor 1
  (index 0), the desktop overlay is off, and the camera index comes from
  `Config.csv`. This matches the prototype on a single monitor. Add a
  `config.toml` to choose monitors/fullscreen and other machine settings; its
  `camera.usb_index`, when present, wins over `Config.csv`.
- Results go to `Data/` with the Python prefix `Yeta1_...csv`; native
  workspaces keep `yet1_...csv` at the workspace root.

```sh
cargo run --release -p yet-zero -- run /path/to/my-python-workspace --device 0
cargo run --release -p yet-zero -- present /path/to/my-python-workspace --device 0
```

`yet-zero plan <dir>` prints `Workspace: yet_zero Python (Config.csv)` for this
layout.

**Known parity difference:** the Python prototype runs a quick center-target
recalibration before every stimulus. yet-rs keeps quick recalibration manual
(**F9** in the console or presenter-only window); there is no automatic
per-trial quick calibration yet. Full calibration is also automatic in yet-rs
(targets are collected without a key press), which is an intentional
improvement.

## Design

- The engine is independent of any UI. `step(state, event, now)` returns
  effects; the application executes them.
- Protocol coordinates are monitor-local physical pixels.
- Protocol messages carry media identifiers and owned text, never image buffers.
- `yet-core` and `yet-stim` must not depend on a windowing toolkit.
- The stimulus is a **deferred** viewport. Console painting never forces a
  stimulus repaint, and the presenter never reads experiment state.

The reasoning behind these choices is in
[`DEVELOPMENT_PLAN.md`](DEVELOPMENT_PLAN.md),
[`UI_analysis.md`](UI_analysis.md) and [`CV_analysis.md`](CV_analysis.md).

## License

GPL-3.0-only. Logic ported from the Apache-2.0 `yet_zero` prototype retains its
notice.
