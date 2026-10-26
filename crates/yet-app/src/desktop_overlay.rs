// SPDX-License-Identifier: GPL-3.0-only

//! Overlay backend selection.
//!
//! `auto` never prevents a session from starting: on X11 it falls back to the
//! best-effort egui overlay with a warning, and on Wayland it returns `Off` plus
//! a notice if layer-shell is unavailable. An explicit mode that cannot be
//! satisfied is an error, so a configuration mistake is visible.

use std::env;
use std::path::Path;
use std::time::{Duration, Instant};

use yet_core::{gaze_scene, AppConfig, GazeMarkerStyle, GazePoint, OverlayMode};
use yet_present::{OverlayBackend, OverlayError, ScreenGeometry};
use yet_present_egui::EguiOverlay;

use crate::driver::RunError;

/// A selected overlay backend.
pub enum OverlayHost {
    /// eframe viewport driven by the console loop.
    Egui(EguiOverlay),
    /// A backend with its own thread, owned by the controller.
    ///
    /// Only the layer-shell backend constructs this today, and only on Linux;
    /// the variant stays in the enum so the app code is platform-independent.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Thread(Box<dyn OverlayBackend + Send>),
    /// No overlay.
    Off,
}

impl OverlayHost {
    /// Whether a backend was selected.
    pub const fn is_available(&self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Short label for logs.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Egui(_) => "egui",
            Self::Thread(_) => "layer-shell",
            Self::Off => "off",
        }
    }
}

/// Result of backend selection.
pub struct OverlaySelection {
    /// Selected backend.
    pub host: OverlayHost,
    /// Monitor the overlay is bound to.
    pub monitor: usize,
    /// Non-fatal explanation for an automatic fallback.
    pub notice: Option<String>,
}

/// Session information used to choose a backend.
#[derive(Clone, Debug, Default)]
struct SessionInfo {
    session_type: Option<String>,
    wayland_display: bool,
    x11_display: bool,
}

impl SessionInfo {
    fn detect() -> Self {
        Self {
            session_type: env::var("XDG_SESSION_TYPE").ok(),
            wayland_display: env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty()),
            x11_display: env::var_os("DISPLAY").is_some_and(|value| !value.is_empty()),
        }
    }

    /// An explicit X11 session, or an untyped session with an X11 display and
    /// no Wayland socket.
    fn is_x11(&self) -> bool {
        self.session_type.as_deref() == Some("x11")
            || (self.session_type.is_none() && self.x11_display && !self.wayland_display)
    }

    fn is_wayland(&self) -> bool {
        cfg!(target_os = "linux")
            && (self.wayland_display || self.session_type.as_deref() == Some("wayland"))
    }

    fn x11_warning(&self) -> String {
        "X11 session detected; using the best-effort egui overlay. X11 is deprecated: \
         transparency, z-order and click-through may not work."
            .to_owned()
    }
}

/// Select the overlay backend for the configuration.
pub fn open(config: &AppConfig) -> Result<OverlaySelection, OverlayError> {
    open_with(config, &SessionInfo::detect())
}

fn open_with(config: &AppConfig, session: &SessionInfo) -> Result<OverlaySelection, OverlayError> {
    let presenter_monitor = config.display.presenter_monitor;
    let monitor = config.overlay.monitor.unwrap_or(presenter_monitor);
    let mode = config.presenter.overlay;
    let automatic = mode == OverlayMode::Auto;

    if mode == OverlayMode::Off {
        return Ok(OverlaySelection {
            host: OverlayHost::Off,
            monitor,
            notice: None,
        });
    }

    if monitor != presenter_monitor {
        let message = format!(
            "overlay.monitor {monitor} must equal display.presenter_monitor {presenter_monitor} \
             in v1; the overlay is disabled"
        );
        return if automatic {
            Ok(OverlaySelection {
                host: OverlayHost::Off,
                monitor: presenter_monitor,
                notice: Some(message),
            })
        } else {
            Err(OverlayError::Unavailable(message))
        };
    }

    let x11 = session.is_x11();
    let wayland = session.is_wayland();

    match mode {
        OverlayMode::Off => unreachable!("handled above"),
        OverlayMode::Xwayland => Err(xwayland_unsupported()),
        OverlayMode::Egui => {
            if wayland && !x11 {
                let message = "the egui overlay targets Windows/macOS; use \
                               presenter.overlay = \"layer-shell\" on Wayland"
                    .to_owned();
                if automatic {
                    Ok(off_with_notice(monitor, message))
                } else {
                    Err(OverlayError::Unavailable(message))
                }
            } else {
                Ok(OverlaySelection {
                    host: OverlayHost::Egui(EguiOverlay::new(monitor)),
                    monitor,
                    notice: if x11 {
                        Some(session.x11_warning())
                    } else if !cfg!(any(target_os = "windows", target_os = "macos")) {
                        Some("egui overlay on X11 is best effort; X11 is deprecated".to_owned())
                    } else {
                        None
                    },
                })
            }
        }
        OverlayMode::LayerShell | OverlayMode::Auto => {
            #[cfg(target_os = "linux")]
            {
                if x11 {
                    if mode == OverlayMode::LayerShell {
                        return Err(OverlayError::Unavailable(
                            "layer-shell requires a Wayland session, but XDG_SESSION_TYPE=x11; \
                             use presenter.overlay = \"egui\" for the best-effort X11 path"
                                .to_owned(),
                        ));
                    }
                    return Ok(OverlaySelection {
                        host: OverlayHost::Egui(EguiOverlay::new(monitor)),
                        monitor,
                        notice: Some(session.x11_warning()),
                    });
                }
                if !wayland {
                    if mode == OverlayMode::LayerShell {
                        return Err(OverlayError::Unavailable(
                            "layer-shell requires a Wayland session (WAYLAND_DISPLAY or \
                             XDG_SESSION_TYPE=wayland)"
                                .to_owned(),
                        ));
                    }
                    return Ok(off_with_notice(
                        monitor,
                        "no Wayland session detected; set presenter.overlay = \"egui\" to try \
                         the best-effort X11 path"
                            .to_owned(),
                    ));
                }
                match yet_overlay::LayerShellOverlay::spawn(monitor) {
                    Ok(overlay) => Ok(OverlaySelection {
                        host: OverlayHost::Thread(Box::new(overlay)),
                        monitor,
                        notice: None,
                    }),
                    Err(error) if automatic => Ok(off_with_notice(monitor, error.to_string())),
                    Err(error) => Err(error),
                }
            }
            #[cfg(not(target_os = "linux"))]
            {
                if mode == OverlayMode::LayerShell {
                    Err(OverlayError::Unavailable(
                        "layer-shell is only available on Linux Wayland".to_owned(),
                    ))
                } else {
                    Ok(OverlaySelection {
                        host: OverlayHost::Egui(EguiOverlay::new(monitor)),
                        monitor,
                        notice: None,
                    })
                }
            }
        }
    }
}

