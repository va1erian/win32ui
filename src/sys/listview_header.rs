//! The list view's header subclass (owner-drawn header, column-resize veto)
//! and the size subclass that keeps `Fill` columns stretched.

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::Controls::{
    HDN_BEGINTRACK, HDN_ENDTRACK, NM_CUSTOMDRAW, NMCUSTOMDRAW, NMHDR, NMHEADERW,
};
use windows::Win32::UI::Shell::DefSubclassProc;
use windows::Win32::UI::WindowsAndMessaging::{WM_NOTIFY, WM_SIZE};

use crate::geometry::Rect;
use crate::hwnd::Hwnd;

use super::hwnd_from;

// `WM_DPICHANGED_AFTERPARENT`, from `winuser.h`: sent to a per-monitor-v2
// child window (never `WM_DPICHANGED`, which only a top-level window gets)
// after its parent has already handled the DPI change. Reading the new DPI
// back with `GetDpiForWindow` (see `sys::dpi::window_dpi`) is the documented
// way to answer it, since neither `wParam`/`lParam` carry it for this variant.
const WM_DPICHANGED_AFTERPARENT: u32 = 0x02E3;

/// One `NM_CUSTOMDRAW` notification from a list view's header control.
pub(crate) struct HeaderDraw {
    /// The `CDDS_*` stage.
    pub stage: u32,
    /// The header item (column) index.
    pub item: i32,
    /// The DC to paint into.
    pub hdc: HDC,
    /// The item rectangle.
    pub rect: Rect,
}

/// Implemented by the owner of a list view's header to paint it.
pub(crate) trait HeaderPainter {
    /// Handles one header custom-draw stage, returning the `CDRF_*` code to
    /// send back, or `None` to let the header draw itself.
    fn draw_header(&self, draw: &HeaderDraw) -> Option<isize>;

    /// Whether the user may resize `item` by dragging the header divider.
    /// Returning `false` vetoes the drag (`HDN_BEGINTRACK`).
    fn allow_resize(&self, _item: i32) -> bool {
        true
    }

    /// Called after the user finished a header drag (`HDN_ENDTRACK`) for the
    /// header item `item` the divider belonged to, so the new width is honoured
    /// and `Fill` columns can take up the space the drag freed or claimed.
    fn end_track(&self, _item: i32) {}
}

struct HeaderRefdata {
    header: Hwnd,
    painter: Box<dyn HeaderPainter>,
}

/// Owns a header painter and the subclass that feeds it header notifications.
pub(crate) struct HeaderSubclass {
    listview: Hwnd,
    raw: *mut HeaderRefdata,
}

impl HeaderSubclass {
    /// Subclasses `listview` so header `NM_CUSTOMDRAW` notifications reach
    /// `painter`. Returns `None` if subclassing fails.
    pub(crate) fn install(
        listview: Hwnd,
        header: Hwnd,
        painter: Box<dyn HeaderPainter>,
    ) -> Option<HeaderSubclass> {
        let raw = Box::into_raw(Box::new(HeaderRefdata { header, painter }));
        if !super::window::set_subclass(
            listview,
            Some(header_proc),
            HEADER_SUBCLASS_ID,
            raw as usize,
        ) {
            // SAFETY: install failed before the subclass could adopt it.
            unsafe { drop(Box::from_raw(raw)) };
            return None;
        }
        Some(HeaderSubclass { listview, raw })
    }
}

impl Drop for HeaderSubclass {
    fn drop(&mut self) {
        super::window::remove_subclass(self.listview, Some(header_proc), HEADER_SUBCLASS_ID);
        // SAFETY: installed in `install` and reclaimed exactly once.
        unsafe { drop(Box::from_raw(self.raw)) };
    }
}

const HEADER_SUBCLASS_ID: usize = 0x7768_6472; // "whdr"

