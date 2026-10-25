# Roadmap

The detailed sprint plan is in [`DEVELOPMENT_PLAN.md`](DEVELOPMENT_PLAN.md).

## Milestones

- [x] **M0 Foundations green (S0)** — workspace, protocols, pure engine skeleton,
  GPL-3.0-only license and CI definition. Local checks pass; the three-OS CI
  workflow has been added but not yet exercised in this environment.
- [ ] **M1 Risks retired (S1)** — eframe presentation, overlay matrix, camera
  build/parity and A/V spikes.
- [ ] **M2 Headless experiment (S3)** — a two-block experiment writes a
  yet_zero-compatible CSV without a window.
- [ ] **M3 Eye tracking parity (S5)** — quadrant features match Python and
  calibration works on the real camera.
- [ ] **M4 End-to-end experiment (S7)** — console, fullscreen presenter,
  calibration and a pilot session.
- [ ] **M5 Media complete (S8)** — PNG sequences and WAV audio stay in sync;
  drift and underruns are logged.
- [ ] **M6 Overlay parity (S10)** — Windows, macOS, layer-shell Wayland and
  GNOME/XWayland are tested or explicitly documented.
- [ ] **M7 v0.1.0 (S11)** — release checklist, pilot data and complete docs.

## Post-v1

Out-of-process presenter, GNOME Shell extension (only if XWayland fails),
real-time AOIs, runtime video decode, experiment authoring UI, browser version.
