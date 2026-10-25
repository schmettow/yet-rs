// SPDX-License-Identifier: GPL-3.0-only

//! Decoded media and their GPU textures.
//!
//! Decoding happens on the caller's thread, texture upload on the UI thread.
//! A texture is uploaded once per media and reused for every frame; a blurred
//! preview is cached per media and radius.

use std::collections::HashMap;

use eframe::egui;
use image::RgbaImage;
use yet_present::{Filter, MediaId};

use crate::blur::box_blur_rgba;

struct MediaEntry {
    image: RgbaImage,
    texture: Option<(Filter, egui::TextureHandle)>,
    blur: Option<(u32, egui::TextureHandle)>,
    /// The image changed since the last upload; reuse the texture handle.
    pending: bool,
}

/// Images available to the presenter, keyed by the protocol's [`MediaId`].
#[derive(Default)]
pub(crate) struct MediaStore {
    entries: HashMap<MediaId, MediaEntry>,
}

impl MediaStore {
    /// Insert a decoded image. Returns `true` when an earlier image was replaced.
    pub fn insert(&mut self, id: MediaId, image: RgbaImage) -> bool {
        self.entries
            .insert(
                id,
                MediaEntry {
                    image,
                    texture: None,
                    blur: None,
                    pending: true,
                },
            )
            .is_some()
    }

    /// Replace the pixels of an existing medium, keeping its texture handle.
    ///
    /// Returns `false` when the medium is not loaded. Used for sequence frames.
    pub fn replace_image(&mut self, id: MediaId, image: RgbaImage) -> bool {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        entry.image = image;
        entry.blur = None;
        entry.pending = true;
        true
    }

    /// Natural size of a loaded image.
    pub fn size(&self, id: MediaId) -> Option<(u32, u32)> {
        self.entries.get(&id).map(|entry| entry.image.dimensions())
    }

    /// Whether an image is loaded.
    pub fn contains(&self, id: MediaId) -> bool {
        self.entries.contains_key(&id)
    }

    /// Number of loaded images.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Texture for an image, uploading it on first use or filter change.
    pub fn texture(
        &mut self,
        ctx: &egui::Context,
        id: MediaId,
        filter: Filter,
    ) -> Option<egui::TextureId> {
        let entry = self.entries.get_mut(&id)?;
        let options = texture_options(filter);

        if let Some((current, handle)) = &mut entry.texture {
            if *current == filter {
                if entry.pending {
                    let color = color_image(&entry.image);
                    handle.set(color, options);
                    entry.pending = false;
                }
                return Some(handle.id());
            }
        }

        let color = color_image(&entry.image);
        let handle = ctx.load_texture(format!("yet-media-{}", id.get()), color, options);
        entry.pending = false;
        entry.texture = Some((filter, handle.clone()));
        Some(handle.id())
    }

    /// Cached blurred texture, creating it on first use or radius change.
    pub fn blurred_texture(
        &mut self,
        ctx: &egui::Context,
        id: MediaId,
        radius_px: f32,
    ) -> Option<egui::TextureId> {
        let radius = radius_px.max(0.0).round() as u32;
        let entry = self.entries.get_mut(&id)?;
        if let Some((current, handle)) = &entry.blur {
            if *current == radius {
                return Some(handle.id());
            }
        }

        let blurred = box_blur_rgba(&entry.image, radius);
        let handle = upload(
            ctx,
            format!("yet-media-{}-blur-{radius}", id.get()),
            &blurred,
            Filter::Linear,
        );
        entry.blur = Some((radius, handle.clone()));
        Some(handle.id())
    }
}

fn texture_options(filter: Filter) -> egui::TextureOptions {
    match filter {
        Filter::Linear => egui::TextureOptions::LINEAR,
        Filter::Nearest => egui::TextureOptions::NEAREST,
    }
}

fn color_image(image: &RgbaImage) -> egui::ColorImage {
    let (width, height) = image.dimensions();
    egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], image.as_raw())
}

fn upload(
    ctx: &egui::Context,
    name: String,
    image: &RgbaImage,
    filter: Filter,
) -> egui::TextureHandle {
    ctx.load_texture(name, color_image(image), texture_options(filter))
}

#[cfg(test)]
mod tests {
    use eframe::egui;
    use image::{Rgba, RgbaImage};
    use yet_present::MediaId;

    use super::MediaStore;

    fn image(width: u32, height: u32) -> RgbaImage {
        RgbaImage::from_pixel(width, height, Rgba([10, 20, 30, 255]))
    }

    #[test]
    fn a_texture_is_uploaded_once_and_reused() {
        let ctx = egui::Context::default();
        let mut store = MediaStore::default();
        let media = MediaId::new(1);
        store.insert(media, image(4, 3));

        let first = store
            .texture(&ctx, media, yet_present::Filter::Linear)
            .unwrap();
        let second = store
            .texture(&ctx, media, yet_present::Filter::Linear)
            .unwrap();

        assert_eq!(first, second);
    }

    #[test]
    fn a_blurred_texture_is_cached_per_radius() {
        let ctx = egui::Context::default();
        let mut store = MediaStore::default();
        let media = MediaId::new(2);
        store.insert(media, image(8, 8));

        let first = store.blurred_texture(&ctx, media, 2.0).unwrap();
        let same = store.blurred_texture(&ctx, media, 2.0).unwrap();
        let other = store.blurred_texture(&ctx, media, 4.0).unwrap();

        assert_eq!(first, same);
        assert_ne!(first, other);
    }

    #[test]
    fn unknown_media_has_no_size_or_texture() {
        let ctx = egui::Context::default();
        let mut store = MediaStore::default();
        let media = MediaId::new(9);

        assert_eq!(store.size(media), None);
        assert!(!store.contains(media));
        assert_eq!(
            store.texture(&ctx, media, yet_present::Filter::Linear),
            None
        );
        assert_eq!(store.len(), 0);
    }
}
