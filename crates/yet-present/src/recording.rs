// SPDX-License-Identifier: GPL-3.0-only

//! A presenter that records everything it receives.
//!
//! This is the headless test double and the reference implementation of the
//! [`Presenter`] protocol.

use crate::geometry::ScreenGeometry;
use crate::protocol::{FrameStamp, PresentStats, Presenter, PresenterCommand, PresenterError};
use crate::scene::Scene;

/// Records applied commands, presented scenes, and frame stamps.
#[derive(Debug)]
pub struct RecordingPresenter {
    geometry: ScreenGeometry,
    commands: Vec<PresenterCommand>,
    scenes: Vec<Scene>,
    frames: Vec<FrameStamp>,
}

impl RecordingPresenter {
    /// Create a recorder for the given monitor geometry.
    pub fn new(geometry: ScreenGeometry) -> Self {
        Self {
            geometry,
            commands: Vec::new(),
            scenes: Vec::new(),
            frames: Vec::new(),
        }
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
