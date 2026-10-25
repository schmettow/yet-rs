// SPDX-License-Identifier: GPL-3.0-only

//! The eframe-backed stimulus presenter.
//!
//! [`EguiPresenter`] is a thread-safe handle that implements the UI-free
//! [`Presenter`] trait. Decoding and scene updates happen on the controller
//! thread; painting happens in a deferred egui viewport on the UI thread. The
//! handle never blocks on the UI thread and never paints.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui;
use image::RgbaImage;
use yet_present::{
    FrameStamp, MediaId, PresentStats, Presenter, PresenterCommand, PresenterError, Scene,
    ScreenGeometry, SessionTime,
};

use crate::media::MediaStore;
use crate::painter;
use crate::view;

const PAINT_INTERVALS: usize = 120;
const READY_FALLBACK: Duration = Duration::from_millis(500);

/// Monitor and surface configuration for a presenter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresenterConfig {
    /// Zero-based monitor index for the stimulus viewport.
    pub monitor: usize,
    /// Whether the stimulus viewport is borderless fullscreen.
    pub fullscreen: bool,
    /// Nominal monitor size in physical pixels, used until the viewport paints.
    pub nominal_size_px: (u32, u32),
    /// Nominal physical pixels per logical point.
    pub nominal_scale_factor: f32,
}

impl PresenterConfig {
    /// Create a configuration from the experiment's display settings.
    pub const fn new(
        monitor: usize,
        fullscreen: bool,
        nominal_size_px: (u32, u32),
        nominal_scale_factor: f32,
    ) -> Self {
        Self {
            monitor,
            fullscreen,
            nominal_size_px,
            nominal_scale_factor,
        }
    }
}

impl Default for PresenterConfig {
    fn default() -> Self {
        Self::new(0, true, (1920, 1080), 1.0)
    }
}

/// A snapshot of presenter progress for the console.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PresenterStatus {
    /// Whether the stimulus viewport has settled at its final monitor size.
    pub ready: bool,
    /// Configured monitor index.
    pub monitor: usize,
    /// Whether the stimulus viewport is borderless fullscreen.
    pub fullscreen: bool,
    /// Actual viewport size in physical pixels.
    pub size_px: (u32, u32),
    /// Actual physical pixels per logical point.
    pub scale_factor: f32,
    /// Scenes painted by the viewport.
    pub paints: u64,
    /// Scenes handed to [`Presenter::present`].
    pub presents: u64,
    /// Scenes replaced before they were painted.
    pub coalesced: u64,
    /// Measured paint rate while scenes are arriving.
    pub paint_hz: Option<f32>,
    /// Interval between the two most recent presents.
    pub last_dt: Option<Duration>,
    /// Loaded media entries.
    pub media_count: usize,
    /// Whether the viewport currently has input focus.
    pub focused: bool,
    /// Experiment time of the most recently presented frame.
    pub last_frame_at: Option<SessionTime>,
}

/// The painting side of a presenter, shared with the controller thread.
pub struct PresenterState {
    config: PresenterConfig,
    geometry: ScreenGeometry,
    media: MediaStore,
    scene: Scene,
    frame: Option<FrameStamp>,
    dirty: bool,
    ready: bool,
    focused: bool,
    focus_requested: bool,
    presents: u64,
    paints: u64,
    coalesced: u64,
    intervals: VecDeque<Duration>,
    last_content_paint: Option<Instant>,
    size_since: Option<(Instant, (u32, u32))>,
    last_dt: Option<Duration>,
    ctx: Option<egui::Context>,
    active_viewport: egui::ViewportId,
}

impl PresenterState {
    fn new(config: PresenterConfig) -> Self {
        let geometry = ScreenGeometry::new(
            config.monitor,
            None,
            config.nominal_size_px,
            config.nominal_scale_factor.max(0.1),
        );
        Self {
            config,
            geometry,
            media: MediaStore::default(),
            scene: Scene::new(),
            frame: None,
            dirty: false,
            ready: false,
            focused: true,
            focus_requested: false,
            presents: 0,
            paints: 0,
            coalesced: 0,
            intervals: VecDeque::with_capacity(PAINT_INTERVALS),
            last_content_paint: None,
            size_since: None,
            last_dt: None,
            ctx: None,
            active_viewport: view::viewport_id(config.monitor),
        }
    }

