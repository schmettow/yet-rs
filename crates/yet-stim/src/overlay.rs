// SPDX-License-Identifier: GPL-3.0-only

//! UI-free overlay scenes.
//!
//! The overlay is a transparent, click-through layer above the desktop. This
//! module only composes its display list: it knows the presenter geometry and
//! the overlay geometry, maps the gaze point between them, and emits a small
//! scene. Rendering and windowing belong to the platform backend.
//!
//! The layer-shell backend lives in this module; the egui fallback lives in
//! `yet-egui`, because only the UI crate may depend on a windowing toolkit.

pub mod raster;

#[cfg(target_os = "linux")]
mod wayland;

#[cfg(target_os = "linux")]
pub use wayland::LayerShellOverlay;

use crate::{Color, PointPx, Scene, ScreenGeometry};

pub use crate::OverlayError;

use yet_core::GazePoint;

/// Visual style of the gaze marker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GazeMarkerStyle {
    /// Radius of the filled dot in physical pixels.
    pub radius_px: f32,
    /// Dot color; alpha is honored.
    pub color: Color,
    /// Width of the white halo around the dot in physical pixels.
    pub halo_px: f32,
    /// Halo color; alpha is honored.
    pub halo_color: Color,
}

impl Default for GazeMarkerStyle {
    fn default() -> Self {
        Self {
            radius_px: 7.0,
            color: Color::rgba(255, 40, 40, 220),
            halo_px: 2.0,
            halo_color: Color::rgba(255, 255, 255, 230),
        }
    }
}

/// Compose one overlay frame with an optional gaze marker.
///
/// The scene is always cleared to transparent. A gaze point outside the
/// presenter surface is dropped rather than clamped, so a stale or wild
/// prediction does not stick to an edge. The presenter and overlay sizes may
/// differ; the point is scaled proportionally and the overlay is assumed to
/// cover the same physical area.
pub fn gaze_scene(
    presenter: ScreenGeometry,
    overlay: ScreenGeometry,
    gaze: Option<GazePoint>,
    style: &GazeMarkerStyle,
) -> Scene {
    let mut scene = Scene::with_capacity(3);
    scene.clear(Color::TRANSPARENT);

    let Some(point) = gaze else {
        return scene;
    };
    if presenter.size_px.0 == 0
        || presenter.size_px.1 == 0
        || overlay.size_px.0 == 0
        || overlay.size_px.1 == 0
    {
        return scene;
    }
    if !point.x.is_finite()
        || !point.y.is_finite()
        || point.x < 0.0
        || point.y < 0.0
        || point.x > presenter.size_px.0 as f32
        || point.y > presenter.size_px.1 as f32
    {
        return scene;
    }

    let x = point.x * overlay.size_px.0 as f32 / presenter.size_px.0 as f32;
    let y = point.y * overlay.size_px.1 as f32 / presenter.size_px.1 as f32;
    let center = PointPx::new(x, y);

    if style.halo_px > 0.0 {
        scene.circle(
            center,
            style.radius_px + style.halo_px,
            0.0,
            style.halo_color,
        );
    }
    scene.circle(center, style.radius_px, 0.0, style.color);
    scene
}

#[cfg(test)]
mod tests {
    use super::{gaze_scene, GazeMarkerStyle};
    use crate::{Color, DrawCommand, ScreenGeometry};
    use yet_core::GazePoint;

    fn geometry(index: usize, width: u32, height: u32) -> ScreenGeometry {
        ScreenGeometry::new(index, None, (width, height), 1.0)
    }

    #[test]
    fn no_gaze_is_a_transparent_clear() {
        let scene = gaze_scene(
            geometry(0, 800, 600),
            geometry(0, 800, 600),
            None,
            &GazeMarkerStyle::default(),
        );

        assert_eq!(scene.len(), 1);
        assert_eq!(scene.commands(), &[DrawCommand::Clear(Color::TRANSPARENT)]);
    }

    #[test]
    fn a_center_gaze_maps_to_the_overlay_center() {
        let scene = gaze_scene(
            geometry(0, 800, 600),
            geometry(1, 1600, 1200),
            Some(GazePoint::new(400.0, 300.0)),
            &GazeMarkerStyle::default(),
        );

        let centers: Vec<_> = scene
            .commands()
            .iter()
            .filter_map(|command| match command {
                DrawCommand::Circle { center_px, .. } => Some((center_px.x, center_px.y)),
                _ => None,
            })
            .collect();
        assert_eq!(centers, vec![(800.0, 600.0), (800.0, 600.0)]);
    }

    #[test]
    fn an_out_of_bounds_gaze_is_dropped() {
        for point in [
            GazePoint::new(-1.0, 10.0),
            GazePoint::new(10.0, -1.0),
            GazePoint::new(801.0, 10.0),
            GazePoint::new(10.0, 601.0),
            GazePoint::new(f32::NAN, 10.0),
        ] {
            let scene = gaze_scene(
                geometry(0, 800, 600),
                geometry(0, 800, 600),
                Some(point),
                &GazeMarkerStyle::default(),
            );
            assert_eq!(scene.len(), 1, "point {point:?}");
        }
    }

    #[test]
    fn a_zero_sized_geometry_has_no_marker() {
        let scene = gaze_scene(
            geometry(0, 0, 0),
            geometry(0, 800, 600),
            Some(GazePoint::new(0.0, 0.0)),
            &GazeMarkerStyle::default(),
        );

        assert_eq!(scene.len(), 1);
    }

    #[test]
    fn the_halo_can_be_disabled() {
        let style = GazeMarkerStyle {
            halo_px: 0.0,
            ..GazeMarkerStyle::default()
        };
        let scene = gaze_scene(
            geometry(0, 800, 600),
            geometry(0, 800, 600),
            Some(GazePoint::new(400.0, 300.0)),
            &style,
        );

        assert_eq!(scene.len(), 2);
    }
}
