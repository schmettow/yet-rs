# Overlay backends: decision record (OVL-7)

**Status:** accepted, 2026-10-26
**Scope:** the click-through gaze overlay, not the stimulus presenter.

## Context

Yet draws a gaze marker over the desktop so a researcher can see the estimate
during a session and so future free-viewing tasks can annotate real applications.
The overlay must be visible above ordinary windows, must not receive pointer or
keyboard input, and must not steal focus from the application under study.

Windows and macOS expose the needed window styles directly. Wayland does not
have a general "always-on-top, click-through" window protocol. The practical
options are `zwlr_layer_shell_v1` (KDE, wlroots/Sway, many nested and naked
compositors) or a fullscreen transparent application window (which is not
reliably click-through and takes focus). GNOME's Mutter implements neither
layer-shell nor an equivalent public protocol.

Native X11 is deprecated in this project. It remains a best-effort path only
because some lab machines still start an X11 session.

## Decision

| Platform | Backend | Contract |
|---|---|---|
| Windows, macOS | `egui` viewport in `yet-egui::EguiOverlay` | Transparent, borderless, always-on-top, `with_active(false)`, `with_mouse_passthrough(true)`, no taskbar entry |
| Linux Wayland with `zwlr_layer_shell_v1` | `yet-stim::LayerShellOverlay` | `wlr-layer-shell` overlay layer on one output, keyboard interactivity `None`, empty input region, `wl_shm` software raster, no eframe loop |
| Linux X11 | `egui` viewport, best effort | Only when `auto` or `egui` is selected; a warning is printed. Compositing and input passthrough may fail. X11 is deprecated |
| GNOME Wayland | none | `auto` disables the overlay with a notice; explicit `layer-shell` fails; `xwayland` returns a clear error |

`auto` never prevents a session from starting. An explicit backend that cannot
be satisfied is a startup error, so a configuration mistake is visible.

## Alternatives considered

1. **GNOME Shell extension.** Rejected. It ties Yet to a versioned, privileged
   desktop extension, needs installation per user and breaks on release. The
   project supports layer-shell compositors instead.
2. **XWayland override-redirect window.** Rejected. It cannot guarantee stacking
   or input behavior on a Wayland session, starts an additional X server, and
   would make the overlay depend on X11 after all.
3. **Fullscreen transparent eframe window on Wayland.** Rejected as the Wayland
   path. It is not click-through by contract, and two fullscreen surfaces (the
   stimulus and the overlay) would compete for focus.
4. **A separate compositor plugin or GPU overlay.** Rejected for v1. It would
   multiply platform-specific code for a marker that needs only a small scene.

## Consequences

- GNOME-on-Wayland users run without an overlay or switch to KDE, Sway/wlroots,
  or a naked compositor. This is documented in the README and in the
  [naked-compositor guide](naked-compositor.md).
- Layer-shell outputs are selected in the compositor's order, which is not the
  same as eframe's monitor index. v1 binds the overlay to the presenter monitor;
  with several outputs, validate the mapping with `yet-zero overlay`.
- The overlay never receives input: layer-shell uses an empty input region and
  keyboard interactivity `None`; egui uses `with_mouse_passthrough` and
  `with_active(false)`. A platform that ignores those hints is a platform bug to
  record in the [matrix](platform-matrix.md), not a reason to add a fallback.
- Coalesced overlay frames are counted (`requested`, `painted`, `superseded`,
  `errors`) and printed by `yet-zero overlay`; a long soak must not accumulate an
  unbounded backlog.

## Troubleshooting

| Symptom | Check | Action |
|---|---|---|
| `layer-shell requires a Wayland session` | `echo "$XDG_SESSION_TYPE"`, `echo "$WAYLAND_DISPLAY"` | Start a Wayland session; set `overlay = "off"` on X11 |
| `zwlr_layer_shell_v1 is unavailable` | `wayland-info \| grep zwlr_layer_shell_v1` | GNOME is unsupported; use KDE/Sway/wlroots or turn the overlay off |
| Overlay on the wrong output | `swaymsg -t get_outputs` / compositor output order | v1 requires `overlay.monitor == display.presenter_monitor`; single-output is unambiguous |
| Marker not visible | console **Gaze overlay** checkbox; session state; presenter fullscreen | v1 shows the marker only while a session runs and the presenter is fullscreen |
| Clicks do not reach the window below | session type, GPU/compositor | Layer-shell should pass through; X11 is best effort. Record the platform in the matrix |
| A window loses focus when the overlay appears | Window manager behavior | Layer-shell has keyboard interactivity `None` and egui uses `with_active(false)`; report a reproducible case |
| Backlog or stutter | `yet-zero overlay --seconds 3600` summary | Compare `requested`, `painted`, `superseded`; a growing `outstanding` means the backend cannot keep up |

The XWayland parser value remains accepted only to return this error:

> GNOME/XWayland overlay is not supported; use a layer-shell compositor
> (KDE, Sway/wlroots, or a naked compositor) or set `presenter.overlay = "off"`.
