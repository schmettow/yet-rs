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
- [ ] **M3 Eye tracking parity (S5)** — quadrant features match Python and
  calibration works on the real camera.
- [ ] **M4 End-to-end experiment (S7)** — console, fullscreen presenter,
  calibration and a pilot session.
- [ ] **M5 Media complete (S8)** — PNG sequences and WAV audio stay in sync;
  drift and underruns are logged.
- [ ] **M6 Overlay parity (S10)** — Windows, macOS, layer-shell Wayland and
  GNOME/XWayland are tested or explicitly documented.
- [ ] **M7 v0.1.0 (S11)** — release checklist, pilot data and complete docs.

## Sprint status

- **S0 Foundations** — complete.
- **S1 Risk spikes** — complete for this environment; target-platform runtime
  matrix remains.
- **S2 Experiment model and configuration** — complete: `config.toml`,
  `Experiment.csv`, `Stimuli.csv`, typed validation, resolved paths and a
  deterministic per-block plan; `yet-app plan examples/demo` prints it.
- **S3 Scheduler and headless slice** — complete: absolute-deadline scheduling
  with late-tick accounting, calibration and placement golden scenes, the CSV
  sink, and `yet-app run --headless`. 89 tests passed at the end of the sprint.
- **S4 CV features and model** — implemented and parity-checked: `GrayFrame`/`EyeRegion`,
  canonical quadrant means with the odd-dimension rule, Rec. 601 luma,
  `LinearGazeModel` (SVD least squares), calibration buffer and RMSE. 114 tests
  pass. CV-4 uses committed synthetic fixtures generated with NumPy, OpenCV and
  scikit-learn; recorded eye-ROI frames remain to be appended before S5
  ([`docs/fixtures/CV-4.md`](docs/fixtures/CV-4.md)).
- **Next:** append recorded eye-ROI frames, then S5 camera and calibration on
  hardware.

## Post-v1

Out-of-process presenter, GNOME Shell extension (only if XWayland fails),
real-time AOIs, runtime video decode, experiment authoring UI, browser version.
