// SPDX-License-Identifier: GPL-3.0-only

//! A presenter that records everything it receives.
//!
//! This is the headless test double and the reference implementation of the
//! [`Presenter`] protocol.

use std::collections::HashMap;

use crate::geometry::ScreenGeometry;
use crate::protocol::{FrameStamp, PresentStats, Presenter, PresenterCommand, PresenterError};
use crate::scene::{MediaId, Scene};

/// Records applied commands, presented scenes, and frame stamps.
#[derive(Debug)]
pub struct RecordingPresenter {
    geometry: ScreenGeometry,
    commands: Vec<PresenterCommand>,
    scenes: Vec<Scene>,
    frames: Vec<FrameStamp>,
    media_sizes: HashMap<MediaId, (u32, u32)>,
}

impl RecordingPresenter {
    /// Create a recorder for the given monitor geometry.
    pub fn new(geometry: ScreenGeometry) -> Self {
        Self {
            geometry,
            commands: Vec::new(),
            scenes: Vec::new(),
            frames: Vec::new(),
            media_sizes: HashMap::new(),
        }
    }

    /// Change the reported geometry, as a resizing presenter would.
    pub fn set_geometry(&mut self, geometry: ScreenGeometry) {
        self.geometry = geometry;
    }

    /// Declare the natural size of a media identifier.
    ///
    /// A recorder does not decode media, so tests and headless runs announce
    /// the size they want the scene composer to use.
    pub fn with_media_size(mut self, media: MediaId, size: (u32, u32)) -> Self {
        self.media_sizes.insert(media, size);
        self
    }

    /// Declare the natural size of a media identifier in place.
    pub fn set_media_size(&mut self, media: MediaId, size: (u32, u32)) {
        self.media_sizes.insert(media, size);
    }

    /// Commands in the order they were applied.
    pub fn applied_commands(&self) -> &[PresenterCommand] {
        &self.commands
    }

    /// Scenes in the order they were presented.
    pub fn scenes(&self) -> &[Scene] {
        &self.scenes
    }

    /// Frame stamps in the order they were presented.
    pub fn frames(&self) -> &[FrameStamp] {
        &self.frames
    }
}

impl Default for RecordingPresenter {
    fn default() -> Self {
        Self::new(ScreenGeometry::new(0, Some((0, 0)), (0, 0), 1.0))
    }
}

impl Presenter for RecordingPresenter {
    fn geometry(&self) -> ScreenGeometry {
        self.geometry
    }

    fn media_size(&self, media: MediaId) -> Option<(u32, u32)> {
        self.media_sizes.get(&media).copied()
    }

    fn apply(&mut self, command: PresenterCommand) -> Result<(), PresenterError> {
        self.commands.push(command);
        Ok(())
    }

    fn present(&mut self, frame: &FrameStamp, scene: &Scene) -> PresentStats {
        self.frames.push(frame.clone());
        self.scenes.push(scene.clone());

        PresentStats {
            frame: frame.clone(),
            dropped_frames: 0,
            audio_drift_ms: None,
            audio_underruns: 0,
        }
    }
}
