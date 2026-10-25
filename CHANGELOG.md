# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
This project is pre-1.0 and follows semantic versioning for its public crates.

## [Unreleased]

### Added

- Cargo workspace with the `yet-present`, `yet-core` and `yet-app` crates.
- `yet-present`: `SessionTime`, physical-pixel geometry, `Scene`/`DrawCommand`,
  presenter, overlay, media-sequence and audio traits, and `RecordingPresenter`.
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
- `yet-core`: camera capture behind `Camera`: `linuxvideo` 0.3.5 on Linux,
  `nokhwa` 0.10.11 on macOS/Windows, a dedicated capture thread with a one-slot
  latest-frame mailbox, YUYV/GRAY/MJPEG conversion with explicit Rec. 601, format
  fallback, corrupt/dropped/error counters and normalized-ROI conversion.
- `yet-core`: `Calibration`, recording target/feature pairs, fitting and
  refitting without a session restart, and reporting sample count and Euclidean
  RMSE. `GrayFrame::crop` supports ROI uploads.
- `yet-app`: `yet-app camera <dir>` measures a real camera, prints format, fps
  and counters, and writes the raw eye ROI with `--save-roi`.
- `yet-app`: `yet-app calibrate <dir>` walks the 3×3 grid in the terminal,
  averages frames per target, supports `--quick` center recalibration and
  prints live gaze predictions.
- `docs/spikes/SPK-5.md`: Linux camera and calibration measurements.
- `yet-app`: `yet-app run --headless <dir> [--participant <id>] [--output <csv>]`
  drives a complete experiment with a synthetic 10 ms clock and writes the
  result CSV. A library target makes the runner testable without a window.
- `yet-app`: `yet-app plan <experiment-dir>` prints the resolved plan; a demo
  experiment lives in `examples/demo`.
- GitHub Actions CI for Linux, macOS, Windows and MSRV 1.88.
- README, ROADMAP and CHANGELOG.
- Sprint 1 risk-spike harnesses under `spikes/` and reports under `docs/spikes/`.

### Changed

- `config.toml` camera settings gained `width`, `height`, `fps`, `format` and a
  normalized `eye_region`; all are optional with 640×480, 30 fps, `auto` and no
  region defaults. The demo uses MJPG and a centered half-frame ROI.
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
- The Windows target compiles `yet-core` and `yet-app` with all targets; macOS
  runtime and the TCC prompt remain untested.
- Linux audio is now verified with `libasound2-dev`; `get_pos()` measured
  ~128 ms behind wall clock, so S8 must compensate output latency.

### Known Issues

- CV-4 (Python parity) is verified on committed synthetic fixtures. A
  real-camera eye-ROI run is still to be appended before the live session path;
  the format is in `docs/fixtures/CV-4.md`. OpenCV's fixed-point luma can differ
  from the Rec. 601 formula by one level on arbitrary colors.
- A valid human calibration RMSE is not recorded yet. The SPK-5 run exercised
  the flow with a static scene and piped Enter presses, so its 447 px RMSE is a
  plumbing result, not a tracking measurement. macOS and Windows camera runtime
  are also untested.
- The terminal calibration prints target coordinates but does not draw them; the
  drawn grid arrives with the presenter/console.
- The headless runner does not capture gaze yet, so it writes the CSV header
  but no data rows. The scheduler, presenter commands and sink are exercised;
  the live camera path is covered separately by `yet-app camera` and
  `yet-app calibrate`.
- The overlay runtime matrix on Windows, macOS, GNOME and KDE/Sway is still
  pending; the X11 proxy is not sufficient evidence for R11.
- The audio clock offset is device-dependent and is not compensated yet; the
  50 ms A/V target is not met uncompensated.
