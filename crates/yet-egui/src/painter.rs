// SPDX-License-Identifier: GPL-3.0-only

//! Mapping from the backend-neutral display list to egui drawing calls.
//!
//! The canonical scene space is physical pixels. egui paints in logical
//! points, so every coordinate crosses exactly one conversion here. Circles,
//! lines and text positions are snapped to whole physical pixels first, as
//! recommended for calibration targets and gaze dots.

use eframe::egui;
use yet_stim::{Anchor, Color, DrawCommand, FontId, PointPx, RectPx, Scene};

use crate::media::MediaStore;

/// Convert a physical-pixel point to logical points.
pub(crate) fn pos(px: PointPx, pixels_per_point: f32) -> egui::Pos2 {
    egui::pos2(px.x / pixels_per_point, px.y / pixels_per_point)
}

/// Snap a physical-pixel point to whole pixels, then convert it.
pub(crate) fn snapped_pos(px: PointPx, pixels_per_point: f32) -> egui::Pos2 {
    pos(PointPx::new(px.x.round(), px.y.round()), pixels_per_point)
}

/// Convert a physical-pixel rectangle to logical points.
pub(crate) fn rect(rect_px: RectPx, pixels_per_point: f32) -> egui::Rect {
    egui::Rect::from_min_size(
        pos(PointPx::new(rect_px.x, rect_px.y), pixels_per_point),
        egui::vec2(
            rect_px.width / pixels_per_point,
            rect_px.height / pixels_per_point,
        ),
    )
}

/// Convert an sRGB color with straight alpha.
pub(crate) fn color(color: Color) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r, color.g, color.b, color.a)
}

/// Map a protocol font identifier and physical size to an egui font.
///
/// `FontId(0)` is the bundled proportional font and every other identifier is
/// the bundled monospace font. Both come from egui's `default_fonts` feature,
/// so rendering is deterministic and does not depend on the host.
pub(crate) fn font_id(font: FontId, size_px: f32, pixels_per_point: f32) -> egui::FontId {
    let family = if font.get() == 0 {
        egui::FontFamily::Proportional
    } else {
        egui::FontFamily::Monospace
    };
    egui::FontId::new((size_px / pixels_per_point).max(1.0), family)
}

/// Offset from an anchor position to the top-left corner of a text box.
pub(crate) fn anchor_offset(anchor: Anchor, size: egui::Vec2) -> egui::Vec2 {
    let (x, y) = match anchor {
        Anchor::TopLeft => (0.0, 0.0),
        Anchor::TopCenter => (-0.5, 0.0),
        Anchor::TopRight => (-1.0, 0.0),
        Anchor::CenterLeft => (0.0, -0.5),
        Anchor::Center => (-0.5, -0.5),
        Anchor::CenterRight => (-1.0, -0.5),
        Anchor::BottomLeft => (0.0, -1.0),
        Anchor::BottomCenter => (-0.5, -1.0),
        Anchor::BottomRight => (-1.0, -1.0),
    };
    egui::vec2(x * size.x, y * size.y)
}

