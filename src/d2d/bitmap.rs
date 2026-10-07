#![forbid(unsafe_code)]

//! RGBA bitmap uploads and drawing.
//!
//! [`D2dCanvas::image`] uploads an [`RgbaImage`](crate::RgbaImage) and returns
//! an [`ImageId`]; the image is cached on the surface (LRU, bounded by bytes)
//! and re-uploaded to the device lazily after a device loss. The RGBA data is
//! kept in an [`Arc`](std::sync::Arc) so the device copy can be rebuilt.

use std::collections::HashMap;
use std::sync::Arc;

use crate::capture::RgbaImage;

use super::RectF;
use super::canvas::D2dCanvas;
use super::surface::D2dSurface;

/// A handle to an image uploaded with [`D2dCanvas::image`], valid for the life
/// of the surface it was uploaded to (or until [`D2dSurface::forget_image`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageId(pub(crate) usize);

/// How an image is resampled when drawn scaled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Interpolation {
    /// Nearest-neighbour: crisp pixels, no smoothing.
    Nearest,
    /// Bilinear: smooth, the default.
    #[default]
    Linear,
    /// The highest-quality mode the target supports (maps to bilinear on the
    /// classic `ID2D1HwndRenderTarget`).
    HighQualityCubic,
}

/// The default cache budget, in bytes of RGBA data.
const DEFAULT_BUDGET_BYTES: usize = 64 * 1024 * 1024;

/// The uploaded image's retained data.
#[derive(Clone)]
struct Entry {
    data: Arc<RgbaImage>,
    bytes: usize,
    last_used: u64,
}

/// The per-surface image cache: uploads are retained so a lost device can be
/// re-uploaded from memory, bounded by an LRU eviction over bytes. A clone
/// shares the retained data and keeps every [`ImageId`]; the device bitmaps are
/// re-uploaded lazily by whichever target draws them.
#[derive(Clone)]
pub(super) struct ImageCache {
    images: HashMap<ImageId, Entry>,
    next_id: usize,
    clock: u64,
    bytes: usize,
    budget: usize,
}

impl ImageCache {
    pub(super) fn new() -> ImageCache {
        ImageCache {
            images: HashMap::new(),
            next_id: 0,
            clock: 0,
            bytes: 0,
            budget: DEFAULT_BUDGET_BYTES,
        }
    }

    /// Uploads `image`, returning its id (evicting the least-recently-used
    /// images first if the cache is over budget).
    pub(super) fn insert(&mut self, image: &RgbaImage) -> ImageId {
        let id = ImageId(self.next_id);
        self.next_id += 1;
        let bytes = image.pixels.len();
        let last_used = self.tick();
        self.images.insert(
            id,
            Entry {
                data: Arc::new(image.clone()),
                bytes,
                last_used,
            },
        );
        self.bytes += bytes;
        self.evict();
        id
    }

    /// Marks `id` as used and returns its data, if still cached.
    pub(super) fn touch(&mut self, id: ImageId) -> Option<Arc<RgbaImage>> {
        let last_used = self.tick();
        let entry = self.images.get_mut(&id)?;
        entry.last_used = last_used;
        Some(Arc::clone(&entry.data))
    }

    /// Drops `id` and releases its memory.
    pub(super) fn forget(&mut self, id: ImageId) {
        if let Some(entry) = self.images.remove(&id) {
            self.bytes = self.bytes.saturating_sub(entry.bytes);
        }
    }

    /// Drops every retained image, releasing their RGBA memory. The id counter
    /// is left as is, so ids issued before this call stay unique; a draw with a
    /// stale id finds nothing and uploads nothing.
    pub(super) fn clear(&mut self) {
        self.images.clear();
        self.bytes = 0;
    }

    /// Adds the images `other` holds that this cache doesn't, and moves the id
    /// counter past `other`'s. Nothing is removed: an image `other` forgot or
    /// evicted stays here, so merging a cache cloned from this one (an
    /// offscreen frame's) only carries over what that frame uploaded.
    pub(super) fn merge_new(&mut self, other: &ImageCache) {
        for (id, entry) in &other.images {
            if self.images.contains_key(id) {
                continue;
            }
            let last_used = self.tick();
            self.bytes += entry.bytes;
            self.images.insert(
                *id,
                Entry {
                    last_used,
                    ..entry.clone()
                },
            );
        }
        self.next_id = self.next_id.max(other.next_id);
        self.evict();
    }

    fn tick(&mut self) -> u64 {
        self.clock += 1;
        self.clock
    }

    fn evict(&mut self) {
        while self.bytes > self.budget && !self.images.is_empty() {
            let oldest = self
                .images
                .iter()
                .min_by_key(|(_, entry)| entry.last_used)
                .map(|(&id, _)| id);
            if let Some(id) = oldest {
                self.forget(id);
            }
        }
    }
}

