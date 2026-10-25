# SPK-2 — desktop overlay matrix

**Status:** Conditional go. The X11 proxy confirms the click-through and monitor-targeting mechanics. Windows/macOS/GNOME/Wayland runtime behavior is not tested.

**Question:** Can the planned overlay backends produce a transparent, topmost, click-through overlay on a chosen monitor without stealing focus, on Windows, macOS and Linux Wayland?

**Harness:** three disposable crates:

- `spikes/spk2-egui-overlay` — eframe viewport with `transparent`, `always_on_top`, `mouse_passthrough`, `taskbar(false)`, `active(false)`, `monitor(1)`, fullscreen.
- `spikes/spk2-xwayland` — `x11rb` override-redirect window with an empty input shape; the XWayland/client mechanics without a compositor.
- `spikes/spk2-layer-shell` — Smithay Client Toolkit 0.21.1 probe for `zwlr_layer_shell_v1`.

## Results by target

| Target | Method | Result |
|---|---|---|
| Windows | `cargo check --target x86_64-pc-windows-msvc -p spk2-egui-overlay` | **Compile pass.** Runtime not tested. |
| macOS | `cargo check --target x86_64-apple-darwin -p spk2-egui-overlay` | **Compile pass.** Runtime not tested. |
| Linux Wayland, layer-shell | `cargo check -p spk2-layer-shell` | **Compile pass** with SCTK 0.21.1. Runtime: `Error: NoCompositor` because no `WAYLAND_DISPLAY`/compositor exists here. |
| Linux X11, eframe proxy | Actual run on `DISPLAY=:1` | Window at `+2560+0`, `1440×2560`, depth 32, ARGB visual, fullscreen. Focused (`_NET_WM_STATE_FOCUSED`). No `_NET_WM_STATE_ABOVE`. |
| Linux X11, x11rb proxy | Actual run on `DISPLAY=:1` | `4000×2560` root-sized window, `override_redirect=yes`, empty input region (`input_rectangles=0`). |
| GNOME Wayland | — | **Not tested.** No compositor; XWayland requires one. |

## Findings

### 1. Whole-monitor geometry must be explicit

The raw X11 root window is the whole virtual desktop: `4000×2560` across two monitors. A fullscreen X11 client that uses the root size covers both monitors, violating R3. The production `XwaylandOverlay` must use RandR/output geometry to select the target monitor rectangle. The layer-shell backend naturally creates one surface per output, and the eframe overlay correctly targeted `+2560+0` on monitor 1 in the proxy run.

### 2. Focus is the unresolved overlay risk

The eframe proxy got a 32-bit visual and the correct monitor, but the window manager marked it `_NET_WM_STATE_FOCUSED`. `with_active(false)` did not prevent this on X11. Click-through and “no focus steal” are separate properties; the target-platform matrix must test both. Do not claim R11 from the X11 proxy alone.

### 3. Click-through works at the X11 protocol level

`x11rb` set an empty input shape and read back `input_rectangles=0`. That is the correct X11 mechanism. Interactive click-through was not tested because the environment has no target application to click. The eframe path relies on winit's platform implementation; `xwininfo -shape` does not report the input region, so it is not conclusive.

### 4. Transparency differs by backend

The eframe proxy selected a 32-bit visual (`Depth: 32`, visual `0x7c`), so an ARGB surface is available on this X11 setup. The `x11rb` spike used a 24-bit window, which is opaque. The production XWayland overlay needs a 32-bit visual or a shape mask; GNOME/XWayland behavior remains unverified.

### 5. Layer-shell is compile-checked, not behavior-checked

The SCTK 0.21.1 probe compiled and correctly reported the absence of a compositor. `zwlr_layer_shell_v1`, an output, a layer surface and an empty input region still need a runtime test on KDE, wlroots or Sway.

## Decision

**Conditional go.** Continue with the three-backend design from the UI analysis:

- `EguiOverlay` for Windows/macOS;
- `LayerShellOverlay` for KDE/wlroots/Sway;
- `XwaylandOverlay` for GNOME, in an in-process thread;
- native X11 stays deprecated.

Do **not** mark R11 met until the target matrix passes. If GNOME XWayland fails on focus, z-order or multi-monitor geometry, choose a GNOME Shell extension or a documented limitation before S9.

## Follow-ups

- Run the overlay matrix on real Windows/macOS/GNOME/KDE/Sway machines; record OS/compositor versions.
- Add output geometry (RandR) to `XwaylandOverlay`; never use the root-window size as a monitor size.
- Test click-through with a real target window and check focus with `_NET_WM_STATE`/platform APIs.
- Test 32-bit visuals and transparency on GNOME/XWayland; if unavailable, use a shape mask or accept an opaque overlay.
- Recheck `with_always_on_top` vs fullscreen z-order on each platform.
