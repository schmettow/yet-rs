# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project is pre-1.0 and follows semantic versioning for its public crates.

## [Unreleased]

### Added

- Cargo workspace with the `yet-present`, `yet-core` and `yet-app` crates.
- `yet-present`: `SessionTime`, physical-pixel geometry, `Scene`/`DrawCommand`
  (including `BlurredImage`), presenter, overlay, media-sequence and audio
  traits, and `RecordingPresenter`. `Presenter::media_size` lets the driver
  compose placed scenes without decoding media itself; `PresenterError::Media`
  reports unreadable files with their path.
- `yet-present-egui`: the eframe presenter with a monitor-targeted deferred
  viewport, physical-pixel to logical-point conversion at 100% and 150%,
  image decode, texture reuse, cached box blur, text/circle/line drawing,
  coalesced-frame accounting and a measured paint rate. The viewport waits for
  the window manager to fill the monitor before reporting ready, and the scene
  recomposes if the surface is resized.
- `yet-console`: the experimenter console with start/pause/resume/abort,
  presenter-monitor selection, progress, frame statistics, a bounded log,
  actionable errors and a warning when the stimulus window is not focused.
- `yet-core`: a pure `step` engine with start/pause/resume/abort/tick effects,
  `TestClock`, and the calibration data types.
- `yet-core`: experiment model (`Experiment`/`Block`/`Trial`/`Stimulus`),
  `config.toml` loading, optional `Experiment.csv` metadata, `Stimuli.csv`
  loading, typed validation, path resolution and deterministic per-block trial
  planning.
- `yet-core`: absolute-deadline trial scheduling in `step` with `TrialStarted`
  effects, per-tick frame records, late-tick accounting, pause/resume deadline
  shifts and an incomplete record when a session is aborted mid-trial.
- `yet-core`: `CalibrationGrid` with the Python 3×3 proportions and colors,
  `StimulusPlacement` for scaled and centered stimuli, and instruction-text
  scenes, all as backend-neutral `DrawCommand`s.
- `yet-core`: `DataSink` writing the yet_zero columns
  `Exp,Part,Stim,time,x,y,x_pro,y_pro`, with a header at creation and flush per
  trial.
- `yet-core`: `GrayFrame`, `EyeRegion`, canonical `quad_brightness` with the
  odd-dimension split rule, and `rec601_luma` matching OpenCV `BGR2GRAY`.
- `yet-core`: `GazeModel` trait and `LinearGazeModel`, a centered SVD least-squares
  fit from four quadrant means to two monitor coordinates, with typed
  `ModelError`s and repeatable refits.
- `yet-core`: `CalibrationBuffer` with fit and Euclidean RMSE, for quick
  recalibration and calibration-quality reporting.
- `yet-core`: `LinearGazeModel::parameters`, exposing the fitted coefficient and
  intercept matrices for calibration diagnostics.
- `tools/cv4_fixtures.py` and committed synthetic Python-oracle fixtures:
  NumPy quadrant means, scikit-learn predictions/coefficients, and OpenCV luma
  for odd-sized patterns. `tests/cv4_parity.rs` checks them.
- `docs/fixtures/CV-4.md`: the parity format, the canonical quadrant mapping
  and privacy handling for eye crops.
- `yet-media`: a UI-free crate for pre-decoded media. `PngSequence` scans and
  sorts PNG frames, validates the first frame, and maps elapsed time to frames
  with loop and last-frame behavior. `WavAudio` decodes 8/16/32-bit integer and
  32-bit float PCM with `hound`. `RodioAudioPlayer` feeds `f32` samples to
  `rodio::buffer::SamplesBuffer`, validates default or named `cpal` devices,
  exposes `get_pos()` plus `output_latency` and a best-effort underrun signal,
  and `NullAudioPlayer` keeps headless runs silent.
- `yet-present-egui`: sequence playback on the controller thread. Frames are
  decoded outside the painting mutex, textures are reused per medium, the audio
  clock is master when a WAV is present (monotonic otherwise), pause/resume
  reaches both clocks, and each present reports `media_pts`, `audio_pts`, clock
  source, skipped frames, drift and underruns. Unchanged scenes no longer force
  a repaint.
- `yet-app`: `yet-app media <dir>` validates every sequence and WAV, printing
  frames, fps, duration, size, audio format and a duration-mismatch warning.
  The console shows media position, dropped frames, drift and underruns, and
  opens audio only when the plan actually uses a track.
