//! The per-thread `ID2D1DCRenderTarget` behind `win32ui::d2d::DcCanvas`.
//!
//! A DC render target draws onto an `HDC` handed to it for one paint (the
//! device context of a `WM_DRAWITEM`/`WM_PAINT`), so it cannot live on a
//! window surface. It is created once per thread and re-bound before each draw;
//! the brushes and geometry caches are shared with the window target.

use core::cell::Cell;
use core::ffi::c_void;
use std::cell::RefCell;

use windows::Win32::Graphics::Gdi::{GetDeviceCaps, HDC, LOGPIXELSX};

use crate::error::{Error, Result};
use crate::geometry::Rect;

use super::EndDraw;
use super::target::Target;

thread_local! {
    /// One unbound DC target per UI thread, created on first use and re-bound
    /// per draw. Dropped when a draw reports a lost device.
    static DC_TARGET: RefCell<Option<Target>> = const { RefCell::new(None) };
    /// Whether a DC frame is open. Only one can be: the target is single.
    static DRAWING: Cell<bool> = const { Cell::new(false) };
}

/// Binds the thread's DC target to `hdc`/`rect`, creating it on first use, and
/// begins a frame. Fails when a frame is already open (a `DcCanvas` was not
/// ended before another was created).
pub(crate) fn begin(hdc: isize, rect: Rect) -> Result<()> {
    if DRAWING.get() {
        return Err(Error::Direct2d("begin while a DC frame is in progress"));
    }
    DC_TARGET.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            *slot = Some(Target::new_dc()?);
        }
        let target = slot.as_mut().expect("just created");
        // SAFETY: `hdc` is a device context valid for the caller's paint; the
        // target only uses it for the duration of the draw.
        let dc = HDC(hdc as *mut c_void);
        if let Err(error) = target.bind_dc(dc, rect) {
            *slot = None;
            return Err(error);
        }
        target.set_dpi(dpi(dc) as f32);
        target.begin_draw();
        DRAWING.set(true);
        Ok(())
    })
}

/// Ends the frame, discarding the target when the device was lost.
pub(crate) fn end() -> Result<EndDraw> {
    if !DRAWING.replace(false) {
        return Ok(EndDraw::Presented);
    }
    let outcome = DC_TARGET.with(|cell| {
        cell.borrow_mut()
            .as_mut()
            .map(Target::end_draw)
            .unwrap_or(Ok(EndDraw::Presented))
    });
    if matches!(outcome, Ok(EndDraw::TargetLost)) {
        discard();
    }
    outcome
}

/// Runs `draw` against the thread's DC target, or returns `None` when no frame
/// is open.
pub(crate) fn with<R>(draw: impl FnOnce(&mut Target) -> R) -> Option<R> {
    DC_TARGET.with(|cell| cell.borrow_mut().as_mut().map(draw))
}

/// Discards the thread's DC target, so the next draw re-creates it.
pub(crate) fn discard() {
    DC_TARGET.with(|cell| *cell.borrow_mut() = None);
    DRAWING.set(false);
}

/// The device context's dots per inch (96 when it cannot be read).
fn dpi(hdc: HDC) -> u32 {
    // SAFETY: `GetDeviceCaps` only reads the DC; a stale handle returns 0.
    let dpi = unsafe { GetDeviceCaps(Some(hdc), LOGPIXELSX) };
    if dpi <= 0 { 96 } else { dpi as u32 }
}

