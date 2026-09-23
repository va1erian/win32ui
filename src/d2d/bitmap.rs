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
struct Entry {
    data: Arc<RgbaImage>,
    bytes: usize,
    last_used: u64,
}

/// The per-surface image cache: uploads are retained so a lost device can be
/// re-uploaded from memory, bounded by an LRU eviction over bytes.
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

    /// Releases an uploaded image, freeing its memory.
    pub fn forget_image(&mut self, id: ImageId) {
        self.surface.images.borrow_mut().forget(id);
    }
}

impl D2dSurface {
    /// Releases an uploaded image, freeing its memory.
    pub fn forget_image(&self, id: ImageId) {
        self.images.borrow_mut().forget(id);
    }
}