- `tools/prepare_media.sh`: one FFmpeg command produces a PNG directory and a
  48 kHz stereo WAV, with explicit failures for missing FFmpeg/input, zero
  frames or audio extraction errors, and a ready-to-edit `Stimuli.csv` row.
- `examples/media-demo`: the SPK-4 3 s, 30 fps clip with a 1 kHz beep at 1 s and
  a matching WAV, for interactive A/V validation.
- `config.toml` `audio.output_latency_ms` (default `0`, maximum 1000) for
  devices with a measured output-buffer offset.
- Tests for PNG ordering/pacing/validation, WAV decoding and duration, sequence
  decoding, loop and last-frame behavior, audio-master pacing with a fake audio
  clock, pause behavior, rejected empty sequences and the no-repaint path.
- `yet-core`: pause/resume now emits `PresenterCommand::Pause`/`Resume` so the
  audio and media clocks stop with the engine.
- `yet-present`: `MediaError::Decode`, `PresenterError::Audio`, and
  `AudioPlayer::{underruns, output_latency, set_output_latency}` with neutral
  defaults.
- `yet-core` — camera capture behind `Camera`: `linuxvideo` on Linux,
  `nokhwa` 0.10.11 on macOS/Windows, a dedicated capture thread with a one-slot
  latest-frame mailbox, YUYV/GRAY/MJPEG conversion with explicit Rec. 601, format
  fallback, corrupt/dropped/error counters and normalized-ROI conversion.
- `yet-core`: `Calibration`, recording target/feature pairs, fitting and
  refitting without a session restart, and reporting sample count and Euclidean
  RMSE. `GrayFrame::crop` supports ROI uploads.
- `yet-app`: `yet-app camera <dir>` measures a real camera, prints format, fps
  and counters, and writes the raw eye ROI with `--save-roi`.
- `yet-app`: `yet-app calibrate <dir>` opens a borderless calibration window
  on the presenter monitor with the 3×3 grid, automatic frame collection, the
  RMSE report, quick center recalibration and a live gaze dot. `--terminal`
  keeps the coordinate-printing fallback, and `--device` overrides the camera
  index.
- `docs/spikes/SPK-5.md`: Linux camera, two-monitor geometry and calibration
  window measurements.
- `yet-app`: `yet-app run --headless <dir> [--participant <id>] [--output <csv>]`
  drives a complete experiment with a synthetic 10 ms clock and writes the
  result CSV. A library target makes the runner testable without a window.
- `yet-app`: `yet-app plan <experiment-dir>` prints the resolved plan; a demo
  experiment lives in `examples/demo`.
- `yet-app`: `yet-app run <experiment-dir>` opens the console plus a fullscreen
  stimulus viewport on the configured monitor. `--headless` keeps the
  windowless runner. Both paths share `yet-app/src/driver.rs`, which composes
  stimulus scenes from `StimulusPlacement`, loads media into the presenter,
  applies engine effects and writes the data sink.
- `yet-core`: `CalibrationRun`, a UI-free calibration state machine with settle,
  automatic collection (20 frames, 500–1500 ms), a 2 s report hold, quick
  center recalibration, cancellation, typed errors and restoration of the
  previous model and buffer after a failed full run. `CalibrationGrid` gained
  `surface_px`, `scene_with_text` and `quick_scene` with a blurred stimulus
  background for recalibration.
- `yet-console`: a calibration panel with full/quick/cancel controls, phase and
  target progress, frame counts, RMSE, drift against the previous quick or full
  report, actionable errors and phase-aware gating. A camera preview shows the
  eye-ROI thumbnail, capture rate, a 120×96 gaze map and the gaze in stimulus
  pixels and normalized coordinates, with a retry path for camera errors.
- `yet-app`: the session path now owns the camera. `Tracker` polls the capture
  thread without counting a frame twice, `Tracker::preview` crops and downscales
  the eye region off the UI thread, and gaze samples are mapped to stimulus
  pixels and normalized coordinates before they reach the CSV. Full and quick
  calibration run inside the console at the controller tick; `--device`
  overrides the configured camera index.
- Tests for the calibration state machine, phase-aware console gating, drift
  reporting, the preview texture path, tracker polling and thumbnail scaling,
  and gaze mapping. An ignored hardware test exercises `Tracker` against the
  real YET camera.
