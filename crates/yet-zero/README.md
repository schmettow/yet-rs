# yet-zero

A webcam-based eye-tracking experiment runner with an experimenter console, a
fullscreen stimulus presenter and a click-through gaze overlay.

`yet-zero` is the ready-to-run application of the
[yet-rs](https://github.com/schmettow/yet-rs) toolkit. It is a Rust port of the
Python `yet_zero` prototype and loads existing Python experiment workspaces
without conversion.

## Status

yet-rs is still early-stage. The long-term goal is a complete research toolkit
and applications for eye tracking. The first external milestone — feature
parity and a drop-in replacement for the Python `yet_zero` prototype — is
achieved.

The main addition over the Python prototype is the **experimenter console**.
The console can stay on the experimenter's monitor while the stimulus runs
fullscreen on the participant's monitor. The console controls calibration,
shows a live camera preview, tracks progress and keeps a log; the participant
only ever sees the stimulus.

Linux is the tested platform. macOS and Windows camera runtime and the full
overlay matrix are still pending.

## Why use it

- **Two-window experiment control.** Console and presenter are separate
  viewports from one event loop.
- **Multi-monitor by design.** The presenter is placed on a chosen monitor;
  the console stays on the experimenter's screen.
- **Drop-in Python workspaces.** `Config.csv` plus `Stimuli/Stimuli.csv` load
  unchanged.
- **Calibration workflow.** Adjust the eye region, run a full 3×3 grid, run a
  quick center recalibration, and gate every trial with an automatic quick
  recalibration.
- **Pre-decoded media.** PNG sequences and WAV audio with audio-master pacing.
- **Headless mode.** Deterministic runs for CI and dry checks.
- **Presenter-only mode.** A linear participant flow with no console.

## Install

### 1. Install Rust

Install Rust 1.95 or newer with [rustup](https://rustup.rs):

```sh
rustup update stable
rustc --version
```

### 2. Install the system build dependencies

Linux, Debian/Ubuntu:

```sh
sudo apt update
sudo apt install build-essential pkg-config libasound2-dev
```

`build-essential` also installs a C++ compiler, which yet-zero does not use;
installing `gcc`, `pkg-config` and `libasound2-dev` is enough if you prefer the
minimal set.

Linux, Fedora:

```sh
sudo dnf install gcc pkgconf-pkg-config alsa-lib-devel
```

Linux, Arch:

```sh
sudo pacman -S gcc pkgconf alsa-lib
```

macOS:

```sh
xcode-select --install
```

This installs the Xcode Command Line Tools, which provide the system linker and
the macOS SDK. A full Xcode installation is not required.

Windows: install the **Visual Studio Build Tools** with the **Desktop
development with C++** workload, or just the MSVC build tools and Windows SDK
components. Rust uses the MSVC linker (`link.exe`); yet-zero contains no C or
C++ source.

**Pure Rust does not mean no native toolchain.** Rust still invokes the platform
linker, and a few dependencies bind system libraries: ALSA on Linux,
CoreAudio/AVFoundation on macOS, and Win32/Media Foundation on Windows. There is
no OpenCV, no C/C++ source to compile and no CMake step. The ALSA package is
required because `rodio`/`cpal` links against the system audio library. A working
GL or Vulkan driver is required at runtime for the egui/wgpu windows.

### 3. Install yet-zero

```sh
cargo install --locked yet-zero
yet-zero --help
```

`cargo install` builds from source, so the linker and platform SDK from step 2
must be present on the machine that installs the crate.

`cargo install` places the binary in `~/.cargo/bin`. If `yet-zero` is not found,
add that directory to `PATH` (rustup normally does this) or restart the shell.

The first build compiles a GUI stack and takes a few minutes.

### 4. Get the example workspaces

`cargo install` installs the binary only; it does not install the example
experiment workspaces. Clone the repository:

```sh
git clone https://github.com/schmettow/yet-rs
cd yet-rs
```

The commands below assume you are in that directory.

> **Single-monitor setup:** the bundled demos are configured for a two-monitor
> desk (`presenter_monitor = 1`). On a laptop with one screen, edit
> `examples/demo/config.toml` and `examples/demo-gui/config.toml` before the
> graphical commands: set `presenter_monitor = 0`, `console_monitor = 0`,
> `fullscreen = false`, and `width`/`height` to your screen resolution.
> `yet-zero devices` and the `--headless` run work without changes.

## First run

### No camera, no windows

```sh
yet-zero run --headless examples/demo --participant demo01 --output results/demo.csv
```

This exercises loading, planning, scheduling and CSV writing. The headless
runner does not open the camera, so the CSV has the header but no gaze rows.

### Find and test the camera

```sh
yet-zero devices
yet-zero camera examples/demo --seconds 3
```

`devices` prints camera indices and names. `camera` prints the negotiated
format, frame rate and the last Quadbright means. Use `--device N` to select a
camera.

The project was developed with the GEMBIRD/AppoTech UVC "Yet" camera (YUYV,
640×480). Other UVC webcams can work; check the negotiated format with
`yet-zero camera`. If the camera advertises YUYV only at 640×480, set
`format = "yuyv"` in `config.toml`.

Camera permissions:

- **Linux:** your user must be able to read `/dev/videoN` (usually the `video`
  group): `sudo usermod -aG video "$USER"`, then log out and in again.
- **macOS:** allow camera access when the TCC prompt appears.
- **Windows:** enable camera access for desktop apps in
  *Settings → Privacy → Camera*.

### Calibrate

```sh
yet-zero calibrate examples/demo --quick
```

The graphical calibration opens on the configured presenter monitor. Follow
the targets; the report shows the sample count and RMSE. Use `--terminal` only
as a fallback. In the full application, calibration is done in the console
(F7).

### Run with the console

```sh
yet-zero run examples/demo-gui --participant demo01
```

This opens two windows:

- **Console** — controls, progress, calibration, camera preview and log.
- **Presenter** — borderless fullscreen stimulus on
  `display.presenter_monitor`.

If the console and presenter would open on the same monitor, the console warns
you. Set the monitors in `config.toml` as described below.

### Presenter-only mode

```sh
yet-zero present examples/demo-gui --participant demo01
```

There is no console. The participant sees a linear flow:
Welcome → Adjust → Calibration → Stimuli → Debrief. **Space** moves forward,
**Backspace** moves back, **F11** toggles fullscreen and **Esc** leaves
fullscreen without cancelling.

## Two-monitor setup

Edit `config.toml` in the experiment directory:

```toml
[display]
# Zero-based platform monitor indices.
# The console labels them from 1, so index 1 is "2" in the console.
presenter_monitor = 1   # participant screen
console_monitor = 0     # experimenter screen
width = 1920
height = 1080
fullscreen = true

[camera]
usb_index = 0
width = 640
height = 480
fps = 30
format = "auto"
# Normalized [x, y, width, height] eye region; edit live in Adjust (F3).
eye_region = [0.05, 0.275, 0.9, 0.45]

[presenter]
backend = "egui"
overlay = "auto"

[overlay]
monitor = 1

[audio]
enabled = true
device = "default"
volume = 0.8
```

Notes:

- `presenter_monitor` and `console_monitor` are zero-based in the file; the
  console's selector shows one-based labels.
- Match `width` and `height` to the participant monitor's physical resolution.
- Use `fullscreen = false` for a windowed presenter while testing.
- On a single monitor, set both indices to 0 and `fullscreen = false`, or
  accept that the fullscreen stimulus covers the console.
- The console records `console_monitor`, but the desktop places the console
  window; the presenter monitor is the enforced one. Move the console once and
  the window manager will usually remember its position.
- Presenter and camera settings are locked while a session or calibration is
  active. The console can change the presenter monitor at runtime.

## The experimenter console

Recommended workflow:

| Step | Key | What it does |
|---|---|---|
| 1. Adjust | F3 | Enlarged camera image with the eye ROI outlined. Move the camera until the eye is inside the box. |
| 2. Calibrate | F7 | Full 3×3 calibration. The report shows sample count and RMSE. |
| 3. Start | F5 | Disabled until the adjustment is finished and a calibration is fitted. |
| Pause/Continue | F6 | One toggle button. |
| Quick recalibration | F9 | One center target; requires an existing fit. |
| Camera preview | F10 | Live preview, capture rate and gaze map. |
| Follow circle | F | Draws the predicted gaze over the stimulus; purely visual. |
| Reset | F4 | Returns a paused or finished session to Idle. |
| Exit | F8 | Available while paused; confirms before aborting and flushing. |

Each trial is gated by an automatic quick recalibration, matching the Python
prototype: the next stimulus is composed, the clock pauses, one center target is
shown over the blurred stimulus, and the clock resumes with the trial deadline
shifted.

The **PiP camera** checkbox in the console (key **P** in presenter-only mode)
draws the camera stream over the stimulus. The follow circle and PiP are visual
overlays; they never change the trial scene or the CSV.

## Python yet_zero workspaces

Point `yet-zero` at a Python workspace; no conversion is required:

```text
my-python-workspace/
  Config.csv
  Stimuli/
    Stimuli.csv
    18.jpg
    ...
```

```sh
yet-zero plan /path/to/my-python-workspace
yet-zero run /path/to/my-python-workspace --participant s01
```

`Config.csv` supplies `USB`, `EXP_ID`, `EXPERIMENTER`, `WIDTH`, `HEIGHT`,
`SLIDE_TIME`, `STIM_FILE` and `SHUFFLE`; `SEED` is an accepted extension.
Results go to `Data/` with the `Yeta1_...csv` prefix. Add an optional
`config.toml` to choose monitors, fullscreen and other machine settings; its
`camera.usb_index` wins over `Config.csv`.

## Results

Native workspaces write
`yet1_<experiment-id><experimenter>_<participant>.csv` at the workspace root;
Python workspaces write `Data/Yeta1_...csv`. Use `--output` to choose a path;
parent directories are created.

The columns are exactly:

```text
Exp,Part,Stim,time,x,y,x_pro,y_pro
```

`x`/`y` are stimulus pixels; `x_pro`/`y_pro` are normalized. Times are epoch
seconds with microsecond precision. The file is flushed at every trial
boundary, so an aborted session keeps the completed trials.

## Command overview

```text
yet-zero plan <experiment-dir>
yet-zero media <experiment-dir>
yet-zero devices
yet-zero run <experiment-dir> [--headless] [--participant <id>] [--output <csv>] [--device <n>] [--adjust]
yet-zero present <experiment-dir> [--participant <id>] [--output <csv>] [--device <n>] [--follow] [--pip-camera] [--adjust]
yet-zero camera <experiment-dir> [--device <n>] [--seconds <s>] [--save-roi <file>]
yet-zero calibrate <experiment-dir> [--device <n>] [--samples <n>] [--quick] [--skip-live] [--live-seconds <s>] [--terminal]
yet-zero overlay <experiment-dir> [--seconds <s>]
```

## Troubleshooting

**`yet-zero: command not found`**
`~/.cargo/bin` is not on `PATH`. Add it or restart the shell.

**`package requires rustc 1.95`**
Update Rust with `rustup update stable`.

**`link.exe not found` or a linker error**
The platform linker or SDK from step 2 is missing. Rust ships the compiler but
uses the system linker: MSVC Build Tools on Windows, the Xcode Command Line
Tools on macOS and `gcc` on Linux.

**No camera found or permission denied**
Run `yet-zero devices`. On Linux, add your user to the `video` group and log in
again. On macOS, grant camera access. On Windows, enable camera access for
desktop apps.

**The stimulus opens on the wrong monitor**
Check `display.presenter_monitor` (zero-based) and use the console's presenter
selector. If the console and presenter target the same monitor, the console
warns.

**"waiting for fullscreen"**
The window manager did not grant fullscreen. Check the monitor index; an
out-of-range index is clamped and logged.

**Audio fails to initialize**
Install the ALSA development package at build time (`libasound2-dev` on
Debian/Ubuntu). Set `audio.enabled = false` if the experiment does not need
sound; a named `audio.device` must match a `cpal` output.

**The stimulus looks dimmed on KDE**
KWin's *Dim Inactive* effect multiplies unfocused windows. Disable it for
luminance-sensitive sessions, or click the stimulus window before starting.

**The overlay does not appear**
Wayland layer-shell is supported on KDE/wlroots/Sway and naked compositors.
GNOME Wayland is explicitly unsupported. On X11 the egui overlay is
best-effort; `overlay = "off"` disables it.

**The binary name conflicts with the Python package**
Both the Python prototype and this crate install a `yet-zero` executable. Use
`which -a yet-zero` to see which one your shell resolves.

## Known limitations

- Still early-stage; macOS and Windows camera runtime and the full overlay
  matrix are not yet validated.
- A human calibration RMSE and the 2–3 person pilot are pending.
- The headless runner does not open the camera, so it writes the CSV header and
  no gaze rows.
- Sequence playback decodes PNG frames just-in-time on the controller thread;
  skipped frames are counted and reported.
- Audio output latency is device-dependent; the default `output_latency_ms` is
  0 until measured per device.
- The console monitor is recorded but the window manager places the console;
  the presenter monitor is the enforced one.
- `--terminal` calibration prints targets but does not draw them; the graphical
  console/calibration window is the supported path.
- The console applies no automatic calibration-quality threshold: a poor fit is
  visible, not rejected.

## Related crates

- [`yet-egui`](https://crates.io/crates/yet-egui) — presenter, console and
  overlay components.
- [`yet-stim`](https://crates.io/crates/yet-stim) — experiment engine, media
  and protocols.
- [`yet-core`](https://crates.io/crates/yet-core) — portable `no_std` gaze
  primitives.

## License

GPL-3.0-only. Portions of the logic are derived from the Apache-2.0 Python
`yet_zero` prototype. See `LICENSE`, `NOTICE` and `THIRD_PARTY_LICENSES.md`.

## Links

- Repository: <https://github.com/schmettow/yet-rs>
- Full documentation: <https://github.com/schmettow/yet-rs/blob/main/README.md>
- Issues: <https://github.com/schmettow/yet-rs/issues>