/// Options for the standalone overlay test command.
#[derive(Clone, Copy, Debug)]
pub struct OverlayOptions<'a> {
    /// Experiment directory with `config.toml`.
    pub experiment_dir: &'a Path,
    /// How long to animate the marker.
    pub seconds: f32,
}

impl<'a> OverlayOptions<'a> {
    /// Create options for an experiment directory.
    pub fn new(experiment_dir: &'a Path) -> Self {
        Self {
            experiment_dir,
            seconds: 10.0,
        }
    }

    /// Set the animation duration.
    pub fn seconds(mut self, seconds: f32) -> Self {
        self.seconds = seconds;
        self
    }
}

/// Show a moving gaze marker over the desktop for manual validation.
pub fn run_overlay(options: OverlayOptions<'_>) -> Result<(), RunError> {
    let loaded = yet_core::load_experiment(options.experiment_dir)?;
    let selection = open(&loaded.config)?;
    if let Some(notice) = &selection.notice {
        eprintln!("overlay warning: {notice}");
    }

    let geometry = ScreenGeometry::new(
        selection.monitor,
        None,
        (loaded.config.display.width, loaded.config.display.height),
        1.0,
    );
    let duration = Duration::from_secs_f32(if options.seconds.is_finite() {
        options.seconds.clamp(0.1, 3600.0)
    } else {
        10.0
    });
    let style = GazeMarkerStyle::default();
    println!(
        "overlay: {} on monitor {} for {:.1} s",
        selection.host.label(),
        selection.monitor,
        duration.as_secs_f32()
    );

    match selection.host {
        OverlayHost::Thread(mut overlay) => {
            overlay.show(selection.monitor)?;
            let start = Instant::now();
            while start.elapsed() < duration {
                let point = marker_at(geometry, start.elapsed());
                let scene = gaze_scene(geometry, geometry, Some(point), &style);
                overlay.present(&scene)?;
                std::thread::sleep(Duration::from_millis(33));
            }
            overlay.set_visible(false);
            Ok(())
        }
        OverlayHost::Egui(overlay) => {
            let app = OverlayTestApp {
                overlay,
                geometry,
                style,
                monitor: selection.monitor,
                start: Instant::now(),
                duration,
            };
            let native = eframe::NativeOptions {
                viewport: eframe::egui::ViewportBuilder::default()
                    .with_title("yet-rs overlay test")
                    .with_inner_size([420.0, 120.0]),
                renderer: eframe::Renderer::Wgpu,
                centered: true,
                ..Default::default()
            };
            eframe::run_native(
                "yet-rs overlay test",
                native,
                Box::new(move |_cc| Ok(Box::new(app))),
            )
            .map_err(|error| RunError::Window(error.to_string()))
        }
        OverlayHost::Off => Err(RunError::Overlay(OverlayError::Unavailable(
            selection
                .notice
                .unwrap_or_else(|| "no overlay backend is available".to_owned()),
        ))),
    }
}

struct OverlayTestApp {
    overlay: EguiOverlay,
    geometry: ScreenGeometry,
    style: GazeMarkerStyle,
    monitor: usize,
    start: Instant,
    duration: Duration,
}

