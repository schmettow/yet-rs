# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project is pre-1.0 and follows semantic versioning for its public crates.

## [Unreleased]

### Changed

- **Workspace reorganized from eight crates into three publishable libraries
  and one binary**, with a strict dependency direction
  `yet-zero -> yet-egui -> yet-stim -> yet-core`.
- `yet-core` is now `#![no_std]` with `alloc`. It keeps only the abstract gaze
  essentials (time/clock, camera roles and `FrameSet`, grayscale frames and
  regions, Quadbright features, predictor and parameter block). The reference
  trainer and calibration buffer moved behind the non-default `yet-zero`
  feature; `libm` supplies the RMSE square root without `std`.
- `yet-stim` is the new UI-free home for the engine, experiment/config/plan
  model, CSV sink, calibration flow, media/audio/playback, presenter and
  overlay protocols, the recording presenter and the Wayland layer-shell
  overlay.
- `yet-egui` is the new home for the eframe presenter, the experimenter console
  and `EguiOverlay`. It depends on `yet-stim` and never on the binary crate.
- `yet-zero` replaces the `yet-app` binary and owns camera capture, the session
  driver, the headless runner, the CLI and backend selection. It keeps a
  library target for tests only and is `publish = false`.
- `yet-egui`: the deferred overlay viewport now requests its own repaint at
  30 Hz. A 30 s X11 smoke raised the measured paint rate from 4.2 to 27.4
  painted scenes/s with bounded coalescing (`requested=868 painted=823
  superseded=44 outstanding=1 errors=0`).
- `PreferredFormat` moved to `yet-stim::config`; `validate_eye_region` moved to
  `yet-core`; `DEFAULT_EYE_REGION`, `Camera`, `CameraRig` and the platform
  adapters moved to `yet-zero::camera`.

### Added

- Python `yet_zero` workspace compatibility: `Config.csv` plus
  `Stimuli/Stimuli.csv` loads without conversion, using `SLIDE_TIME`,
  `SHUFFLE`/`SEED`, `STIM_FILE`, the `File` column and the Python result prefix
  `Yeta1` under `Data/`. An optional `config.toml` overrides machine settings.
- `yet-zero devices` and `CameraDevice` enumeration: Linux V4L2 capture nodes
  via `linuxvideo`, Windows/macOS via `nokhwa`. The console has a **Camera**
  dropdown with **Scan**; changing the device reopens capture and invalidates
  the calibration.
- `yet-zero present <workspace>`: a presenter-only window with no console. Keys:
  `F` follow circle, `Space` start/pause, `F7`/`F9` calibration, `F8` abort,
  `Esc` quit.
- Follow circle in the console and presenter-only mode: a gaze marker scaled to
  the surface (1/50 radius, 1/200 halo) is appended to a temporary scene for
  painting only and never reaches the trial scene or CSV.
- Native `yet-stim` workspace loading now reports its layout
  (`Workspace: ...`) and keeps results at the workspace root with `yet1_...`;
  Python workspaces write `Data/Yeta1_...`.

- `yet-stim`: `OverlayStats` (`requested`, `painted`, `superseded`, `errors`,
  `outstanding`) and a default `OverlayBackend::stats()` accessor.
- `yet-egui`/`yet-stim`: the egui and layer-shell overlays count accepted,
  painted and coalesced scenes; the layer-shell thread also counts render
  errors.
- `yet-zero`: `overlay ... --seconds <s>` prints a soak summary with painted
  rate and outstanding frames; pacing uses absolute deadlines instead of
  sleeping after each presentation.
- `tools/run-naked.sh`: one-command two-output Sway session with rendered
  config, `--render-only`, output-name and participant overrides. The profile
  disables XWayland and keeps the console/presenter app-ID rules.
- `docs/overlay-decision.md`: GNOME/XWayland decision record, alternatives and
  troubleshooting table.
- `docs/platform-matrix.md`: build/test coverage and the overlay/HiDPI/
  multi-monitor soak recording sheet.
- README: overlay troubleshooting table, soak summary format and links to the
  decision record and matrix.

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
- `yet-gaze`: a `#![no_std]`, allocation-free crate with zero dependencies.
  `FeatureLayout` + `QUAD_LAYOUT`/`OCTA_LAYOUT`, fixed-capacity `FeatureVector`,
  `GazeInput`, `PoseSample`/`HeadPose`/`BodyPose`, `GazePoint`, versioned
  `ModelParameters` (`MODEL_FORMAT_VERSION = 1`) and `LinearPredictor`. The crate
  contains no `std`, `nalgebra`, `image` or UI types; six tests cover the layout,
  vector, prediction and rejection paths. `LinearPredictor` rejects a parameter
  block whose declared feature count exceeds `MAX_FEATURES` instead of indexing
  out of bounds.
- `yet-core`: `FeatureExtractor` and `QuadExtractor { role, region }`;
  `QuadBrightness::to_array`/`to_feature_vector` and
  `From<QuadBrightness> for FeatureVector`.
- `yet-core`: `GazeTrainer` (host fit, `nalgebra`/`f64`) and `GazePredictor`
  (portable) are separate; `GazeModel` is a blanket alias. `LinearGazeModel`
  implements both. Fitting derives its dimension from the samples, and
  `minimum_samples(feature_count)` keeps 4 for Quadbright while requiring
  `feature_count + 1` above four. `LinearParameters::to_model_parameters`
  produces the deployable `f32` block.
