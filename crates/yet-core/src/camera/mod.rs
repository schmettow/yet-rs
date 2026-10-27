// SPDX-License-Identifier: GPL-3.0-only

//! Camera roles and timestamped frame sets.
//!
//! Capture belongs to the application crate: this module only defines the
//! vocabulary that lets one extractor consume one eye camera today and two eye
//! cameras plus a world camera tomorrow. A [`FrameSet`] carries timestamps
//! instead of assuming hardware synchronization.

use crate::time::SessionTime;
use crate::tracking::GrayFrame;

/// Role of a camera in a rig.
///
/// v1 has one [`Self::EyeLeft`] camera. Future binocular tracking adds
/// [`Self::EyeRight`]; a head-mounted world camera uses [`Self::World`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CameraRole {
    /// Camera viewing the left eye.
    EyeLeft,
    /// Camera viewing the right eye.
    EyeRight,
    /// Head-mounted camera viewing the world.
    World,
}

/// One borrowed frame tagged with its camera role and capture time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraFrame<'a> {
    /// Which camera produced the frame.
    pub role: CameraRole,
    /// Session-relative capture time.
    pub captured_at: SessionTime,
    /// Grayscale frame.
    pub frame: &'a GrayFrame,
}

/// A set of camera frames at one reference time.
///
/// Multi-camera fusion uses the timestamps, not an assumption of hardware sync.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameSet<'a> {
    /// Frames in rig order.
    pub frames: &'a [CameraFrame<'a>],
    /// Reference time for features extracted from this set.
    pub reference_at: SessionTime,
}

impl<'a> FrameSet<'a> {
    /// The first frame for a role.
    pub fn frame(&self, role: CameraRole) -> Option<&'a CameraFrame<'a>> {
        self.frames.iter().find(|frame| frame.role == role)
    }

    /// Number of frames.
    pub const fn len(&self) -> usize {
        self.frames.len()
    }

    /// Whether the set is empty.
    pub const fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }
}
