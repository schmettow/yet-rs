# Roadmap

The detailed sprint plan is in [`DEVELOPMENT_PLAN.md`](DEVELOPMENT_PLAN.md).

## Milestones

- [x] **M0 Foundations green (S0)** — workspace, protocols, pure engine skeleton,
  GPL-3.0-only license and CI definition. Local checks pass; the three-OS CI
  workflow has been added but not yet exercised in this environment.
- [x] **M1 Risks retired (S1)** — four spike reports in `docs/spikes/`.
  SPK-1/3/4 were measured on this machine; SPK-2 is a conditional go and still
  needs the Windows/macOS/GNOME/Wayland runtime matrix. Linux audio now runs;
  `get_pos()` measured ~128 ms behind wall clock and needs compensation in S8.
- [x] **M2 Headless experiment (S3)** — a two-block experiment runs without a
  window and writes a yet_zero-compatible CSV. The demo completes 5 of 5
  trials; without the camera the CSV contains the header and no gaze rows.
- [ ] **M3 Eye tracking parity (S5)** — quadrant features match Python and the
  Linux camera path works on the GEMBIRD Yet camera (YUYV, 25.6 fps) with a
  graphical calibration window on the portrait presenter. A valid human
  calibration RMSE and the macOS/Windows runtime are still pending.
- [ ] **M4 End-to-end experiment (S7)** — console, fullscreen presenter,
  calibration and a pilot session. The S7 console now runs full and quick
  calibration, shows a live camera preview and writes gaze rows once fitted;
  the interactive desktop validation and the 2–3-person pilot remain.
- [x] **M5 Media complete (S8)** — PNG sequences and WAV audio play; the audio
  clock is master, drift and skipped frames are shown, and FFmpeg is a setup
  step only. A beep/flash A/V recording and the Windows/macOS runtime remain.
- [ ] **M6 Overlay parity (S10)** — the overlay core is implemented: an egui
  viewport for Windows/macOS and a Wayland layer-shell backend. Layer-shell was
  protocol-validated against a live compositor; the Windows/macOS and
  KDE/Sway/naked visual matrix is still open. GNOME is documented as unsupported.
- [ ] **M7 v0.1.0 (S11)** — release checklist, pilot data and complete docs.

## Sprint status

- **S0 Foundations** — complete.
- **S1 Risk spikes** — complete for this environment; target-platform runtime
  matrix remains.
- **S2 Experiment model and configuration** — complete: `config.toml`,
  `Experiment.csv`, `Stimuli.csv`, typed validation, resolved paths and a
  deterministic per-block plan; `yet-zero plan examples/demo` prints it.
- **S3 Scheduler and headless slice** — complete: absolute-deadline scheduling
  with late-tick accounting, calibration and placement golden scenes, the CSV
  sink, and `yet-zero run --headless`. 89 tests passed at the end of the sprint.
- **S4 CV features and model** — implemented and parity-checked: `GrayFrame`/`EyeRegion`,
  canonical quadrant means with the odd-dimension rule, Rec. 601 luma,
  `LinearGazeModel` (SVD least squares), calibration buffer and RMSE. CV-4 uses
  committed synthetic fixtures generated with NumPy, OpenCV and scikit-learn;
  114 tests passed at the end of the sprint.
- **S5 Camera and calibration** — implemented: `linuxvideo`/`nokhwa` behind
  `Camera`, capture thread and latest-frame mailbox, YUYV/GRAY/MJPEG conversion,
  calibration flow, a graphical calibration window and terminal fallback. The
  demo targets the GEMBIRD Yet camera on `/dev/video2` (YUYV, 25.6 fps) and the
  portrait presenter. Linux hardware and the full window flow were exercised;
  macOS/Windows runtime and a human calibration RMSE remain.