/// The subclass procedure that routes header notifications.
///
/// # Safety
/// Called by Windows for the list view subclass installed by
/// [`HeaderSubclass::install`]; `refdata` is the `HeaderRefdata` pointer.
unsafe extern "system" fn header_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    if msg == WM_NOTIFY && lparam.0 != 0 {
        // SAFETY: WM_NOTIFY's lparam points at an NMHDR for the duration of the
        // call, and `refdata` is the live `HeaderRefdata` installed by
        // `install`. An NM_CUSTOMDRAW's lparam is an NMCUSTOMDRAW; an
        // HDN_BEGINTRACK/ENDTRACK's lparam is an NMHEADERW.
        unsafe {
            let header = &*(lparam.0 as *const NMHDR);
            let data = &*(refdata as *const HeaderRefdata);
            if hwnd_from(header.hwndFrom) == data.header {
                if header.code == NM_CUSTOMDRAW {
                    let draw = &*(lparam.0 as *const NMCUSTOMDRAW);
                    let request = HeaderDraw {
                        stage: draw.dwDrawStage.0,
                        item: draw.dwItemSpec as i32,
                        hdc: draw.hdc,
                        rect: Rect::new(draw.rc.left, draw.rc.top, draw.rc.right, draw.rc.bottom),
                    };
                    // A panic unwinding across this `extern "system"` boundary
                    // is undefined behaviour; isolate it instead.
                    let painted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        data.painter.draw_header(&request)
                    }))
                    .unwrap_or(None);
                    if let Some(result) = painted {
                        return LRESULT(result);
                    }
                } else if header.code == HDN_BEGINTRACK {
                    let note = &*(lparam.0 as *const NMHEADERW);
                    let allowed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        data.painter.allow_resize(note.iItem)
                    }))
                    .unwrap_or(true);
                    if !allowed {
                        // Returning TRUE vetoes the drag.
                        return LRESULT(1);
                    }
                } else if header.code == HDN_ENDTRACK {
                    // The drag is applied; fall through to the default handling
                    // after offering the painter a chance to honour the new
                    // width and restretch the remaining `Fill` columns.
                    let note = &*(lparam.0 as *const NMHEADERW);
                    let item = note.iItem;
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        data.painter.end_track(item)
                    }));
                }
            }
        }
    }
    // SAFETY: forward to the subclass chain's original window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Implemented by the owner of a list view to restretch `Fill` columns and
/// react to a scale change.
pub(crate) trait SizeHandler {
    /// The list view's client size changed; recompute `Fill` column widths.
    fn on_size(&self);

    /// The list view's effective DPI changed (`WM_DPICHANGED_AFTERPARENT`);
    /// `dpi` is the new value. Default: no-op.
    fn on_dpi_changed(&self, _dpi: u32) {}
}

struct SizeRefdata {
    handler: Box<dyn SizeHandler>,
}

/// Owns a size handler and the subclass that feeds it `WM_SIZE`.
pub(crate) struct SizeSubclass {
    view: Hwnd,
    raw: *mut SizeRefdata,
}

impl SizeSubclass {
    /// Subclasses `view` so `handler` runs on every `WM_SIZE`. Returns `None`
    /// if subclassing fails.
    pub(crate) fn install(view: Hwnd, handler: Box<dyn SizeHandler>) -> Option<SizeSubclass> {
        let raw = Box::into_raw(Box::new(SizeRefdata { handler }));
        if !super::window::set_subclass(view, Some(size_proc), SIZE_SUBCLASS_ID, raw as usize) {
            // SAFETY: install failed before the subclass could adopt it.
            unsafe { drop(Box::from_raw(raw)) };
            return None;
        }
        Some(SizeSubclass { view, raw })
    }
}

impl Drop for SizeSubclass {
    fn drop(&mut self) {
        super::window::remove_subclass(self.view, Some(size_proc), SIZE_SUBCLASS_ID);
        // SAFETY: installed in `install` and reclaimed exactly once.
        unsafe { drop(Box::from_raw(self.raw)) };
    }
}

const SIZE_SUBCLASS_ID: usize = 0x7773_7a65; // "wsze"

/// The subclass procedure that routes `WM_SIZE` to the size handler.
///
/// # Safety
/// Called by Windows for the list view subclass installed by
/// [`SizeSubclass::install`]; `refdata` is the `SizeRefdata` pointer.
unsafe extern "system" fn size_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    refdata: usize,
) -> LRESULT {
    if msg == WM_SIZE {
        // SAFETY: `refdata` is the live `SizeRefdata` installed by `install`.
        // The default handling still runs below; a column-width change never
        // resizes the control, so this cannot loop back into `WM_SIZE`.
        unsafe {
            let data = &*(refdata as *const SizeRefdata);
            // A panic unwinding across this `extern "system"` boundary is
            // undefined behaviour; isolate it instead.
            let _ =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| data.handler.on_size()));
        }
    } else if msg == WM_DPICHANGED_AFTERPARENT {
        // SAFETY: `refdata` is the live `SizeRefdata` installed by `install`;
        // `hwnd` is this same subclassed list view, so reading its own DPI
        // back is safe and gives the value this message does not carry.
        unsafe {
            let dpi = super::dpi::window_dpi(hwnd_from(hwnd));
            let data = &*(refdata as *const SizeRefdata);
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                data.handler.on_dpi_changed(dpi)
            }));
        }
    }
    // SAFETY: forward to the subclass chain's original window procedure.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