- `yet-core`: `CameraRole` (`EyeLeft`/`EyeRight`/`World`), `CameraFrame`,
  `FrameSet` and `CameraRig` as the additive multi-camera seam. v1 still wires
  one `EyeLeft` camera.
- `docs/spikes/SPK-6.md`: ESP32-C glance-predictor design note (2×2 Quadbright
  compression, parameter block, estimated flash/RAM/latency, risks). Estimates
  are explicitly not measured.
- A CI `no_std` guard builds `yet-gaze` for `thumbv7em-none-eabi` (Cortex-M4) and
  `thumbv6m-none-eabi` (Cortex-M0+).
- `yet-core`: `gaze_scene`/`GazeMarkerStyle`, UI-free composition of a
  transparent overlay frame with a white-haloed gaze dot. Points outside the
  presenter surface are dropped, and presenter/overlay geometries are mapped
  proportionally.
- `yet-overlay`: a new crate with a software `Argb8888` rasterizer for clears,
  circles and lines, and `LayerShellOverlay`, a dedicated-thread Wayland
  layer-shell backend (`smithay-client-toolkit` + calloop + `wl_shm`). It uses a
  fullscreen surface, an empty input region for click-through, output selection,
  buffer reuse and explicit errors for unsupported commands.
- `yet-present-egui`: `EguiOverlay`, a deferred transparent viewport with
  topmost, borderless, no-taskbar, no-activate and mouse-passthrough settings,
  plus a shared primitive painter for overlay scenes.
- `yet-present`: `OverlayBackend::present(&Scene)` and
  `OverlayError::{Unsupported, Render}`.
- `yet-console`: `OverlayStatus` with availability, an enable toggle, the latest
  gaze point and backend errors; the presenter panel shows the toggle and
  click-through state.
- `yet-app`: overlay backend selection (`auto`, `off`, `egui`, `layer-shell`;
  explicit `xwayland` returns the GNOME error). `auto` uses layer-shell on
  Wayland, egui on Windows/macOS, and the best-effort egui viewport on X11 with
  a warning (detected through `XDG_SESSION_TYPE`, with `DISPLAY` as a fallback).
  Controller-side layer-shell updates at gaze rate, console-side egui updates at
  30 Hz, and a `RunError::Overlay` path.
- `yet-app`: `yet-app overlay <experiment-dir> [--seconds <s>]` animates a gaze
  marker over the desktop for manual overlay validation.
- `docs/naked-compositor.md` and `tools/sway-yet.conf`: a minimal Sway profile
  with no bar, background service or notification daemon, plus console and
  presenter placement rules.
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

- The overlay strategy changed: **GNOME Wayland is explicitly unsupported**
  (decision 18). `presenter.overlay = "xwayland"` now returns a clear error;
  layer-shell on KDE/wlroots/Sway or a naked compositor is the supported path.
  `auto` disables the overlay with a notice when layer-shell is unavailable, and
  falls back to the best-effort egui overlay on an X11 session with a warning.
- `config.toml` `[overlay] monitor` must equal `display.presenter_monitor` in
  v1. An explicit mismatch is an error; `auto` disables the overlay and logs it.
  The overlay is hidden when the presenter is not fullscreen or the monitor
  changes; restart to rebind.
- The console uses a fully transparent eframe clear color so its deferred overlay
  viewport can composite; the console paints its own opaque background.
- `smithay-client-toolkit` 0.19 and `calloop` 0.13 are direct dependencies of
  `yet-overlay`; both were already in the workspace lock through winit.
- `DEVELOPMENT_PLAN.md` records S9 as implemented, updates the overlay milestones
  and risk register, and replaces the GNOME implementation plan with the
  naked-compositor/parity plan. `UI_analysis.md` records the same decision.
- `yet-core`: `CalibrationSample.features` is now a `FeatureVector` (with an
  optional `pose`) instead of a `QuadBrightness`. `CalibrationSample::new`
  still takes `QuadBrightness`, so the v1 call sites and the CSV are unchanged;
  `CalibrationObservation` is an alias for the same type.
- `yet-core`: `LinearParameters` gained `feature_count`; the least-squares fit is
  feature-count aware. `LinearGazeModel::fit`/`predict`, the RMSE and the model
  equation are unchanged for the four-feature path.
- `GazeModel` changed from a two-method trait to a blanket alias over
  `GazeTrainer + GazePredictor`. A type that used to implement `GazeModel`
  directly must implement the two traits separately; the combined name keeps
  working in bounds.
- `DEVELOPMENT_PLAN.md` now records S8.5 as implemented, decision 12 as closed
  and the `no_std` CI guard; the seam sketch matches the shipped API.
- `DEVELOPMENT_PLAN.md` now records the 2026-10-25 plan health check and the
  evolution requirements E1–E3: a portable MCU glance predictor, Octabright and
  a world camera, and motion sensors for a future gaze model. The plan adds six
  architecture seams, a 2–3 day seam-hardening sprint (S8.5, later implemented)
  before overlay work, future backlog items, risks and decisions. No model,
  sensor or camera behavior changed in v1.
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