- **S6 Presenter and console shell** — complete for static images: `yet-egui`
  (deferred monitor-targeted viewport, physical-pixel mapping, images, text,
  shapes, cached blur, texture reuse) and `yet-egui` (controls, progress,
  monitor selection, frame statistics, bounded log, errors). `yet-zero run`
  opens both windows and shares effect handling with the headless runner.
  Measured on the two-monitor X11 workstation: 12.0 s demo, 60 Hz paint rate,
  ~7% coalesced frames, max lateness 0.4 ms, clean shutdown. Readiness waits
  for fullscreen to settle and scenes recompose on resize. Sequences, audio,
  camera integration, console auto-placement and the KWin dim-inactive caveat
  are documented and deferred. 174 tests passed at the end of the sprint.
- **S7 Calibration UX and live preview** — implemented, pilot outstanding:
  `CalibrationRun` (UI-free settle/collect/report/quick/cancel state machine),
  console calibration panel with RMSE and drift, phase-aware gating, and a
  camera preview with eye ROI, capture rate and gaze map. The controller thread
  owns the camera and maps gaze to stimulus pixels for the CSV. 194 tests pass;
  two ignored tests need real camera hardware. The new tracker hardware test was
  run against the GEMBIRD Yet camera (10 frames, preview and quadrant means
  valid). The interactive desktop validation was blocked by a locked session,
  and the UX-4 pilot needs 2–3 people; the README documents the procedure.
- **S8 Media and audio** — implemented: `yet-stim` (`PngSequence`, `WavAudio`,
  `RodioAudioPlayer`), controller-thread frame decoding with texture reuse,
  audio-master pacing, pause/resume through the engine, console drift/drop/
  underrun display, `yet-zero media` validation, `tools/prepare_media.sh` and
  `examples/media-demo`. 221 tests passed at the end of the sprint; audio and
  sequences were exercised on the Linux workstation (`media 2.10 s · audio
  drift +24.9 ms` at 2.1 s). Windows/macOS runtime, a beep/flash A/V
  measurement and background prefetch remain.
- **S8.5 Seam hardening** — complete: `yet-core` (`no_std`, zero dependencies,
  `FeatureLayout`, `FeatureVector`, `ModelParameters`, `LinearPredictor`,
  `GazeInput`, `PoseSample`), predictor/trainer split with a `GazeModel`
  blanket alias, `QuadExtractor`, `CameraRole`/`CameraFrame`/`FrameSet`/
  `CameraRig`, and `docs/spikes/SPK-6.md` (ESP32-C design note, estimates not
  measured). A CI job checks both Cortex-M targets. 230 tests pass; v1 output,
  timing, CSV schema and the model equation are unchanged.
- **S9 Overlay core** — implemented: the new `yet-stim` crate with a
  layer-shell thread (`smithay-client-toolkit` + calloop + `wl_shm`), the
  `EguiOverlay` transparent viewport, `gaze_scene` composition, backend
  selection (`auto`/`off`/`egui`/`layer-shell`, explicit `xwayland` error; X11
  auto-selects best-effort egui with a warning), the console toggle, and
  `yet-zero overlay` for manual checks. 254 tests pass. X11 did not composite
  correctly on the local KWin session; GNOME is unsupported; the Windows/macOS
  and real KDE/Sway/naked visual matrix remains.
- **S9.5 Workspace reorganized (2026-10-26)** — the eight-crate layout was
  consolidated into `yet-core` (no_std gaze essentials, feature `yet-zero` for
  Quadbright training), `yet-stim` (engine, media and presentation protocols),
  `yet-egui` (presenter, console, egui overlay) and the `yet-zero` binary. The
  dependency direction is `yet-zero -> yet-egui -> yet-stim -> yet-core`; the
  three libraries carry publishable path+version dependencies. 254 tests pass;
  behavior, timing, CSV schema and the model equation are unchanged.
- **Next:** S10 naked-compositor profile and platform parity (Sway config,
  Windows/macOS matrix, GNOME limitation docs).

## Post-v1

- **E1:** a `no_std`/fixed-point Quadbright glance predictor with a versioned
  model file for a microcontroller.
- **E2:** Octabright (two eye cameras) and/or a world camera with role-tagged,
  timestamped frame sets.
- **E3:** motion/head/body sensors and a true gaze model; glance stays the
  fallback when pose is absent.
- Out-of-process presenter, GNOME Shell extension (only if XWayland fails),
  real-time AOIs, runtime video decode, experiment authoring UI, browser version.
