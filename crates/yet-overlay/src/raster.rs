// SPDX-License-Identifier: GPL-3.0-only

//! A small software rasterizer for overlay scenes.
//!
//! The overlay only needs a transparent clear, filled or stroked circles and
//! capsules (lines). Keeping the rasterizer here avoids a GPU context for a
//! layer-shell surface. Pixels are written as premultiplied `Argb8888` in
//! little-endian BGRA order, which is what `wl_shm` expects on the supported
//! desktop targets.

use yet_present::{Color, DrawCommand, Scene};

/// Render a scene into a `wl_shm`-compatible BGRA canvas.
///
/// `stride` is the number of bytes per row. The canvas is cleared to
/// transparent first. Commands the overlay cannot draw return
/// `OverlayError::Unsupported` instead of being ignored.
pub fn render(
    scene: &Scene,
    width: u32,
    height: u32,
    stride: usize,
    canvas: &mut [u8],
) -> Result<(), super::OverlayError> {
    let required = stride.saturating_mul(height as usize);
    if width == 0 || height == 0 || stride < width as usize * 4 || canvas.len() < required {
        return Err(super::OverlayError::Render(format!(
            "canvas is {} bytes, needs {required} for {width}×{height} at stride {stride}",
            canvas.len()
        )));
    }

    canvas[..required].fill(0);
    let mut frame = Frame {
        width: width as i32,
        height: height as i32,
        stride,
        canvas,
    };

    for command in scene.commands() {
        match command {
            DrawCommand::Clear(color) => frame.clear(*color),
            DrawCommand::Circle {
                center_px,
                radius_px,
                stroke_px,
                color,
            } => frame.circle(center_px.x, center_px.y, *radius_px, *stroke_px, *color),
            DrawCommand::Line {
                from_px,
                to_px,
                width_px,
                color,
            } => frame.line(from_px.x, from_px.y, to_px.x, to_px.y, *width_px, *color),
            DrawCommand::Image { .. } => {
                return Err(super::OverlayError::Unsupported("image".to_owned()));
            }
            DrawCommand::BlurredImage { .. } => {
                return Err(super::OverlayError::Unsupported("blurred image".to_owned()));
            }
            DrawCommand::Text { .. } => {
                return Err(super::OverlayError::Unsupported("text".to_owned()));
            }
        }
    }
    Ok(())
}

struct Frame<'a> {
    width: i32,
    height: i32,
    stride: usize,
    canvas: &'a mut [u8],
}

impl Frame<'_> {
    fn clear(&mut self, color: Color) {
        let pixel = premultiplied(color);
        if pixel == [0, 0, 0, 0] {
            self.canvas.fill(0);
            return;
        }
        for row in 0..self.height as usize {
            let start = row * self.stride;
            let end = start + self.width as usize * 4;
            for chunk in self.canvas[start..end].as_chunks_mut::<4>().0 {
                chunk.copy_from_slice(&pixel);
            }
        }
    }

    fn circle(&mut self, cx: f32, cy: f32, radius: f32, stroke: f32, color: Color) {
        if !cx.is_finite() || !cy.is_finite() || !radius.is_finite() || radius <= 0.0 {
            return;
        }
        let half_stroke = if stroke > 0.0 { stroke * 0.5 } else { 0.0 };
        let outer = radius + half_stroke + 1.0;
        let (x0, x1, y0, y1) = self.bounds(cx - outer, cy - outer, cx + outer, cy + outer);
        for y in y0..y1 {
            let dy = y as f32 + 0.5 - cy;
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - cx;
                let distance = (dx * dx + dy * dy).sqrt();
                let coverage = if stroke > 0.0 {
                    (half_stroke + 0.5 - (distance - radius).abs()).clamp(0.0, 1.0)
                } else {
                    (radius + 0.5 - distance).clamp(0.0, 1.0)
                };
                self.blend(x, y, color, coverage);
            }
        }
    }

    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, width: f32, color: Color) {
        if !x0.is_finite()
            || !y0.is_finite()
            || !x1.is_finite()
            || !y1.is_finite()
            || !width.is_finite()
            || width <= 0.0
        {
            return;
        }
        let half = width * 0.5 + 1.0;
        let (bx0, bx1, by0, by1) = self.bounds(
            x0.min(x1) - half,
            y0.min(y1) - half,
            x0.max(x1) + half,
            y0.max(y1) + half,
        );
        for y in by0..by1 {
            let py = y as f32 + 0.5;
            for x in bx0..bx1 {
                let px = x as f32 + 0.5;
                let distance = distance_to_segment(px, py, x0, y0, x1, y1);
                let coverage = (width * 0.5 + 0.5 - distance).clamp(0.0, 1.0);
                self.blend(x, y, color, coverage);
            }
        }
    }

    fn bounds(&self, x0: f32, y0: f32, x1: f32, y1: f32) -> (i32, i32, i32, i32) {
        let x0 = x0.floor().max(0.0) as i32;
        let y0 = y0.floor().max(0.0) as i32;
        let x1 = x1.ceil().min(self.width as f32) as i32;
        let y1 = y1.ceil().min(self.height as f32) as i32;
        (x0, x1, y0, y1)
    }

    fn blend(&mut self, x: i32, y: i32, color: Color, coverage: f32) {
        if coverage <= 0.0 {
            return;
        }
        let source_alpha = (coverage.clamp(0.0, 1.0) * f32::from(color.a)).round() as u32;
        if source_alpha == 0 {
            return;
        }
        let inverse = 255 - source_alpha;
        let offset = y as usize * self.stride + x as usize * 4;
        let pixel = &mut self.canvas[offset..offset + 4];
        pixel[0] = blend_channel(color.b, source_alpha, pixel[0], inverse);
        pixel[1] = blend_channel(color.g, source_alpha, pixel[1], inverse);
        pixel[2] = blend_channel(color.r, source_alpha, pixel[2], inverse);
        pixel[3] = blend_channel(255, source_alpha, pixel[3], inverse);
    }
}