- `examples/demo-gui`: a four-image, image-only experiment for the console demo.
  It reuses the stimuli in `examples/demo` and keeps sequences out of the S6
  path.
- Tests for protocol media sizes and blurred images, blur math, pixel-to-point
  mapping at 100%/150%, text anchors, texture and blur caching, coalescing,
  console state and UI smoke frames, scene composition, resize recomposition
  and the new demo experiment.
- `docs/spikes/SPK-5.md` records the S6 presenter measurements: the first paint
  arrives at 800×600 before fullscreen, so readiness must wait for the monitor
  fill; `with_monitor` places the stimulus correctly on DP-3.
- GitHub Actions CI for Linux, macOS and Windows and an MSRV 1.95 check.
- README, ROADMAP and CHANGELOG.
- Sprint 1 risk-spike harnesses under `spikes/` and reports under `docs/spikes/`.

### Changed

- `DEVELOPMENT_PLAN.md` now records the 2026-10-25 plan health check and the
  evolution requirements E1–E3: a portable MCU glance predictor, Octabright and
  a world camera, and motion sensors for a future gaze model. The plan adds six
  architecture seams, a proposed 2–3 day seam-hardening sprint (S8.5) before
  overlay work, future backlog items, risks and decisions. No model, sensor or
  camera behavior changed in v1.
- The experimenter console targets a notebook's main screen: text, padding and
  controls are scaled 2×, the window opens at 1400×860 logical points clamped
  to 92% of the monitor, and wide windows use a two-column layout (controls and
  presenter on the left, camera preview and log on the right). Narrow windows
  fall back to one column, the left column scrolls independently, log lines
  wrap instead of clipping, and the camera preview is served at up to 480×320.
- `config.toml` camera settings gained `width`, `height`, `fps`, `format` and a
  normalized `eye_region`; all are optional with 640×480, 30 fps, `auto` and no
  region defaults. `display.presenter_origin` was removed again in S6: eframe
  0.37 targets a monitor with `ViewportBuilder::with_monitor`. The demo now
  targets the GEMBIRD Yet camera on `/dev/video2` in YUYV and a portrait
  1440×2560 presenter.
- `yet-core`: the tracker moved from a single `tracking.rs` into
  `tracking/{mod,frame,features,model,buffer}.rs`. Public re-exports are
  unchanged.
- `EngineState` can carry a `SessionPlan`; `step` now schedules trials by
  absolute deadline instead of accumulating `dt`. `TrialRecord` and
  `FrameRecord` carry block, trial index and stimulus identifiers.
- The plan-less `Start` path is retained for the protocol skeleton: it changes
  phase and presents an empty scene but no longer fabricates a trial record.
- Project license changed from MIT to GPL-3.0-only, following the requirements
  recorded in `UI_analysis.md`.
- `UI_analysis.md`: corrected for eframe 0.37's `App::ui` entry point, rodio
  0.23's `SamplesBuffer` API and the absence of a `hound` feature; documented the
  Linux ALSA build prerequisite.
