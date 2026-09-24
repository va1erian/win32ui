#![forbid(unsafe_code)]

//! The built-in vertical scroll host for custom widgets.
//!
//! [`Custom::with_vscroll`](super::Custom::with_vscroll) gives a custom widget
//! a native vertical scrollbar with standard behaviour: thumb tracking, line /
//! page / home / end / arrow / PageUp / PageDown keys, and the wheel with
//! `WHEEL_DELTA` accumulation (high-resolution wheels included). The scroll
//! arithmetic and every `SCROLLINFO` / `WM_VSCROLL` call go through
//! [`sys::scroll`], shared with [`ScrollView`](crate::ScrollView).

use std::cell::{Cell, RefCell};

use crate::app::Ui;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::Key;
use crate::sys;
use crate::units::{Dip, Px};

/// How far one wheel notch scrolls, in design units.
pub(crate) const WHEEL_NOTCH_DIP: f32 = 48.0;

/// Whether `key` is a navigation key the scroll host acts on.
pub(crate) fn is_scroll_key(key: Key) -> bool {
    matches!(
        key,
        Key::UP | Key::DOWN | Key::PAGE_UP | Key::PAGE_DOWN | Key::HOME | Key::END
    )
}

/// Maps the current scroll offset (in design units) to an optional message.
type ScrollMapper<M> = Box<dyn Fn(Dip) -> Option<M>>;

/// The scroll state shared between the widget's window procedure and the
/// [`Custom`](super::Custom) handle. The offset lives in device pixels (what
/// `SCROLLINFO` takes); the public API converts to and from [`Dip`].
pub(crate) struct CustomScroll<M> {
    hwnd: Cell<Hwnd>,
    content_height: Cell<i32>,
    page: Cell<i32>,
    offset: Cell<i32>,
    notch: Cell<i32>,
    wheel_accum: Cell<i32>,
    mapper: RefCell<Option<ScrollMapper<M>>>,
    ui: Ui<M>,
}

