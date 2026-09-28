//! The drag image handed to `IDragSourceHelper::InitializeFromBitmap`.

/// A drag image ready for the shell: premultiplied RGBA pixels (the shell
/// composites 32-bit bitmaps with premultiplied alpha), size and hotspot.
pub(crate) struct DragImageBits {
    pub(crate) width: i32,
    pub(crate) height: i32,
    /// Premultiplied RGBA, four bytes per pixel, top-down.
    pub(crate) pixels: Vec<u8>,
    /// The cursor position inside the image, in image pixels.
    pub(crate) hotspot: (i32, i32),
}

impl DragImageBits {
    /// Premultiplies straight-alpha `rgba` pixels. Returns `None` when the
    /// buffer does not hold `width * height` pixels or the size is empty.
    pub(crate) fn from_straight_rgba(
        width: u32,
        height: u32,
        rgba: &[u8],
        hotspot: (i32, i32),
    ) -> Option<DragImageBits> {
        let count = (width as usize).checked_mul(height as usize)?;
        if count == 0 || rgba.len() < count.checked_mul(4)? {
            return None;
        }
        let mut pixels = rgba[..count * 4].to_vec();
        for pixel in pixels.as_chunks_mut::<4>().0 {
            let alpha = u32::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u32::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        Some(DragImageBits {
            width: width as i32,
            height: height as i32,
            pixels,
            hotspot,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn premultiplies_colour_by_alpha() {
        let image =
            DragImageBits::from_straight_rgba(2, 1, &[255, 128, 0, 128, 10, 20, 30, 255], (1, 0))
                .expect("valid image");
        assert_eq!(&image.pixels[..4], &[128, 64, 0, 128]);
        assert_eq!(&image.pixels[4..], &[10, 20, 30, 255]);
        assert_eq!((image.width, image.height, image.hotspot), (2, 1, (1, 0)));
    }

    #[test]
    fn rejects_short_or_empty_buffers() {
        assert!(DragImageBits::from_straight_rgba(2, 2, &[0; 8], (0, 0)).is_none());
        assert!(DragImageBits::from_straight_rgba(0, 4, &[], (0, 0)).is_none());
    }
}