impl eframe::App for OverlayTestApp {
    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        let elapsed = self.start.elapsed();
        let point = marker_at(self.geometry, elapsed);
        self.overlay.set_scene(gaze_scene(
            self.geometry,
            self.geometry,
            Some(point),
            &self.style,
        ));
        let _ = self.overlay.show(self.monitor);
        self.overlay.show_viewport(ui.ctx());
        ui.label("Moving gaze marker; close this window to stop.");
        if elapsed >= self.duration {
            ui.ctx()
                .send_viewport_cmd(eframe::egui::ViewportCommand::Close);
        }
        ui.ctx().request_repaint_after(Duration::from_millis(33));
    }
}

fn marker_at(geometry: ScreenGeometry, elapsed: Duration) -> GazePoint {
    let width = geometry.size_px.0 as f32;
    let height = geometry.size_px.1 as f32;
    let angle = elapsed.as_secs_f32() * 1.2;
    GazePoint::new(
        width * 0.5 + width * 0.35 * angle.cos(),
        height * 0.5 + height * 0.35 * angle.sin(),
    )
}

fn off_with_notice(monitor: usize, notice: String) -> OverlaySelection {
    OverlaySelection {
        host: OverlayHost::Off,
        monitor,
        notice: Some(notice),
    }
}

fn xwayland_unsupported() -> OverlayError {
    OverlayError::Unavailable(
        "GNOME/XWayland overlay is not supported; use a layer-shell compositor \
         (KDE, Sway/wlroots, or a naked compositor) or set \
         presenter.overlay = \"off\""
            .to_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::{open_with, OverlayHost, SessionInfo};
    use yet_core::{AppConfig, OverlayMode};

    fn config(mode: OverlayMode, overlay_monitor: Option<usize>) -> AppConfig {
        let mut config = AppConfig {
            display: yet_core::DisplayConfig {
                presenter_monitor: 1,
                console_monitor: 0,
                width: 800,
                height: 600,
                fullscreen: true,
            },
            camera: Default::default(),
            presenter: Default::default(),
            overlay: Default::default(),
            audio: Default::default(),
        };
        config.presenter.overlay = mode;
        config.overlay.monitor = overlay_monitor;
        config
    }

    fn headless() -> SessionInfo {
        SessionInfo::default()
    }

    #[cfg(target_os = "linux")]
    fn x11() -> SessionInfo {
        SessionInfo {
            session_type: Some("x11".to_owned()),
            wayland_display: false,
            x11_display: true,
        }
    }

    #[test]
    fn off_selects_no_backend() {
        let selection = open_with(&config(OverlayMode::Off, None), &headless()).unwrap();

        assert!(!selection.host.is_available());
        assert_eq!(selection.host.label(), "off");
        assert!(selection.notice.is_none());
    }

    #[test]
    fn a_mismatched_overlay_monitor_is_an_error_when_explicit() {
        let error = match open_with(&config(OverlayMode::Egui, Some(0)), &headless()) {
            Err(error) => error,
            Ok(_) => panic!("expected an error"),
        };

        assert!(error.to_string().contains("overlay.monitor"));
    }

    #[test]
    fn a_mismatched_overlay_monitor_disables_auto_with_a_notice() {
        let selection = open_with(&config(OverlayMode::Auto, Some(0)), &headless()).unwrap();

        assert!(matches!(selection.host, OverlayHost::Off));
        assert!(selection.notice.unwrap().contains("overlay.monitor"));
    }

    #[test]
    fn xwayland_is_always_unsupported() {
        let error = match open_with(&config(OverlayMode::Xwayland, None), &headless()) {
            Err(error) => error,
            Ok(_) => panic!("expected an error"),
        };

        assert!(error.to_string().contains("GNOME"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn auto_on_x11_uses_the_best_effort_egui_overlay() {
        let selection = open_with(&config(OverlayMode::Auto, None), &x11()).unwrap();

        assert!(matches!(selection.host, OverlayHost::Egui(_)));
        assert!(selection.notice.unwrap().contains("best-effort"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn x11_takes_precedence_over_a_wayland_socket() {
        let mut session = x11();
        session.wayland_display = true;

        let selection = open_with(&config(OverlayMode::Auto, None), &session).unwrap();

        assert!(matches!(selection.host, OverlayHost::Egui(_)));
        assert!(selection.notice.unwrap().contains("X11"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn an_untyped_x11_display_uses_best_effort() {
        let session = SessionInfo {
            session_type: None,
            wayland_display: false,
            x11_display: true,
        };

        let selection = open_with(&config(OverlayMode::Auto, None), &session).unwrap();

        assert!(matches!(selection.host, OverlayHost::Egui(_)));
        assert!(selection.notice.unwrap().contains("X11"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn explicit_layer_shell_on_x11_is_an_error() {
        let error = match open_with(&config(OverlayMode::LayerShell, None), &x11()) {
            Err(error) => error,
            Ok(_) => panic!("expected an error"),
        };

        assert!(error.to_string().contains("XDG_SESSION_TYPE=x11"));
    }
}
