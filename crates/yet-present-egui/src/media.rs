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
                },
            )
            .is_some()
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
        let handle = match &entry.texture {
            Some((current, handle)) if *current == filter => handle.clone(),
            _ => {
                let handle = upload(ctx, id, &entry.image, filter);
                entry.texture = Some((filter, handle.clone()));
                handle
            }
        };
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
        let handle = upload(ctx, id, &blurred, Filter::Linear);
        entry.blur = Some((radius, handle.clone()));
        Some(handle.id())
    }
}

fn upload(
    ctx: &egui::Context,
    id: MediaId,
    image: &RgbaImage,
    filter: Filter,
) -> egui::TextureHandle {
    let (width, height) = image.dimensions();
    let color =
        egui::ColorImage::from_rgba_unmultiplied([width as usize, height as usize], image.as_raw());
    let options = match filter {
        Filter::Linear => egui::TextureOptions::LINEAR,
        Filter::Nearest => egui::TextureOptions::NEAREST,
    };
    ctx.load_texture(format!("yet-media-{}", id.get()), color, options)
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