impl Default for ImageCache {
    fn default() -> ImageCache {
        ImageCache::new()
    }
}

impl<'a> D2dCanvas<'a> {
    /// Uploads `image` and returns its [`ImageId`]. Uploads are cached on the
    /// surface and re-created lazily after a device loss.
    pub fn image(&mut self, image: &RgbaImage) -> ImageId {
        self.surface.images.borrow_mut().insert(image)
    }

    /// Draws the image (or a source rectangle of it) into `dest`, at `opacity`
    /// in `0.0..=1.0`. `src` is in image pixels; `None` draws the whole image.
    pub fn draw_image(
        &mut self,
        id: ImageId,
        dest: RectF,
        src: Option<RectF>,
        opacity: f32,
        interpolation: Interpolation,
    ) {
        let data = self.surface.images.borrow_mut().touch(id);
        if let Some(data) = data {
            self.with(|target| target.draw_image(id, &data, dest, src, opacity, interpolation));
        }
    }

    /// Tiles the image across `dest` (CSS `background-repeat`): a bitmap brush
    /// with wrap extend mode fills the rectangle.
    pub fn fill_image_tiled(
        &mut self,
        id: ImageId,
        dest: RectF,
        opacity: f32,
        interpolation: Interpolation,
    ) {
        let data = self.surface.images.borrow_mut().touch(id);
        if let Some(data) = data {
            self.with(|target| target.fill_image_tiled(id, &data, dest, opacity, interpolation));
        }
    }

    /// Releases an uploaded image, freeing its memory: the retained RGBA data
    /// and the device bitmap it was uploaded to.
    pub fn forget_image(&mut self, id: ImageId) {
        self.surface.forget_image(id);
    }
}

impl D2dSurface {
    /// Releases an uploaded image, freeing its retained RGBA data and the
    /// device bitmap, if the target is live.
    pub fn forget_image(&self, id: ImageId) {
        self.images.borrow_mut().forget(id);
        if let Some(target) = self.target.borrow_mut().as_mut() {
            target.images.forget(id);
        }
    }

    /// Releases every image uploaded to this surface: the retained RGBA cache
    /// and, if the target is live, its device bitmaps and tiled brushes.
    ///
    /// Unlike dropping the whole surface, the render target is kept, so this
    /// frees the bulk of a heavy view's memory (the uploaded covers) without a
    /// recreate — and therefore without the unpainted frame that a fresh target
    /// shows until its first paint. Image ids issued before this call are no
    /// longer valid; a caller that cached handles must drop them so the next
    /// paint re-uploads.
    pub fn release_images(&self) {
        self.images.borrow_mut().clear();
        if let Some(target) = self.target.borrow_mut().as_mut() {
            target.images.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::RgbaImage;

    fn solid(width: u32, height: u32, byte: u8) -> RgbaImage {
        RgbaImage {
            width,
            height,
            pixels: vec![byte; (width * height * 4) as usize],
        }
    }

    #[test]
    fn clear_drops_every_retained_image_and_its_bytes() {
        let mut cache = ImageCache::new();
        let first = cache.insert(&solid(4, 4, 0x11));
        let second = cache.insert(&solid(2, 2, 0x22));
        assert!(cache.bytes > 0);
        assert!(cache.touch(first).is_some());

        cache.clear();

        assert_eq!(cache.bytes, 0);
        assert!(cache.touch(first).is_none());
        assert!(cache.touch(second).is_none());
        // Ids stay unique across the clear, so a stale id never aliases a new
        // upload.
        let third = cache.insert(&solid(1, 1, 0x33));
        assert_ne!(third, first);
        assert_ne!(third, second);
    }

    #[test]
    fn merge_new_adds_uploads_and_keeps_what_the_other_cache_dropped() {
        let mut window = ImageCache::new();
        let kept = window.insert(&solid(2, 2, 0x11));
        let forgotten = window.insert(&solid(2, 2, 0x22));

        let mut offscreen = window.clone();
        offscreen.forget(forgotten);
        let uploaded = offscreen.insert(&solid(3, 3, 0x33));

        window.merge_new(&offscreen);

        assert!(window.touch(kept).is_some());
        assert!(window.touch(forgotten).is_some(), "a forget leaked back");
        assert!(
            window.touch(uploaded).is_some(),
            "the new upload is missing"
        );
        assert_eq!(window.bytes, (2 * 2 + 2 * 2 + 3 * 3) * 4);
        // The counter moved past the offscreen upload, so ids stay unique.
        let next = window.insert(&solid(1, 1, 0x44));
        assert!(![kept, forgotten, uploaded].contains(&next));
    }
}