impl<M: 'static> CustomScroll<M> {
    pub(crate) fn new(hwnd: Hwnd, notch: i32, ui: Ui<M>) -> CustomScroll<M> {
        CustomScroll {
            hwnd: Cell::new(hwnd),
            content_height: Cell::new(0),
            page: Cell::new(0),
            offset: Cell::new(0),
            notch: Cell::new(notch.max(1)),
            wheel_accum: Cell::new(0),
            mapper: RefCell::new(None),
            ui,
        }
    }

    /// The current offset in device pixels.
    pub(crate) fn offset(&self) -> i32 {
        self.offset.get()
    }

    fn max_offset(&self) -> i32 {
        (self.content_height.get() - self.page.get()).max(0)
    }

    fn clamped(&self) -> i32 {
        self.offset.get().clamp(0, self.max_offset())
    }

    /// Recomputes the page size from the widget's client area and repositions.
    pub(crate) fn on_size(&self) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_alive() {
            return;
        }
        self.page.set(sys::window::client_rect(hwnd).height());
        self.offset.set(self.clamped());
        self.push_info();
    }

    fn push_info(&self) {
        let hwnd = self.hwnd.get();
        if hwnd.is_alive() {
            sys::scroll::set_vertical_info(
                hwnd,
                self.content_height.get(),
                self.page.get(),
                self.clamped(),
            );
        }
    }

    pub(crate) fn set_content_height(&self, height: Dip, dpi: u32) {
        self.content_height.set(height.to_px(dpi).value().max(0));
        self.on_size();
    }

    pub(crate) fn offset_dip(&self, dpi: u32) -> Dip {
        Px(self.offset.get()).to_dip(dpi)
    }

    pub(crate) fn scroll_to(&self, offset: Dip, dpi: u32) {
        self.scroll_to_px(offset.to_px(dpi).value());
    }

    /// Scrolls the minimum amount so `rect` (in device-independent pixels) is
    /// fully inside the viewport.
    pub(crate) fn scroll_into_view(&self, rect: Rect, dpi: u32) {
        let top = Dip(rect.top as f32).to_px(dpi).value();
        let bottom = Dip(rect.bottom as f32).to_px(dpi).value();
        let offset = self.offset.get();
        let page = self.page.get();
        let next = if top < offset {
            top
        } else if bottom > offset + page {
            bottom - page
        } else {
            offset
        };
        self.scroll_to_px(next);
    }

    pub(crate) fn scroll_to_px(&self, offset: i32) {
        self.offset.set(offset.clamp(0, self.max_offset()));
        let hwnd = self.hwnd.get();
        if hwnd.is_alive() {
            sys::scroll::set_vertical_pos(hwnd, self.offset.get());
        }
        self.push_info();
        self.notify();
    }

    pub(crate) fn scroll_by_px(&self, delta: i32) {
        self.scroll_to_px(self.offset.get() + delta);
    }

    /// Accumulates a raw wheel delta and scrolls whole notches.
    pub(crate) fn wheel(&self, delta: i16) {
        let unit = sys::scroll::wheel_delta().max(1);
        let accumulated = self.wheel_accum.get() + i32::from(delta);
        let notches = accumulated / unit;
        self.wheel_accum.set(accumulated % unit);
        if notches != 0 {
            self.scroll_by_px(-notches * self.notch.get());
        }
    }

    /// Applies one `WM_VSCROLL` request decoded by [`sys::scroll`].
    pub(crate) fn scroll_request(&self, request: sys::scroll::ScrollRequest) {
        use sys::scroll::ScrollRequest as Req;
        let page = self.page.get();
        match request {
            Req::LineUp => self.scroll_by_px(-self.notch.get()),
            Req::LineDown => self.scroll_by_px(self.notch.get()),
            Req::PageUp => self.scroll_by_px(-page),
            Req::PageDown => self.scroll_by_px(page),
            Req::Top => self.scroll_to_px(0),
            Req::Bottom => self.scroll_to_px(self.max_offset()),
            Req::ThumbTrack(_) | Req::ThumbPosition(_) => {
                let track = sys::scroll::track_position(self.hwnd.get());
                self.scroll_to_px(track);
            }
            Req::EndScroll => {}
        }
    }

    /// Scrolls for a key, returning whether the key was consumed.
    pub(crate) fn key(&self, key: Key) -> bool {
        match key {
            Key::UP => self.scroll_by_px(-self.notch.get()),
            Key::DOWN => self.scroll_by_px(self.notch.get()),
            Key::PAGE_UP => self.scroll_by_px(-self.page.get()),
            Key::PAGE_DOWN => self.scroll_by_px(self.page.get()),
            Key::HOME => self.scroll_to_px(0),
            Key::END => self.scroll_to_px(self.max_offset()),
            _ => return false,
        }
        true
    }

    pub(crate) fn set_mapper(&self, f: impl Fn(Dip) -> Option<M> + 'static) {
        self.mapper.replace(Some(Box::new(f)));
    }

    /// Maps the current offset through the `on_scroll` closure and enqueues it.
    fn notify(&self) {
        let guard = self.mapper.borrow();
        let Some(mapper) = guard.as_ref() else {
            return;
        };
        if let Some(msg) = mapper(Px(self.offset.get()).to_dip(self.ui.dpi())) {
            self.ui.emit(msg);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::controls::control::ControlExt;
    use crate::controls::custom::{Custom, CustomWidget};
    use crate::gdi::Canvas;
    use crate::geometry::Rect;
    use crate::theme::Theme;
    use crate::units::dip;
    use crate::{App, Ui, WindowSpec, run_app, sys};

    const WATCHDOG_MS: u32 = 5000;
    const WM_MOUSEWHEEL: u32 = 0x020A; // from `WinUser.h`, via the `windows` crate

    /// A display-only widget: no preferred size, GDI paint.
    struct Tall;

    impl CustomWidget for Tall {
        type Event = ();

        fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}
    }

    struct Checks {
        range: Cell<Option<(i32, i32, u32, i32)>>,
        clamped_bottom: Cell<Option<f32>>,
        clamped_top: Cell<Option<f32>>,
        wheel: Cell<Option<f32>>,
        into_view: Cell<Option<f32>>,
        dpi: Cell<u32>,
    }

    struct ScrollApp {
        custom: Custom<Tall, ()>,
        checks: Rc<Checks>,
    }

    impl App for ScrollApp {
        type Msg = ();

        fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
            let hwnd = self.custom.hwnd();

            self.checks
                .range
                .set(Some(sys::scroll::vertical_info(hwnd)));

            self.custom.scroll_to(dip(100_000.0));
            self.checks
                .clamped_bottom
                .set(Some(self.custom.scroll_offset().value()));

            self.custom.scroll_to(dip(-50.0));
            self.checks
                .clamped_top
                .set(Some(self.custom.scroll_offset().value()));

            self.custom.scroll_to(dip(200.0));
            sys::window::send_message(hwnd, WM_MOUSEWHEEL, ((-120i16) as u16 as usize) << 16, 0);
            self.checks
                .wheel
                .set(Some(self.custom.scroll_offset().value()));

            self.custom.scroll_to(dip(0.0));
            self.custom.scroll_into_view(Rect::new(0, 800, 100, 900));
            self.checks
                .into_view
                .set(Some(self.custom.scroll_offset().value()));

            ui.quit();
        }
    }

    /// Content 5000 dip tall in a 300 dip viewport reports the right
    /// `SCROLLINFO` range/page, `scroll_to` clamps, and a wheel notch moves by
    /// one line (48 dip).
    #[test]
    fn scroll_host_reports_range_page_and_scrolls() {
        let checks = Rc::new(Checks {
            range: Cell::new(None),
            clamped_bottom: Cell::new(None),
            clamped_top: Cell::new(None),
            wheel: Cell::new(None),
            into_view: Cell::new(None),
            dpi: Cell::new(96),
        });
        let checks_for_make = Rc::clone(&checks);
        let timed_out = Rc::new(Cell::new(false));
        let timed_out_after = Rc::clone(&timed_out);

        let _ = run_app(
            WindowSpec::new("win32ui.custom.scroll").theme(Theme::light()),
            move |ui| {
                let watchdog = ui.set_timer(WATCHDOG_MS).ok();
                let timed_out_for_timer = Rc::clone(&timed_out);
                ui.on_timer(move |id| {
                    if Some(id) == watchdog {
                        timed_out_for_timer.set(true);
                        crate::quit(1);
                    }
                    None
                });

                checks_for_make.dpi.set(ui.dpi());
                let viewport = dip(300.0).to_px(ui.dpi()).value();
                let custom = Custom::new(ui, Tall).expect("custom").with_vscroll();
                custom.set_bounds(Rect::new(0, 0, viewport, viewport));
                custom.set_content_height(dip(5000.0));
                ui.emit(());
                ScrollApp {
                    custom,
                    checks: checks_for_make,
                }
            },
        );

        assert!(
            !timed_out_after.get(),
            "the watchdog fired before the app quit"
        );
        let dpi = checks.dpi.get();

        let (min, max, page, pos) = checks
            .range
            .get()
            .expect("the update never ran (vertical_info not read)");
        let content = dip(5000.0).to_px(dpi).value();
        let viewport = dip(300.0).to_px(dpi).value();
        assert_eq!(min, 0);
        assert_eq!(max, content - 1);
        assert_eq!(page, viewport as u32);
        assert_eq!(pos, 0);

        let bottom = checks.clamped_bottom.get().expect("clamp not recorded");
        assert!(
            (bottom - dip(4700.0).value()).abs() < 1.0,
            "scrolling past the end should clamp to content - page (got {bottom})"
        );

        let top = checks.clamped_top.get().expect("clamp not recorded");
        assert_eq!(top, 0.0, "scrolling above the top should clamp to zero");

        let wheel = checks.wheel.get().expect("wheel offset not recorded");
        assert!(
            (wheel - dip(248.0).value()).abs() < 1.0,
            "one wheel notch down should scroll 48 dip from 200 (got {wheel})"
        );

        let into_view = checks
            .into_view
            .get()
            .expect("scroll_into_view not recorded");
        assert!(
            (into_view - dip(600.0).value()).abs() < 1.0,
            "scroll_into_view should move a 800..900 band into the 300 viewport (got {into_view})"
        );
    }
}