    /// Current status snapshot.
    pub fn status(&self) -> PresenterStatus {
        PresenterStatus {
            ready: self.ready,
            monitor: self.config.monitor,
            fullscreen: self.config.fullscreen,
            size_px: self.geometry.size_px,
            scale_factor: self.geometry.scale_factor,
            paints: self.paints,
            presents: self.presents,
            coalesced: self.coalesced,
            paint_hz: self.paint_rate(),
            last_dt: self.last_dt,
            media_count: self.media.len(),
            focused: self.focused,
            last_frame_at: self.frame.as_ref().map(|frame| frame.experiment_t),
        }
    }

    /// Monitor geometry as last observed from the viewport.
    pub fn geometry(&self) -> ScreenGeometry {
        self.geometry
    }

    /// Target monitor and fullscreen mode for the viewport builder.
    pub fn target_monitor(&self) -> (usize, bool) {
        (self.config.monitor, self.config.fullscreen)
    }

    /// Natural size of loaded media.
    pub fn media_size(&self, media: MediaId) -> Option<(u32, u32)> {
        self.media.size(media)
    }

    /// Insert a decoded image.
    pub fn insert_image(&mut self, media: MediaId, image: RgbaImage) -> bool {
        self.media.insert(media, image)
    }

    /// Change the target monitor or fullscreen mode.
    pub fn set_monitor(&mut self, monitor: usize, fullscreen: bool) {
        self.config.monitor = monitor;
        self.config.fullscreen = fullscreen;
        self.geometry.monitor_index = monitor;
        self.ready = false;
        self.size_since = None;
        self.dirty = true;
    }

    /// Remember which viewport the console is currently showing.
    pub fn set_active_viewport(&mut self, viewport: egui::ViewportId) {
        self.active_viewport = viewport;
    }

    /// The viewport that currently displays the stimulus.
    pub fn active_viewport(&self) -> egui::ViewportId {
        self.active_viewport
    }

    /// Mark a medium as active, failing for an unknown identifier.
    pub fn play(&mut self, media: MediaId) -> Result<(), PresenterError> {
        if self.media.contains(media) {
            Ok(())
        } else {
            Err(PresenterError::UnknownMedia(media))
        }
    }

    /// Clear the presented scene.
    pub fn stop(&mut self) {
        self.scene = Scene::new();
        self.frame = None;
        self.dirty = true;
    }

    /// Record a scene for the next paint.
    pub fn present(&mut self, frame: &FrameStamp, scene: &Scene) -> PresentStats {
        let dropped = self.dirty;
        if dropped {
            self.coalesced += 1;
        }
        self.scene.clone_from(scene);
        self.frame = Some(frame.clone());
        self.dirty = true;
        self.presents += 1;
        self.last_dt = Some(frame.dt);

        PresentStats {
            frame: FrameStamp {
                dropped,
                ..frame.clone()
            },
            dropped_frames: u32::from(dropped),
            audio_drift_ms: None,
            audio_underruns: 0,
        }
    }

    /// Paint the current scene. Called from the deferred viewport.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let pixels_per_point = ctx.pixels_per_point().max(0.1);
        let clip_rect = ui.max_rect();
        let size_px = (
            (clip_rect.width() * pixels_per_point).round().max(1.0) as u32,
            (clip_rect.height() * pixels_per_point).round().max(1.0) as u32,
        );