fn blend_channel(source: u8, alpha: u32, destination: u8, inverse: u32) -> u8 {
    let source = u32::from(source) * alpha / 255;
    (source + (u32::from(destination) * inverse + 127) / 255).min(255) as u8
}

fn premultiplied(color: Color) -> [u8; 4] {
    let alpha = u32::from(color.a);
    [
        (u32::from(color.b) * alpha / 255) as u8,
        (u32::from(color.g) * alpha / 255) as u8,
        (u32::from(color.r) * alpha / 255) as u8,
        color.a,
    ]
}

fn distance_to_segment(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32) -> f32 {
    let dx = x1 - x0;
    let dy = y1 - y0;
    let length_squared = dx * dx + dy * dy;
    if length_squared <= f32::EPSILON {
        return ((px - x0).powi(2) + (py - y0).powi(2)).sqrt();
    }
    let t = (((px - x0) * dx + (py - y0) * dy) / length_squared).clamp(0.0, 1.0);
    let closest_x = x0 + t * dx;
    let closest_y = y0 + t * dy;
    ((px - closest_x).powi(2) + (py - closest_y).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::render;
    use yet_present::{Color, PointPx, Scene};

    fn canvas(width: u32, height: u32) -> Vec<u8> {
        vec![0; width as usize * height as usize * 4]
    }

    fn pixel(canvas: &[u8], width: u32, x: u32, y: u32) -> [u8; 4] {
        let offset = (y as usize * width as usize + x as usize) * 4;
        canvas[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn a_transparent_clear_leaves_a_zero_canvas() {
        let mut buffer = canvas(8, 8);
        let mut scene = Scene::new();
        scene.clear(Color::TRANSPARENT);

        render(&scene, 8, 8, 32, &mut buffer).unwrap();

        assert!(buffer.iter().all(|value| *value == 0));
    }

    #[test]
    fn a_red_dot_is_premultiplied_bgra() {
        let mut buffer = canvas(8, 8);
        let mut scene = Scene::new();
        scene.clear(Color::TRANSPARENT);
        scene.circle(PointPx::new(4.0, 4.0), 2.0, 0.0, Color::rgb(255, 0, 0));

        render(&scene, 8, 8, 32, &mut buffer).unwrap();

        assert_eq!(pixel(&buffer, 8, 4, 4), [0, 0, 255, 255]);
        assert_eq!(pixel(&buffer, 8, 0, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn an_opaque_clear_fills_every_pixel() {
        let mut buffer = canvas(4, 3);
        let mut scene = Scene::new();
        scene.clear(Color::rgb(0, 0, 255));

        render(&scene, 4, 3, 16, &mut buffer).unwrap();

        for value in buffer.as_chunks::<4>().0 {
            assert_eq!(*value, [255, 0, 0, 255]);
        }
    }

    #[test]
    fn a_half_transparent_dot_is_premultiplied() {
        let mut buffer = canvas(8, 8);
        let mut scene = Scene::new();
        scene.clear(Color::TRANSPARENT);
        scene.circle(
            PointPx::new(4.0, 4.0),
            2.0,
            0.0,
            Color::rgba(255, 0, 0, 128),
        );

        render(&scene, 8, 8, 32, &mut buffer).unwrap();

        let center = pixel(&buffer, 8, 4, 4);
        assert_eq!(center[0], 0);
        assert!((center[2] as i32 - 128).abs() <= 1);
        assert!((center[3] as i32 - 128).abs() <= 1);
    }

    #[test]
    fn a_line_draws_along_its_path() {
        let mut buffer = canvas(12, 8);
        let mut scene = Scene::new();
        scene.clear(Color::TRANSPARENT);
        scene.line(
            PointPx::new(1.0, 4.0),
            PointPx::new(10.0, 4.0),
            2.0,
            Color::rgb(0, 255, 0),
        );

        render(&scene, 12, 8, 48, &mut buffer).unwrap();

        assert_eq!(pixel(&buffer, 12, 5, 4), [0, 255, 0, 255]);
        assert_eq!(pixel(&buffer, 12, 5, 0), [0, 0, 0, 0]);
    }

    #[test]
    fn out_of_bounds_shapes_do_not_panic_or_write() {
        let mut buffer = canvas(8, 8);
        let mut scene = Scene::new();
        scene.clear(Color::TRANSPARENT);
        scene.circle(PointPx::new(-100.0, -100.0), 4.0, 0.0, Color::WHITE);
        scene.line(
            PointPx::new(-50.0, -50.0),
            PointPx::new(-40.0, -40.0),
            2.0,
            Color::WHITE,
        );

        render(&scene, 8, 8, 32, &mut buffer).unwrap();

        assert!(buffer.iter().all(|value| *value == 0));
    }

    #[test]
    fn unsupported_commands_are_reported() {
        let mut buffer = canvas(8, 8);
        let mut scene = Scene::new();
        scene.clear(Color::TRANSPARENT);
        scene.text(
            PointPx::new(0.0, 0.0),
            "gaze",
            yet_present::FontId::new(0),
            12.0,
            Color::WHITE,
            yet_present::Anchor::TopLeft,
        );

        let error = render(&scene, 8, 8, 32, &mut buffer).unwrap_err();
        assert!(error.to_string().contains("text"));
    }

    #[test]
    fn a_short_canvas_is_rejected() {
        let mut buffer = vec![0; 10];
        let scene = Scene::new();

        assert!(render(&scene, 8, 8, 32, &mut buffer).is_err());
    }
}
