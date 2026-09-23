#![forbid(unsafe_code)]

//! The tab control's subclass callback: themed background, hot-tab tracking
//! and `Ctrl+Tab` between pages.

use std::rc::Weak;

use crate::app::core::Core;
use crate::sys;

use super::paint::{paint_chrome, theme_of};
use super::{TabsShared, apply};

/// The subclass callback installed on the native tab control.
pub(super) fn hover_handler<M: 'static>(
    shared: Weak<TabsShared>,
    core: Weak<Core<M>>,
) -> Box<dyn Fn(sys::tabs::TabEvent) -> Option<isize>> {
    Box::new(move |event| {
        let shared = shared.upgrade()?;
        match event {
            sys::tabs::TabEvent::Erase { dc, bounds } => {
                sys::tabs::fill(dc, bounds, theme_of(&core).background);
                Some(1)
            }
            sys::tabs::TabEvent::Chrome { dc, bounds } => {
                paint_chrome(&shared, &core, dc, bounds, &theme_of(&core));
                None
            }
            sys::tabs::TabEvent::MouseMove { x, y } => {
                let hwnd = shared.hwnd.get();
                if hwnd.is_alive() {
                    let hot = sys::tabs::hit_test(hwnd, x, y);
                    if hot != shared.hot.get() {
                        shared.hot.set(hot);
                        sys::window::invalidate(hwnd);
                    }
                    let _ = sys::window::track_mouse_leave(hwnd);
                }
                None
            }
            sys::tabs::TabEvent::MouseLeave => {
                if shared.hot.get().is_some() {
                    shared.hot.set(None);
                    sys::window::invalidate(shared.hwnd.get());
                }
                None
            }
            sys::tabs::TabEvent::Key { key, ctrl, shift } => {
                let count = shared.count.get();
                if ctrl && sys::tabs::is_tab_key(key) && count > 0 {
                    let current = shared.selected.get().min(count - 1);
                    let next = if shift {
                        (current + count - 1) % count
                    } else {
                        (current + 1) % count
                    };
                    sys::tabs::set_cur_sel(shared.hwnd.get(), next);
                    apply(&shared, &core, true);
                    Some(0)
                } else {
                    None
                }
            }
        }
    })
}