        if self.geometry.size_px != size_px || self.geometry.scale_factor != pixels_per_point {
            self.geometry =
                ScreenGeometry::new(self.config.monitor, None, size_px, pixels_per_point);
        }
        self.ctx = Some(ctx.clone());
        self.focused = ctx.input(|input| input.viewport().focused).unwrap_or(true);
        if self.focus_requested {
            self.focus_requested = false;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // A freshly created viewport paints once at egui's default 800×600
        // before the window manager applies borderless fullscreen. Do not let
        // the console start a session until the viewport actually fills the
        // monitor, otherwise the first scene would be composed for the wrong
        // surface. If a compositor does not report a monitor size, fall back to
        // a size that has been stable for half a second.
        let now = Instant::now();
        let monitor_size = ctx.input(|input| input.viewport().monitor_size);
        let fills_monitor = monitor_size.is_some_and(|monitor| {
            (clip_rect.width() - monitor.x).abs() <= 2.0
                && (clip_rect.height() - monitor.y).abs() <= 2.0
        });
        if self.size_since.is_none_or(|(_, size)| size != size_px) {
            self.size_since = Some((now, size_px));
        }
        let stable = self
            .size_since
            .is_some_and(|(since, _)| now.duration_since(since) >= READY_FALLBACK);
        if !self.config.fullscreen || fills_monitor || (monitor_size.is_none() && stable) {
            self.ready = true;
        } else if !self.ready {
            ctx.request_repaint_after(Duration::from_millis(100));
        }

        let now = Instant::now();
        if self.dirty {
            if let Some(previous) = self.last_content_paint {
                let interval = now.duration_since(previous);
                if interval < Duration::from_millis(250) {
                    self.intervals.push_back(interval);
                    while self.intervals.len() > PAINT_INTERVALS {
                        self.intervals.pop_front();
                    }
                } else {
                    self.intervals.clear();
                }
            }
            self.last_content_paint = Some(now);
        }

        ui.painter()
            .rect_filled(clip_rect, 0.0, egui::Color32::from_rgb(24, 24, 24));
        painter::paint_scene(
            ui.painter(),
            clip_rect,
            pixels_per_point,
            &self.scene,
            &mut self.media,
            &ctx,
        );

        self.paints += 1;
        self.dirty = false;
    }

    fn paint_rate(&self) -> Option<f32> {
        if self.intervals.len() < 2 {
            return None;
        }
        let total: Duration = self.intervals.iter().sum();
        let seconds = total.as_secs_f64();
        (seconds > 0.0).then(|| self.intervals.len() as f32 / seconds as f32)
    }
}

/// A cloneable presenter handle backed by a deferred egui viewport.
#[derive(Clone)]
pub struct EguiPresenter {
    state: Arc<Mutex<PresenterState>>,
}

impl EguiPresenter {
    /// Create a presenter and its shared state.
    pub fn new(config: PresenterConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(PresenterState::new(config))),
        }
    }

    /// Shared state for the viewport closure.
    pub fn shared(&self) -> Arc<Mutex<PresenterState>> {
        Arc::clone(&self.state)
    }

    /// Current status snapshot.
    pub fn status(&self) -> PresenterStatus {
        self.lock().status()
    }

    fn lock(&self) -> MutexGuard<'_, PresenterState> {
        lock_state(&self.state)
    }

    /// Change the target monitor or fullscreen mode of the viewport.
    /// Change the target monitor or fullscreen mode of the viewport.
    ///
    /// The next [`crate::show_viewport`] call creates a new native window with
    /// a monitor-keyed viewport identifier; the old window is removed.
    pub fn set_monitor(&self, monitor: usize, fullscreen: bool) {
        self.lock().set_monitor(monitor, fullscreen);
    }

    /// Ask the viewport to take input focus on its next paint.
    ///
    /// Some compositors dim unfocused windows (KWin's "Dim Inactive"
    /// multiplies them by 75%), which changes stimulus luminance. Requesting
    /// focus keeps the stimulus at its intended brightness while a trial runs.
    pub fn request_focus(&self) {
        let (ctx, viewport) = {
            let mut state = self.lock();
            state.focus_requested = true;
            (state.ctx.clone(), state.active_viewport)
        };
        if let Some(ctx) = ctx {
            ctx.request_repaint_of(viewport);
        }
    }

    fn request_repaint(&self) {
        let (ctx, viewport) = {
            let state = self.lock();
            (state.ctx.clone(), state.active_viewport)
        };
        if let Some(ctx) = ctx {
            ctx.request_repaint_of(viewport);
        }
    }
}

impl Presenter for EguiPresenter {
    fn geometry(&self) -> ScreenGeometry {
        self.lock().geometry()
    }

    fn media_size(&self, media: MediaId) -> Option<(u32, u32)> {
        self.lock().media_size(media)
    }

