// SPDX-License-Identifier: GPL-3.0-only

//! Overlay backend selection.
//!
//! `auto` never prevents a session from starting: if the platform backend is
//! unavailable it returns `Off` plus a notice. An explicit mode that cannot be
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

/// Select the overlay backend for the configuration.
pub fn open(config: &AppConfig) -> Result<OverlaySelection, OverlayError> {
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

    let wayland = cfg!(target_os = "linux")
        && env::var_os("WAYLAND_DISPLAY").is_some_and(|value| !value.is_empty());

    match mode {
        OverlayMode::Off => unreachable!("handled above"),
        OverlayMode::Xwayland => Err(xwayland_unsupported()),
        OverlayMode::Egui => {
            if wayland {
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
                    notice: (!cfg!(any(target_os = "windows", target_os = "macos"))).then(|| {
                        "egui overlay on X11 is best effort; X11 is deprecated".to_owned()
                    }),
                })
            }
        }
        OverlayMode::LayerShell | OverlayMode::Auto => {
            #[cfg(target_os = "linux")]
            {
                if !wayland {
                    if mode == OverlayMode::LayerShell {
                        return Err(OverlayError::Unavailable(
                            "layer-shell requires WAYLAND_DISPLAY; start a Wayland compositor \
                             (KDE, Sway/wlroots, or a naked compositor)"
                                .to_owned(),
                        ));
                    }
                    return Ok(off_with_notice(
                        monitor,
                        "X11 has no supported overlay backend; use a Wayland layer-shell \
                         compositor or set presenter.overlay = \"egui\" to try the \
                         best-effort X11 path"
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
        eprintln!("overlay: {notice}");
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
    use super::{open, OverlayHost};
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

    #[test]
    fn off_selects_no_backend() {
        let selection = open(&config(OverlayMode::Off, None)).unwrap();

        assert!(!selection.host.is_available());
        assert_eq!(selection.host.label(), "off");
        assert!(selection.notice.is_none());
    }

    #[test]
    fn a_mismatched_overlay_monitor_is_an_error_when_explicit() {
        let error = match open(&config(OverlayMode::Egui, Some(0))) {
            Err(error) => error,
            Ok(_) => panic!("expected an error"),
        };

        assert!(error.to_string().contains("overlay.monitor"));
    }

    #[test]
    fn a_mismatched_overlay_monitor_disables_auto_with_a_notice() {
        let selection = open(&config(OverlayMode::Auto, Some(0))).unwrap();

        assert!(matches!(selection.host, OverlayHost::Off));
        assert!(selection.notice.unwrap().contains("overlay.monitor"));
    }

    #[test]
    fn xwayland_is_always_unsupported() {
        let error = match open(&config(OverlayMode::Xwayland, None)) {
            Err(error) => error,
            Ok(_) => panic!("expected an error"),
        };

        assert!(error.to_string().contains("GNOME"));
    }
}