- `CV_analysis.md`: corrected for Rec. 601 luma (not `image::to_luma8`'s Rec. 709)
  and the macOS `objc_exception` build step in `nokhwa`.
- `DEVELOPMENT_PLAN.md`: recorded Sprint 1 findings, risks and the build-prerequisite
  decision; recorded Sprint 2 as implemented.
- `nalgebra` 0.34 is now a `yet-core` dependency (MSRV 1.88 preserved); the gaze
  model uses SVD rather than normal equations because quadrant means are
  collinear.
- `image` 0.25.10, `linuxvideo` 0.3.5 (Linux) and `nokhwa` 0.10.11
  (macOS/Windows) are camera dependencies. `nokhwa` is compiled with only
  `input-avfoundation`/`input-msmf`; its Linux backend and bindgen are excluded.
- `eframe` 0.37 is now the UI dependency for `yet-present-egui`, `yet-console`
  and `yet-app`, with `wgpu`/`AutoVsync` and `desired_maximum_frame_latency = 1`.
  This raises the MSRV to Rust 1.95; the `display.presenter_origin` workaround
  from S5 is gone, and `with_monitor` targets a monitor on Windows, macOS, X11
  and Wayland. `system_font_fallback` is disabled so text uses egui's bundled
  deterministic fonts.
- `yet-app run` without `--headless` now opens the graphical console; the
  headless runner keeps the same CLI flags and output.
- `yet-app run` gained in-console calibration (F7 full grid, F9 quick center,
  Esc cancel, F10 preview). The controller thread opens the camera lazily, uses
  it for calibration and for gaze while a fit is active, and caps preview uploads
  at about 15 Hz.
- The session report and errors moved to `yet-app/src/driver.rs` so the console
  and the headless runner share them. `yet-app::headless` re-exports both.
- The controller runs on its own thread with a 16.7 ms absolute tick. The
  console reads snapshots; the presenter handle is thread-safe and decodes
  media on the controller thread, never on the UI thread.
- The Windows target compiles `yet-core` and `yet-app` with all targets; macOS
  runtime and the TCC prompt remain untested.
- Linux audio is now verified with `libasound2-dev`; `get_pos()` measured
  ~128 ms behind wall clock, so S8 must compensate output latency.

### Fixed

- The presenter monitor selector now lists the monitors reported by the
  platform, labelled from 1 with resolution and output name, instead of always
  offering indices 0–3. An out-of-range `display.presenter_monitor` is clamped
  to the last monitor with a console warning, so it no longer opens an 800×600
  windowed stimulus. Fullscreen is requested explicitly on the viewport builder,
  so re-enabling it or switching monitors no longer leaves the stimulus window
  windowed.
- The default presenter monitor is now the second monitor (zero-based index 1,
  labelled **2**), matching the two-monitor experiment workstation.

### Known Issues

- CV-4 (Python parity) is verified on committed synthetic fixtures. A
  real-camera eye-ROI run is still to be appended before the live session path;
  the format is in `docs/fixtures/CV-4.md`. OpenCV's fixed-point luma can differ
  from the Rec. 601 formula by one level on arbitrary colors.
- A valid human calibration RMSE is not recorded yet. The scripted window runs
  exercised the full state machine with no human fixations, so their RMSE values
  are plumbing results, not tracking measurements. macOS and Windows camera
  runtime are also untested.
- eframe 0.37 is required for `with_monitor`; the MSRV is therefore 1.95. The
  three-OS CI workflow still needs its first remote run.
- KWin's *Dim Inactive* effect dims an unfocused stimulus window by 25%, which
  changes its luminance. yet-rs requests focus on Start/Resume and warns in the
  console when the stimulus is unfocused, but KWin refused the focus request in
  the X11 test. Disable the effect for luminance-sensitive sessions.
- The console window is a normal window; its monitor is recorded but not
  enforced. The stimulus viewport is the monitor-targeted one.
- The S7 calibration flow compiles and its state machine is covered by tests,
  but the interactive desktop validation could not run in this environment
  because the session was locked. The pilot usability test (UX-4) needs two or
  three people and is still outstanding; the README documents how to run it.
- The console shows the human calibration RMSE but applies no automatic quality
  threshold: a poor fit is visible, not rejected. Accuracy and latency budgets
  stay deferred to the end of the project as agreed.
- Sequence playback decodes PNG frames just-in-time on the controller thread
  rather than in a prefetch worker. On the portrait 1440×2560 presenter, about
  13 of 63 frames of the 3 s demo were skipped while the UI painted (counted and
  reported); they never block the session. A background prefetch worker is a
  follow-up if drops become a problem on slower machines.
- Audio plays once per trial. A looped sequence loops its frames, but its WAV is
  not restarted; the trial duration remains authoritative. Underruns are a
  best-effort signal from an empty rodio queue and are not observable on every
  backend.
- The audio clock offset is device-dependent. SPK-4 measured −128 ms on its
  harness, but the integrated S8 run showed `get_pos()` tracking wall time
  (+8 ms drift with no offset), so the default `output_latency_ms` is 0. A
  beep/flash screen-and-sound recording is needed before claiming a tight A/V
  budget. Windows and macOS playback are compile-checked only.
- A 60 Hz tick against 3 s deadlines produces sub-millisecond lateness on every
  trial (max 0.4 ms in the debug run). The console reports it with one decimal
  rather than truncating it to "0 ms".
- The `--terminal` calibration fallback prints targets but does not draw them;
  the graphical window is the supported calibration path.
- The headless runner does not open the camera, so it still writes the CSV
  header but no data rows. The graphical console runs the camera and writes gaze
  rows once a calibration is fitted.
- The overlay runtime matrix on Windows, macOS, GNOME and KDE/Sway is still
  pending; the X11 proxy is not sufficient evidence for R11.
