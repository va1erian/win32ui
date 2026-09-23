use win32ui::gdi::Bitmap;
use win32ui::prelude::*;

/// Builds a small circular icon as raw RGBA, exercising the DIB-section path.
pub(super) fn dot_icon(color: Color) -> Bitmap {
    const SIZE: i32 = 16;
    let mut pixels = vec![0u8; (SIZE * SIZE * 4) as usize];
    let center = (SIZE as f32 - 1.0) / 2.0;
    let radius = SIZE as f32 / 2.0 - 1.0;
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            if (dx * dx + dy * dy).sqrt() <= radius {
                let offset = ((y * SIZE + x) * 4) as usize;
                pixels[offset] = color.r;
                pixels[offset + 1] = color.g;
                pixels[offset + 2] = color.b;
                pixels[offset + 3] = 0xff;
            }
        }
    }
    Bitmap::from_rgba(SIZE, SIZE, &pixels).expect("build a demo icon")
}