#[cfg(test)]
mod tests {
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
        DeleteDC, DeleteObject, GetDC, HBITMAP, HGDIOBJ, ReleaseDC, SelectObject,
    };

    use super::*;
    use crate::color::Color;
    use crate::d2d::{DcCanvas, PointF};

    /// A 32-bpp top-down DIB selected into a memory DC, so drawn pixels can be
    /// read back directly.
    struct MemoryDc {
        dc: HDC,
        bitmap: HBITMAP,
        old: HGDIOBJ,
        bits: *mut u8,
        width: i32,
    }

    impl MemoryDc {
        fn new(width: i32, height: i32) -> Option<MemoryDc> {
            // SAFETY: a null window asks for the screen DC; the returned handles
            // are owned by `MemoryDc` and released in `Drop`.
            unsafe {
                let screen = GetDC(None);
                if screen.0.is_null() {
                    return None;
                }
                let dc = CreateCompatibleDC(Some(screen));
                let _ = ReleaseDC(None, screen);
                if dc.0.is_null() {
                    return None;
                }
                let info = BITMAPINFO {
                    bmiHeader: BITMAPINFOHEADER {
                        biSize: core::mem::size_of::<BITMAPINFOHEADER>() as u32,
                        biWidth: width,
                        biHeight: -height,
                        biPlanes: 1,
                        biBitCount: 32,
                        biCompression: BI_RGB.0,
                        ..Default::default()
                    },
                    ..Default::default()
                };
                let mut bits: *mut core::ffi::c_void = core::ptr::null_mut();
                let bitmap =
                    CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
                if bitmap.0.is_null() || bits.is_null() {
                    let _ = DeleteDC(dc);
                    return None;
                }
                let old = SelectObject(dc, HGDIOBJ(bitmap.0));
                Some(MemoryDc {
                    dc,
                    bitmap,
                    old,
                    bits: bits.cast(),
                    width,
                })
            }
        }

        fn pixel(&self, x: i32, y: i32) -> (u8, u8, u8) {
            let offset = ((y * self.width + x) * 4) as usize;
            // SAFETY: the DIB section is `width * height * 4` bytes, top-down
            // BGRA, so `offset` is in bounds for the 4 bytes read.
            let bytes = unsafe { core::slice::from_raw_parts(self.bits.add(offset), 4) };
            (bytes[2], bytes[1], bytes[0])
        }
    }

    impl Drop for MemoryDc {
        fn drop(&mut self) {
            // SAFETY: restores the original bitmap before deleting both.
            unsafe {
                SelectObject(self.dc, self.old);
                let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
                let _ = DeleteDC(self.dc);
            }
        }
    }

    /// A filled circle's edge contains pixels that are neither the background
    /// nor the fill: the proof that the shape is anti-aliased. GDI's `Ellipse`
    /// would leave only the two pure colours.
    #[test]
    fn circle_edges_are_anti_aliased() {
        const SIZE: i32 = 64;
        let background = Color::rgb(0x10, 0x10, 0x10);
        let fill = Color::rgb(0xF0, 0xF0, 0xF0);
        let Some(memory) = MemoryDc::new(SIZE, SIZE) else {
            return;
        };
        {
            let Ok(mut canvas) = DcCanvas::new(memory.dc.0 as isize, Rect::new(0, 0, SIZE, SIZE))
            else {
                return;
            };
            canvas.clear(background);
            let center = SIZE as f32 / 2.0;
            canvas.fill_ellipse(
                PointF::new(center, center),
                center - 8.0,
                center - 8.0,
                fill,
            );
            let _ = canvas.end_draw();
        }

        let mut intermediate = 0;
        for y in 0..SIZE {
            for x in 0..SIZE {
                let (r, _, _) = memory.pixel(x, y);
                if r != background.r && r != fill.r {
                    intermediate += 1;
                }
            }
        }
        assert!(
            intermediate > 0,
            "the circle had no anti-aliased edge pixels"
        );
    }

    /// Re-binding the shared target for many draws must keep working and leave
    /// the thread-local target usable.
    #[test]
    fn repeated_binds_are_stable() {
        const SIZE: i32 = 8;
        let black = Color::rgb(0, 0, 0);
        let Some(memory) = MemoryDc::new(SIZE, SIZE) else {
            return;
        };
        for _ in 0..64 {
            let Ok(mut canvas) = DcCanvas::new(memory.dc.0 as isize, Rect::new(0, 0, SIZE, SIZE))
            else {
                return;
            };
            canvas.fill_rect(crate::d2d::RectF::new(0.0, 0.0, 8.0, 8.0), black);
            let _ = canvas.end_draw();
        }
        assert_eq!(memory.pixel(4, 4), (0, 0, 0));
    }
}