fn alpha(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn paint_clear(painter: &egui::Painter, clip_rect: egui::Rect, fill: Color) {
    painter.rect_filled(clip_rect, 0.0, color(fill));
}

fn paint_circle(
    painter: &egui::Painter,
    pixels_per_point: f32,
    center_px: PointPx,
    radius_px: f32,
    stroke_px: f32,
    fill: Color,
) {
    let center = snapped_pos(center_px, pixels_per_point);
    let radius = radius_px.round() / pixels_per_point;
    if stroke_px > 0.0 {
        painter.circle_stroke(
            center,
            radius,
            egui::Stroke::new(stroke_px / pixels_per_point, color(fill)),
        );
    } else {
        painter.circle_filled(center, radius, color(fill));
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_text(
    painter: &egui::Painter,
    pixels_per_point: f32,
    pos_px: PointPx,
    text: &str,
    font: FontId,
    size_px: f32,
    fill: Color,
    anchor: Anchor,
) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        font_id(font, size_px, pixels_per_point),
        color(fill),
    );
    let top_left = pos(pos_px, pixels_per_point) + anchor_offset(anchor, galley.size());
    painter.galley(top_left, galley, color(fill));
}

fn paint_line(
    painter: &egui::Painter,
    pixels_per_point: f32,
    from_px: PointPx,
    to_px: PointPx,
    width_px: f32,
    fill: Color,
) {
    painter.line_segment(
        [
            snapped_pos(from_px, pixels_per_point),
            snapped_pos(to_px, pixels_per_point),
        ],
        egui::Stroke::new(width_px / pixels_per_point, color(fill)),
    );
}

/// Paint an overlay scene without a media store.
///
/// The overlay display list is intentionally small: transparent clears, gaze
/// dots and short trails. Images and blurred images are not available; they
/// are skipped rather than blocking the overlay thread.
pub(crate) fn paint_overlay(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    pixels_per_point: f32,
    scene: &Scene,
) {
    for command in scene.commands() {
        match command {
            DrawCommand::Clear(fill) => paint_clear(painter, clip_rect, *fill),
            DrawCommand::Circle {
                center_px,
                radius_px,
                stroke_px,
                color: fill,
            } => paint_circle(
                painter,
                pixels_per_point,
                *center_px,
                *radius_px,
                *stroke_px,
                *fill,
            ),
            DrawCommand::Text {
                pos_px,
                text,
                font,
                size_px,
                color: fill,
                anchor,
            } => paint_text(
                painter,
                pixels_per_point,
                *pos_px,
                text,
                *font,
                *size_px,
                *fill,
                *anchor,
            ),
            DrawCommand::Line {
                from_px,
                to_px,
                width_px,
                color: fill,
            } => paint_line(
                painter,
                pixels_per_point,
                *from_px,
                *to_px,
                *width_px,
                *fill,
            ),
            DrawCommand::Image { .. } | DrawCommand::BlurredImage { .. } => {}
        }
    }
}

/// Paint a scene into `clip_rect`.
pub(crate) fn paint_scene(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    pixels_per_point: f32,
    scene: &Scene,
    media: &mut MediaStore,
    ctx: &egui::Context,
) {
    for command in scene.commands() {
        match command {
            DrawCommand::Clear(fill) => {
                paint_clear(painter, clip_rect, *fill);
            }
            DrawCommand::Image {
                media: id,
                dest_px,
                filter,
                alpha: opacity,
            } => {
                if let Some(texture) = media.texture(ctx, *id, *filter) {
                    painter.image(
                        texture,
                        rect(*dest_px, pixels_per_point),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::from_white_alpha(alpha(*opacity)),
                    );
                }
            }
            DrawCommand::BlurredImage {
                media: id,
                dest_px,
                radius_px,
                alpha: opacity,
            } => {
                if let Some(texture) = media.blurred_texture(ctx, *id, *radius_px) {
                    painter.image(
                        texture,
                        rect(*dest_px, pixels_per_point),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::from_white_alpha(alpha(*opacity)),
                    );
                }
            }
            DrawCommand::Circle {
                center_px,
                radius_px,
                stroke_px,
                color: fill,
            } => {
                paint_circle(
                    painter,
                    pixels_per_point,
                    *center_px,
                    *radius_px,
                    *stroke_px,
                    *fill,
                );
            }
            DrawCommand::Text {
                pos_px,
                text,
                font,
                size_px,
                color: fill,
                anchor,
            } => {
                paint_text(
                    painter,
                    pixels_per_point,
                    *pos_px,
                    text,
                    *font,
                    *size_px,
                    *fill,
                    *anchor,
                );
            }
            DrawCommand::Line {
                from_px,
                to_px,
                width_px,
                color: fill,
            } => {
                paint_line(
                    painter,
                    pixels_per_point,
                    *from_px,
                    *to_px,
                    *width_px,
                    *fill,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use eframe::egui;
    use yet_stim::{Anchor, DrawCommand, Filter, FontId, PointPx, RectPx, Scene};

    use super::{anchor_offset, font_id, pos, rect, snapped_pos};

    #[test]
    fn physical_pixels_become_points_at_100_percent() {
        assert_eq!(pos(PointPx::new(100.0, 50.0), 1.0), egui::pos2(100.0, 50.0));
        assert_eq!(
            rect(RectPx::new(10.0, 20.0, 100.0, 50.0), 1.0),
            egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0))
        );
    }

    #[test]
    fn physical_pixels_become_points_at_150_percent() {
        assert_eq!(pos(PointPx::new(150.0, 75.0), 1.5), egui::pos2(100.0, 50.0));
        assert_eq!(
            rect(RectPx::new(150.0, 300.0, 450.0, 150.0), 1.5),
            egui::Rect::from_min_size(egui::pos2(100.0, 200.0), egui::vec2(300.0, 100.0))
        );
        assert_eq!(font_id(FontId::new(0), 30.0, 1.5).size, 20.0);
    }

    #[test]
    fn shapes_are_snapped_to_whole_physical_pixels() {
        assert_eq!(
            snapped_pos(PointPx::new(10.4, 10.6), 2.0),
            egui::pos2(5.0, 5.5)
        );
    }

    #[test]
    fn text_anchors_offset_the_text_box() {
        let size = egui::vec2(10.0, 6.0);

        assert_eq!(anchor_offset(Anchor::TopLeft, size), egui::vec2(0.0, 0.0));
        assert_eq!(anchor_offset(Anchor::Center, size), egui::vec2(-5.0, -3.0));
        assert_eq!(
            anchor_offset(Anchor::BottomRight, size),
            egui::vec2(-10.0, -6.0)
        );
    }

    #[test]
    fn a_scene_paints_without_a_window() {
        let ctx = egui::Context::default();
        let media = yet_stim::MediaId::new(1);
        let mut store = crate::media::MediaStore::default();
        store.insert(
            media,
            image::RgbaImage::from_pixel(2, 2, image::Rgba([1, 2, 3, 255])),
        );

        let mut scene = Scene::new();
        scene
            .clear(yet_stim::Color::WHITE)
            .image(media, RectPx::new(0.0, 0.0, 2.0, 2.0), Filter::Linear, 1.0)
            .blurred_image(media, RectPx::new(0.0, 0.0, 2.0, 2.0), 1.0, 0.5)
            .circle(PointPx::new(1.0, 1.0), 1.0, 1.0, yet_stim::Color::BLACK)
            .text(
                PointPx::new(1.0, 1.0),
                "x",
                FontId::new(0),
                8.0,
                yet_stim::Color::BLACK,
                Anchor::Center,
            )
            .line(
                PointPx::new(0.0, 0.0),
                PointPx::new(2.0, 2.0),
                1.0,
                yet_stim::Color::BLACK,
            );

        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            super::paint_scene(
                ui.painter(),
                ui.max_rect(),
                1.0,
                &scene,
                &mut store,
                ui.ctx(),
            );
        });
        output.textures_delta.clear();

        assert!(!output.shapes.is_empty());
        assert_eq!(store.len(), 1);
        assert_eq!(scene.len(), 6);
        assert!(matches!(
            scene.commands()[2],
            DrawCommand::BlurredImage { .. }
        ));
    }
}
