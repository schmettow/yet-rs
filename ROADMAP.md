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

## Sprint status

- **S0 Foundations** — complete.
- **S1 Risk spikes** — complete for this environment; target-platform runtime
  matrix remains.
- **S2 Experiment model and configuration** — complete: `config.toml`,
  `Experiment.csv`, `Stimuli.csv`, typed validation, resolved paths and a
  deterministic per-block plan; `yet-app plan examples/demo` prints it.
- **Next:** S3 scheduler, data sink and headless slice.

## Post-v1

Out-of-process presenter, GNOME Shell extension (only if XWayland fails),
real-time AOIs, runtime video decode, experiment authoring UI, browser version.
