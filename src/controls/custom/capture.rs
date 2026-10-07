#![forbid(unsafe_code)]

//! Filling Direct2D widgets into a window capture.
//!
//! `PrintWindow` asks each child to paint into the capture's DC, which a
//! Direct2D window target cannot do: where the desktop cannot be read, those
//! panes come back blank. Every [`Custom`](super::Custom) registers here, and
//! [`compose`] re-renders the visible Direct2D ones offscreen
//! ([`Custom::render_image`](super::Custom::render_image)) over the capture.
//!
//! The registry is thread-local: widgets, like their windows, belong to the UI
//! thread that captures them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use crate::capture::RgbaImage;
use crate::controls::custom_inner::CustomShared;
use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;
use crate::sys;

use super::{CustomWidget, Renderer, RendererState, surface};

/// A widget that can render itself offscreen.
trait Source {
    /// The widget's offscreen frame, or `None` when it does not paint with
    /// Direct2D (its pixels are already in the capture) or cannot render.
    fn render(&self) -> Option<RgbaImage>;
}

/// The [`Source`] of one [`Custom`](super::Custom), weak so the registry never
/// keeps a destroyed widget alive.
struct WidgetSource<W: CustomWidget, M> {
    shared: Weak<CustomShared<W, M>>,
    renderer: Weak<RefCell<RendererState>>,
    hwnd: Hwnd,
}

impl<W: CustomWidget, M: 'static> Source for WidgetSource<W, M> {
    fn render(&self) -> Option<RgbaImage> {
        let shared = self.shared.upgrade()?;
        let renderer = self.renderer.upgrade()?;
        let direct2d = shared
            .widget
            .try_borrow()
            .is_ok_and(|widget| widget.renderer() == Renderer::Direct2D);
        if !direct2d {
            return None;
        }
        surface::render(&shared, &renderer, self.hwnd).ok()
    }
}

thread_local! {
    static SOURCES: RefCell<HashMap<Hwnd, Rc<dyn Source>>> = RefCell::new(HashMap::new());
}

/// Registers the widget behind `shared` (hosted in `hwnd`) for [`compose`].
pub(super) fn register<W: CustomWidget, M: 'static>(
    hwnd: Hwnd,
    shared: &Rc<CustomShared<W, M>>,
    renderer: &Rc<RefCell<RendererState>>,
) {
    let source: Rc<dyn Source> = Rc::new(WidgetSource {
        shared: Rc::downgrade(shared),
        renderer: Rc::downgrade(renderer),
        hwnd,
    });
    SOURCES.with(|sources| sources.borrow_mut().insert(hwnd, source));
}

/// Removes `hwnd`'s widget (it is being destroyed).
pub(super) fn forget(hwnd: Hwnd) {
    SOURCES.with(|sources| sources.borrow_mut().remove(&hwnd));
}

/// Paints every visible Direct2D widget inside the top-level window `root`
/// over `image`, a `PrintWindow` capture of `root`'s whole window rectangle.
/// Each widget covers only its visible client area, so a widget scrolled or
/// clipped out of view does not paint over its surroundings.
pub(crate) fn compose(root: Hwnd, image: &mut RgbaImage) {
    // Snapshot first: rendering runs widget code, which may create or destroy
    // other widgets.
    let sources: Vec<(Hwnd, Rc<dyn Source>)> = SOURCES.with(|sources| {
        sources
            .borrow()
            .iter()
            .filter(|(hwnd, _)| {
                sys::window::root(**hwnd) == root && sys::window::is_visible(**hwnd)
            })
            .map(|(hwnd, source)| (*hwnd, Rc::clone(source)))
            .collect()
    });
    let origin = sys::window::window_rect(root);
    for (hwnd, source) in sources {
        let visible = sys::window::visible_client_rect(hwnd);
        if visible.is_empty() {
            continue;
        }
        let Some(frame) = source.render() else {
            continue;
        };
        let client = sys::window::client_to_screen(hwnd, Point::new(0, 0));
        let offset = Point::new(client.x - origin.left, client.y - origin.top);
        blit(&frame, visible, offset, image);
    }
}

/// Copies the `area` of `source` (its own pixels) into `target`, with
/// `source`'s origin at `offset`, clipped to both images.
fn blit(source: &RgbaImage, area: Rect, offset: Point, target: &mut RgbaImage) {
    let left = area.left.max(0).max(-offset.x);
    let top = area.top.max(0).max(-offset.y);
    let right = area
        .right
        .min(source.width as i32)
        .min(target.width as i32 - offset.x);
    let bottom = area
        .bottom
        .min(source.height as i32)
        .min(target.height as i32 - offset.y);
    if left >= right || top >= bottom {
        return;
    }
    let row_bytes = (right - left) as usize * 4;
    for y in top..bottom {
        let from = (y as usize * source.width as usize + left as usize) * 4;
        let to = ((y + offset.y) as usize * target.width as usize + (left + offset.x) as usize) * 4;
        target.pixels[to..to + row_bytes].copy_from_slice(&source.pixels[from..from + row_bytes]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(width: u32, height: u32, value: u8) -> RgbaImage {
        RgbaImage {
            width,
            height,
            pixels: vec![value; (width * height * 4) as usize],
        }
    }

    /// The copied area lands at the offset and is clipped to the target, and
    /// pixels outside the visible area are left alone.
    #[test]
    fn blit_copies_the_visible_area_clipped_to_the_target() {
        let source = filled(4, 4, 9);
        let mut target = filled(5, 5, 0);
        blit(
            &source,
            Rect::new(1, 0, 4, 4),
            Point::new(3, -1),
            &mut target,
        );
        for y in 0..5 {
            for x in 0..5 {
                let expected = if x >= 4 && y <= 2 { 9 } else { 0 };
                assert_eq!(target.pixel(x, y).unwrap()[0], expected, "({x}, {y})");
            }
        }
    }
}
