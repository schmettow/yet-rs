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
use yet_core::QuadBrightness;
use yet_stim::media::NullAudioPlayer;
use yet_stim::{
    AudioPlayer, FrameStamp, MediaId, PresentStats, Presenter, PresenterCommand, PresenterError,
    Scene, ScreenGeometry, SessionTime,
};

use crate::media::MediaStore;
use crate::painter;
use crate::view;
use yet_stim::playback::PlaybackBank;

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
    /// Outer position in logical points for a windowed viewport.
    pub position_pt: Option<(f32, f32)>,
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
            position_pt: None,
        }
    }
}

impl Default for PresenterConfig {
    fn default() -> Self {
        // The project's reference setup has two monitors; the presenter is the
        // second one (zero-based index 1, labelled "2" in the console).
        Self::new(1, true, (1920, 1080), 1.0)
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

/// Camera picture-in-picture data for the presenter.
///
/// The controller sends this at preview rate; painting happens in the deferred
/// viewport. It is purely visual and never changes the trial scene.
#[derive(Clone, Debug, PartialEq)]
pub struct PresenterPreview {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Row-major 8-bit grayscale pixels.
    pub data: Arc<[u8]>,
    /// Generation used to decide when to re-upload the texture.
    pub generation: u64,
    /// Quadrant means of the eye region.
    pub quad: Option<QuadBrightness>,
    /// Whether to draw the camera stream.
    pub show_stream: bool,
    /// Whether to draw the Quadbright visualization.
    pub show_quad: bool,
}

/// One full camera frame for the adjust view.
///
/// The frame is shown enlarged while the experimenter centers the camera on
/// the eye. `roi` is the eye region normalized to `0..=1` in frame space.
#[derive(Clone, Debug, PartialEq)]
pub struct AdjustFrame {
    /// Frame width in pixels.
    pub width: u32,
    /// Frame height in pixels.
    pub height: u32,
    /// Row-major 8-bit grayscale pixels.
    pub data: Arc<[u8]>,
    /// Generation used to decide when to re-upload the texture.
    pub generation: u64,
    /// Eye region as `[x, y, width, height]` in `0..=1`.
    pub roi: [f32; 4],
}

/// The painting side of a presenter, shared with the controller thread.
pub struct PresenterState {
    config: PresenterConfig,
    geometry: ScreenGeometry,
    media: MediaStore,
    scene: Scene,
    frame: Option<FrameStamp>,
    preview: Option<PresenterPreview>,
    preview_texture: Option<(u64, egui::TextureHandle)>,
    adjust: Option<AdjustFrame>,
    adjust_texture: Option<(u64, egui::TextureHandle)>,
    adjust_hint: Option<String>,
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
            preview: None,
            preview_texture: None,
            adjust: None,
            adjust_texture: None,
            adjust_hint: None,
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

    /// Stable logical size for a windowed viewport.
    ///
    /// Window sizes are logical points; [`Self::geometry`] is physical pixels.
    /// Feeding the painted geometry back into `with_inner_size` would grow the
    /// window on every frame at a non-unit scale factor.
    pub fn window_size_pt(&self) -> (f32, f32) {
        let scale = self.config.nominal_scale_factor.max(0.1);
        (
            self.config.nominal_size_px.0 as f32 / scale,
            self.config.nominal_size_px.1 as f32 / scale,
        )
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

    /// Position a windowed viewport on its monitor, in logical points.
    ///
    /// Fullscreen viewports ignore this; the monitor index selects the output.
    pub fn set_position_pt(&mut self, position: Option<(f32, f32)>) {
        if self.config.position_pt == position {
            return;
        }
        self.config.position_pt = position;
        self.dirty = true;
    }

    /// Position of a windowed viewport, in logical points.
    pub const fn position_pt(&self) -> Option<(f32, f32)> {
        self.config.position_pt
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

    /// Replace the camera picture-in-picture over the stimulus.
    ///
    /// Purely visual: the preview is painted after the scene and never enters
    /// the scene itself or the result file.
    pub fn set_preview(&mut self, preview: Option<PresenterPreview>) {
        let changed = match (&self.preview, &preview) {
            (Some(old), Some(new)) => {
                old.generation != new.generation
                    || old.show_stream != new.show_stream
                    || old.show_quad != new.show_quad
                    || old.quad != new.quad
            }
            (None, None) => false,
            _ => true,
        };
        self.preview = preview;
        if changed {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_of(self.active_viewport);
            }
        }
    }

    /// Replace the enlarged camera view shown while adjusting.
    ///
    /// While this is `Some`, the presenter paints the camera instead of the
    /// stimulus. Clearing it marks the stored scene dirty so the next paint
    /// restores the stimulus.
    pub fn set_adjust(&mut self, adjust: Option<AdjustFrame>) {
        let changed = match (&self.adjust, &adjust) {
            (Some(old), Some(new)) => old.generation != new.generation || old.roi != new.roi,
            (None, None) => false,
            _ => true,
        };
        if adjust.is_none() && self.adjust.is_some() {
            self.dirty = true;
        }
        self.adjust = adjust;
        if changed {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_of(self.active_viewport);
            }
        }
    }

    /// Override the footer hint under the adjust view.
    pub fn set_adjust_hint(&mut self, hint: Option<String>) {
        self.adjust_hint = hint;
    }

    /// Record a scene for the next paint.
    ///
    /// Calling this with an unchanged scene does not request another repaint;
    /// sequence frames mark the state dirty when their pixels change. This
    /// keeps a 60 Hz controller from forcing redundant paints.
    pub fn present(&mut self, frame: &FrameStamp, scene: &Scene) -> PresentStats {
        self.present_frame(frame, scene, None)
    }

    /// Record a scene and an optional new sequence frame for the next paint.
    pub fn present_frame(
        &mut self,
        frame: &FrameStamp,
        scene: &Scene,
        media: Option<(MediaId, RgbaImage)>,
    ) -> PresentStats {
        let dropped = self.dirty;
        if dropped {
            self.coalesced += 1;
        }
        if let Some((media, image)) = media {
            self.media.replace_image(media, image);
            self.dirty = true;
        }
        if self.scene != *scene {
            self.scene.clone_from(scene);
            self.dirty = true;
        }
        self.frame = Some(frame.clone());
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

    /// Whether the viewport has an unpainted scene or frame.
    pub fn needs_paint(&self) -> bool {
        self.dirty
    }

    /// Paint the current scene. Called from the deferred viewport.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        self.active_viewport = ctx.viewport_id();
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
        if let Some(adjust) = self.adjust.clone() {
            self.paint_adjust(ui, clip_rect, &adjust);
            self.paints += 1;
            self.dirty = false;
            return;
        }

        painter::paint_scene(
            ui.painter(),
            clip_rect,
            pixels_per_point,
            &self.scene,
            &mut self.media,
            &ctx,
        );
        self.paint_preview(ui, clip_rect);

        self.paints += 1;
        self.dirty = false;
    }

    /// Paint the enlarged adjust view instead of the scene.
    fn paint_adjust(&mut self, ui: &egui::Ui, clip_rect: egui::Rect, adjust: &AdjustFrame) {
        let painter = ui.painter();
        painter.rect_filled(clip_rect, 0.0, egui::Color32::from_rgb(16, 16, 16));

        let expected = adjust.width as usize * adjust.height as usize;
        if expected == 0 || adjust.data.len() != expected {
            return;
        }
        let uploaded = self
            .adjust_texture
            .as_ref()
            .map(|(generation, _)| *generation);
        if uploaded != Some(adjust.generation) {
            let image = egui::ColorImage::from_gray(
                [adjust.width as usize, adjust.height as usize],
                &adjust.data,
            );
            match &mut self.adjust_texture {
                Some((generation, texture)) => {
                    texture.set(image, egui::TextureOptions::LINEAR);
                    *generation = adjust.generation;
                }
                None => {
                    self.adjust_texture = Some((
                        adjust.generation,
                        ui.ctx().load_texture(
                            "yet-presenter-adjust",
                            image,
                            egui::TextureOptions::LINEAR,
                        ),
                    ));
                }
            }
        }
        let Some((_, texture)) = &self.adjust_texture else {
            return;
        };

        let natural = egui::vec2(adjust.width as f32, adjust.height as f32);
        let scale =
            (clip_rect.width() * 0.94 / natural.x).min(clip_rect.height() * 0.86 / natural.y);
        let rect =
            egui::Rect::from_center_size(clip_rect.center(), natural * scale.max(f32::EPSILON));
        painter.image(
            texture.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(1.0, 0.0), egui::pos2(0.0, 1.0)),
            egui::Color32::WHITE,
        );

        let font = (clip_rect.width() * 0.018).clamp(16.0, 30.0);
        painter::paint_eye_roi(
            painter,
            rect,
            painter::mirrored_roi(adjust.roi),
            egui::Color32::from_rgb(255, 160, 0),
            4.0,
            "eye ROI",
            egui::FontId::proportional(font * 0.8),
        );

        painter.text(
            clip_rect.left_top() + egui::vec2(24.0, 20.0),
            egui::Align2::LEFT_TOP,
            "Adjust · move the camera until the eye is inside the orange box",
            egui::FontId::proportional(font),
            egui::Color32::WHITE,
        );
        painter.text(
            clip_rect.left_bottom() + egui::vec2(24.0, -20.0),
            egui::Align2::LEFT_BOTTOM,
            self.adjust_hint
                .as_deref()
                .unwrap_or("Press F3 (console) to finish"),
            egui::FontId::proportional(font * 0.85),
            egui::Color32::from_gray(210),
        );
    }

    /// Paint the camera stream and Quadbright picture-in-picture overlays.
    fn paint_preview(&mut self, ui: &egui::Ui, clip_rect: egui::Rect) {
        let Some(preview) = self.preview.clone() else {
            return;
        };
        if !preview.show_stream && !preview.show_quad {
            return;
        }

        let width = (clip_rect.width() * 0.20).clamp(120.0, 320.0);
        let margin = (clip_rect.width() * 0.015).clamp(10.0, 28.0);
        let painter = ui.painter();

        if preview.show_stream {
            if let Some(texture) = self.preview_texture(ui.ctx(), &preview) {
                let natural = egui::vec2(preview.width.max(1) as f32, preview.height.max(1) as f32);
                let mut size = egui::vec2(width, width * natural.y / natural.x);
                let max_height = clip_rect.height() * 0.4;
                if size.y > max_height {
                    size = egui::vec2(max_height * natural.x / natural.y, max_height);
                }
                let rect = egui::Rect::from_min_size(
                    egui::pos2(
                        clip_rect.right() - margin - size.x,
                        clip_rect.bottom() - margin - size.y,
                    ),
                    size,
                );
                painter.rect_filled(rect.expand(2.0), 4.0, egui::Color32::from_black_alpha(170));
                painter.image(
                    texture,
                    rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
                painter.rect_stroke(
                    rect,
                    2.0,
                    egui::Stroke::new(1.5, egui::Color32::WHITE),
                    egui::StrokeKind::Inside,
                );
            }
        }

        if preview.show_quad {
            let quad = preview.quad.unwrap_or_default().to_array();
            let size = egui::vec2(width, width * 0.75);
            let rect = egui::Rect::from_min_size(
                egui::pos2(
                    clip_rect.left() + margin,
                    clip_rect.bottom() - margin - size.y,
                ),
                size,
            );
            painter.rect_filled(rect.expand(2.0), 4.0, egui::Color32::from_black_alpha(170));
            let font = (width * 0.075).clamp(11.0, 24.0);
            for (index, value) in quad.iter().enumerate() {
                let column = index % 2;
                let row = index / 2;
                let cell = egui::Rect::from_min_size(
                    rect.min + egui::vec2(column as f32 * size.x / 2.0, row as f32 * size.y / 2.0),
                    egui::vec2(size.x / 2.0, size.y / 2.0),
                );
                let level = value.clamp(0.0, 255.0).round() as u8;
                painter.rect_filled(cell, 0.0, egui::Color32::from_gray(level));
                let text_color = if level > 140 {
                    egui::Color32::BLACK
                } else {
                    egui::Color32::WHITE
                };
                painter.text(
                    cell.center(),
                    egui::Align2::CENTER_CENTER,
                    level.to_string(),
                    egui::FontId::monospace(font),
                    text_color,
                );
            }
            let cross = egui::Stroke::new(1.0, egui::Color32::from_white_alpha(120));
            painter.line_segment([rect.center_top(), rect.center_bottom()], cross);
            painter.line_segment([rect.left_center(), rect.right_center()], cross);
            painter.rect_stroke(
                rect,
                2.0,
                egui::Stroke::new(1.5, egui::Color32::WHITE),
                egui::StrokeKind::Inside,
            );
            painter.text(
                rect.left_top() - egui::vec2(0.0, 4.0),
                egui::Align2::LEFT_BOTTOM,
                "Quadbright · eye ROI",
                egui::FontId::proportional(font),
                egui::Color32::WHITE,
            );
        }
    }

    fn preview_texture(
        &mut self,
        ctx: &egui::Context,
        preview: &PresenterPreview,
    ) -> Option<egui::TextureId> {
        let expected = preview.width as usize * preview.height as usize;
        if expected == 0 || preview.data.len() != expected {
            return None;
        }
        let uploaded = self
            .preview_texture
            .as_ref()
            .map(|(generation, _)| *generation);
        if uploaded != Some(preview.generation) {
            let image = egui::ColorImage::from_gray(
                [preview.width as usize, preview.height as usize],
                &preview.data,
            );
            match &mut self.preview_texture {
                Some((generation, texture)) => {
                    texture.set(image, egui::TextureOptions::LINEAR);
                    *generation = preview.generation;
                }
                None => {
                    self.preview_texture = Some((
                        preview.generation,
                        ctx.load_texture(
                            "yet-presenter-preview",
                            image,
                            egui::TextureOptions::LINEAR,
                        ),
                    ));
                }
            }
        }
        self.preview_texture
            .as_ref()
            .map(|(_, texture)| texture.id())
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
    playback: Arc<Mutex<PlaybackBank>>,
}

impl EguiPresenter {
    /// Create a presenter without audio playback.
    pub fn new(config: PresenterConfig) -> Self {
        Self::with_audio(config, Box::new(NullAudioPlayer), 1.0)
    }

    /// Create a presenter with an audio backend and initial volume.
    pub fn with_audio(config: PresenterConfig, audio: Box<dyn AudioPlayer>, volume: f32) -> Self {
        Self {
            state: Arc::new(Mutex::new(PresenterState::new(config))),
            playback: Arc::new(Mutex::new(PlaybackBank::new(audio, volume))),
        }
    }

    /// Shared state for the viewport closure.
    pub fn shared(&self) -> Arc<Mutex<PresenterState>> {
        Arc::clone(&self.state)
    }

    /// Paint the presenter into the current root viewport.
    ///
    /// The console uses [`crate::show_viewport`] instead; presenter-only mode
    /// makes the stimulus window the root viewport and calls this each frame.
    pub fn show_root(&self, ui: &mut egui::Ui) {
        self.lock().ui(ui);
    }

    /// Current status snapshot.
    pub fn status(&self) -> PresenterStatus {
        self.lock().status()
    }

    fn lock(&self) -> MutexGuard<'_, PresenterState> {
        lock_state(&self.state)
    }

    fn lock_playback(&self) -> MutexGuard<'_, PlaybackBank> {
        self.playback
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Size of a windowed viewport in logical points.
    pub fn window_size_pt(&self) -> (f32, f32) {
        self.lock().window_size_pt()
    }

    /// Change the target monitor or fullscreen mode of the viewport.
    ///
    /// The next [`crate::show_viewport`] call creates a new native window with
    /// a monitor-keyed viewport identifier; the old window is removed.
    pub fn set_monitor(&self, monitor: usize, fullscreen: bool) {
        self.lock().set_monitor(monitor, fullscreen);
    }

    /// Position the windowed presenter on its monitor, in logical points.
    pub fn set_position_pt(&self, position: Option<(f32, f32)>) {
        self.lock().set_position_pt(position);
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

    /// Replace the camera picture-in-picture over the stimulus.
    pub fn set_preview(&self, preview: Option<PresenterPreview>) {
        self.lock().set_preview(preview);
    }

    /// Show or hide the enlarged adjust view.
    pub fn set_adjust(&self, adjust: Option<AdjustFrame>) {
        self.lock().set_adjust(adjust);
    }

    /// Override the footer hint under the adjust view.
    pub fn set_adjust_hint(&self, hint: Option<String>) {
        let mut state = self.lock();
        let changed = state.adjust_hint != hint;
        state.set_adjust_hint(hint);
        if changed {
            if let Some(ctx) = &state.ctx {
                ctx.request_repaint_of(state.active_viewport);
            }
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
            PresenterCommand::LoadSequence {
                id,
                dir,
                fps,
                looped,
                audio,
            } => {
                let first = self
                    .lock_playback()
                    .load_sequence(id, dir, fps, looped, audio)?;
                self.lock().insert_image(id, first);
                Ok(())
            }
            PresenterCommand::ShowMonitor {
                monitor,
                fullscreen,
            } => {
                self.lock().set_monitor(monitor, fullscreen);
                self.request_repaint();
                Ok(())
            }
            PresenterCommand::Play { id } => {
                self.lock().play(id)?;
                self.lock_playback().play(id)?;
                Ok(())
            }
            PresenterCommand::Pause => {
                self.lock_playback().pause();
                Ok(())
            }
            PresenterCommand::Resume => {
                self.lock_playback().resume();
                Ok(())
            }
            PresenterCommand::Stop | PresenterCommand::Shutdown => {
                self.lock_playback().stop();
                self.lock().stop();
                self.request_repaint();
                Ok(())
            }
            PresenterCommand::SetVolume(volume) => {
                self.lock_playback().set_volume(volume);
                Ok(())
            }
        }
    }

    fn present(&mut self, frame: &FrameStamp, scene: &Scene) -> PresentStats {
        let advance = self.lock_playback().advance(frame.experiment_t);
        let underruns = self.lock_playback().underrun();

        let mut stats = match advance {
            Some(advance) => {
                let media_pts = advance.media_pts;
                let audio_pts = advance.audio_pts;
                let master = advance.master;
                let dropped = advance.dropped;
                let drift_ms = advance.drift_ms;
                let mut state = self.lock();
                let mut stats =
                    state.present_frame(frame, scene, Some((advance.media, advance.image)));
                drop(state);

                stats.frame.media_pts = Some(media_pts);
                stats.frame.audio_pts = audio_pts;
                stats.frame.master = master;
                stats.frame.dropped = dropped > 0;
                stats.dropped_frames = stats.dropped_frames.saturating_add(dropped);
                stats.audio_drift_ms = drift_ms;
                stats
            }
            None => self.lock().present(frame, scene),
        };
        stats.audio_underruns = underruns;

        if self.lock().needs_paint() {
            self.request_repaint();
        }
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

    use eframe::egui;
    use yet_stim::{
        ClockSource, FrameStamp, MediaId, Presenter, PresenterCommand, PresenterError, Scene,
        SessionTime,
    };

    use super::{EguiPresenter, PresenterConfig, PresenterState};

    #[test]
    fn a_windowed_size_is_logical_and_stable() {
        let state = PresenterState::new(PresenterConfig::new(0, false, (900, 900), 1.5));

        assert_eq!(state.window_size_pt(), (600.0, 600.0));
    }

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
        let mut white = Scene::new();
        white.clear(yet_stim::Color::WHITE);
        let mut black = Scene::new();
        black.clear(yet_stim::Color::BLACK);

        let first = presenter.present(&frame_stamp(Duration::ZERO), &white);
        let second = presenter.present(&frame_stamp(Duration::from_millis(16)), &black);

        assert!(!first.frame.dropped);
        assert_eq!(first.dropped_frames, 0);
        assert!(second.frame.dropped);
        assert_eq!(second.dropped_frames, 1);
        assert_eq!(presenter.status().presents, 2);
        assert_eq!(presenter.status().coalesced, 1);
        assert_eq!(presenter.status().paints, 0);
    }

    #[test]
    fn an_unchanged_scene_does_not_request_a_repaint() {
        let mut presenter = EguiPresenter::new(PresenterConfig::default());
        let mut scene = Scene::new();
        scene.clear(yet_stim::Color::WHITE);

        let first = presenter.present(&frame_stamp(Duration::ZERO), &scene);
        let ctx = eframe::egui::Context::default();
        let mut output = ctx.run_ui(eframe::egui::RawInput::default(), |ui| {
            presenter.lock().ui(ui);
        });
        output.textures_delta.clear();
        assert_eq!(presenter.status().paints, 1);

        let second = presenter.present(&frame_stamp(Duration::from_millis(16)), &scene);

        assert!(!first.frame.dropped);
        assert!(!second.frame.dropped);
        assert_eq!(presenter.status().coalesced, 0);
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

    #[test]
    fn a_presenter_preview_is_painted_over_the_scene() {
        use std::sync::Arc;

        use yet_core::QuadBrightness;

        use super::PresenterPreview;

        let ctx = egui::Context::default();
        let mut state = PresenterState::new(PresenterConfig::new(0, false, (320, 240), 1.0));
        state.set_preview(Some(PresenterPreview {
            width: 2,
            height: 2,
            data: Arc::from(vec![10_u8, 20, 30, 40, 50, 60, 70, 80]),
            generation: 1,
            quad: Some(QuadBrightness::new(10.0, 20.0, 30.0, 40.0)),
            show_stream: true,
            show_quad: true,
        }));

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| state.ui(ui));

        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();
    }

    #[test]
    fn the_adjust_view_is_painted_instead_of_the_scene() {
        use std::sync::Arc;

        use super::AdjustFrame;

        let ctx = egui::Context::default();
        let mut state = PresenterState::new(PresenterConfig::new(0, false, (320, 240), 1.0));
        state.set_adjust(Some(AdjustFrame {
            width: 2,
            height: 2,
            data: Arc::from(vec![10_u8, 20, 30, 40]),
            generation: 1,
            roi: [0.25, 0.25, 0.5, 0.5],
        }));

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| state.ui(ui));

        assert!(!output.shapes.is_empty());
        output.textures_delta.clear();

        state.set_adjust(None);
        assert!(state.dirty);
    }

    #[test]
    fn an_unchanged_preview_does_not_request_a_new_generation() {
        use std::sync::Arc;

        use super::PresenterPreview;

        let ctx = egui::Context::default();
        let mut state = PresenterState::new(PresenterConfig::new(0, false, (320, 240), 1.0));
        state.ctx = Some(ctx);
        let preview = PresenterPreview {
            width: 1,
            height: 1,
            data: Arc::from(vec![128_u8]),
            generation: 3,
            quad: None,
            show_stream: true,
            show_quad: false,
        };

        state.set_preview(Some(preview.clone()));
        state.set_preview(Some(preview));

        assert_eq!(state.preview.as_ref().map(|p| p.generation), Some(3));
    }
}