    fn apply(&mut self, command: PresenterCommand) -> Result<(), PresenterError> {
        match command {
            PresenterCommand::LoadImage { id, path } => {
                let image = image::open(&path)
                    .map_err(|error| PresenterError::Media {
                        path,
                        message: error.to_string(),
                    })?
                    .into_rgba8();
                self.lock().insert_image(id, image);
                Ok(())
            }
            PresenterCommand::LoadSequence { .. } => Err(PresenterError::InvalidCommand(
                "frame sequences arrive in Sprint 8; use a static image".to_owned(),
            )),
            PresenterCommand::ShowMonitor {
                monitor,
                fullscreen,
            } => {
                self.lock().set_monitor(monitor, fullscreen);
                self.request_repaint();
                Ok(())
            }
            PresenterCommand::Play { id } => self.lock().play(id),
            PresenterCommand::Pause | PresenterCommand::Resume => Ok(()),
            PresenterCommand::Stop | PresenterCommand::Shutdown => {
                self.lock().stop();
                self.request_repaint();
                Ok(())
            }
            PresenterCommand::SetVolume(_) => Ok(()),
        }
    }

    fn present(&mut self, frame: &FrameStamp, scene: &Scene) -> PresentStats {
        let stats = self.lock().present(frame, scene);
        self.request_repaint();
        stats
    }
}

pub(crate) fn lock_state(state: &Arc<Mutex<PresenterState>>) -> MutexGuard<'_, PresenterState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use yet_present::{
        ClockSource, FrameStamp, MediaId, Presenter, PresenterCommand, PresenterError, Scene,
        SessionTime,
    };

    use super::{EguiPresenter, PresenterConfig};

    fn frame_stamp(at: Duration) -> FrameStamp {
        let now = Instant::now();
        FrameStamp {
            experiment_t: SessionTime::from_duration(at),
            requested_at: now,
            presented_at: now,
            dt: Duration::from_millis(16),
            media_pts: None,
            audio_pts: None,
            master: ClockSource::Monotonic,
            dropped: false,
        }
    }

    #[test]
    fn the_nominal_geometry_is_available_before_the_first_paint() {
        let presenter = EguiPresenter::new(PresenterConfig::new(1, true, (1440, 2560), 1.5));
        let geometry = presenter.geometry();

        assert_eq!(geometry.monitor_index, 1);
        assert_eq!(geometry.size_px, (1440, 2560));
        assert_eq!(geometry.scale_factor, 1.5);
        assert!(!presenter.status().ready);
    }

    #[test]
    fn an_unpainted_scene_is_reported_as_coalesced() {
        let mut presenter = EguiPresenter::new(PresenterConfig::default());
        let mut scene = Scene::new();
        scene.clear(yet_present::Color::WHITE);

        let first = presenter.present(&frame_stamp(Duration::ZERO), &scene);
        let second = presenter.present(&frame_stamp(Duration::from_millis(16)), &scene);

        assert!(!first.frame.dropped);
        assert_eq!(first.dropped_frames, 0);
        assert!(second.frame.dropped);
        assert_eq!(second.dropped_frames, 1);
        assert_eq!(presenter.status().presents, 2);
        assert_eq!(presenter.status().coalesced, 1);
        assert_eq!(presenter.status().paints, 0);
    }

    #[test]
    fn loading_an_image_reports_its_size_and_rejects_unknown_playback() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("stimulus.png");
        image::RgbaImage::from_pixel(4, 3, image::Rgba([1, 2, 3, 255]))
            .save(&path)
            .unwrap();

        let mut presenter = EguiPresenter::new(PresenterConfig::default());
        let media = MediaId::new(5);
        presenter
            .apply(PresenterCommand::LoadImage {
                id: media,
                path: path.clone(),
            })
            .unwrap();

        assert_eq!(presenter.media_size(media), Some((4, 3)));
        assert_eq!(
            presenter.apply(PresenterCommand::Play {
                id: MediaId::new(6)
            }),
            Err(PresenterError::UnknownMedia(MediaId::new(6)))
        );

        let missing = directory.path().join("missing.png");
        match presenter.apply(PresenterCommand::LoadImage {
            id: MediaId::new(7),
            path: missing.clone(),
        }) {
            Err(PresenterError::Media { path, message }) => {
                assert_eq!(path, missing);
                assert!(!message.is_empty());
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }
}
