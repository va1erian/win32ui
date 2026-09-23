//! The pure geometry of the extended title bar: the client rectangle once the
//! caption is removed, and what a hit-test point falls on. No Win32 calls, so
//! the arithmetic is unit-tested.

use windows::Win32::UI::WindowsAndMessaging::{
    HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
    HTTOPRIGHT,
};

use crate::geometry::{Point, Rect};

/// The frame borders around an extended-frame window's client area, in pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct FrameInsets {
    pub(super) left: i32,
    pub(super) top: i32,
    pub(super) right: i32,
    pub(super) bottom: i32,
}

/// The client rectangle for a window whose caption has been removed: `window`
/// inset by the frame on the left, right and bottom.
///
/// A restored window's client starts at the window's top edge, so the caption
/// strip (where DWM draws the caption buttons) is client area and the top
/// resize band is hit-tested by the app. A maximized window overhangs the
/// monitor by the frame on every side, so it is inset on top too, landing the
/// client on the monitor. Pure, so the arithmetic is unit-tested.
pub(super) fn extended_client_rect(window: Rect, frame: FrameInsets, maximized: bool) -> Rect {
    Rect::new(
        window.left + frame.left,
        window.top + if maximized { frame.top } else { 0 },
        window.right - frame.right,
        window.bottom - frame.bottom,
    )
}

/// What a hit-test point falls on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Hit {
    Caption,
    Client,
    Left,
    Right,
    Top,
    Bottom,
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// Decides what a hit-test `point` (client coordinates) is: a resize border,
/// the draggable caption strip, or ordinary client content. `over_interactive`
/// is true when the point is over a widget that opted into caption clicks.
pub(super) fn decide(
    point: Point,
    client: Rect,
    frame: FrameInsets,
    caption_height: i32,
    over_interactive: bool,
) -> Hit {
    let left = point.x < client.left + frame.left;
    let right = point.x >= client.right - frame.right;
    let top = point.y < client.top + frame.top;
    let bottom = point.y >= client.bottom - frame.bottom;
    match (left, right, top, bottom) {
        (true, _, true, _) => Hit::TopLeft,
        (_, true, true, _) => Hit::TopRight,
        (true, _, _, true) => Hit::BottomLeft,
        (_, true, _, true) => Hit::BottomRight,
        (true, _, _, _) => Hit::Left,
        (_, true, _, _) => Hit::Right,
        (_, _, true, _) => Hit::Top,
        (_, _, _, true) => Hit::Bottom,
        _ if point.y < client.top + caption_height && !over_interactive => Hit::Caption,
        _ => Hit::Client,
    }
}

/// The `HT*` code Windows expects for `hit`.
pub(super) fn hit_code(hit: Hit) -> u32 {
    match hit {
        Hit::Caption => HTCAPTION,
        Hit::Client => HTCLIENT,
        Hit::Left => HTLEFT,
        Hit::Right => HTRIGHT,
        Hit::Top => HTTOP,
        Hit::Bottom => HTBOTTOM,
        Hit::TopLeft => HTTOPLEFT,
        Hit::TopRight => HTTOPRIGHT,
        Hit::BottomLeft => HTBOTTOMLEFT,
        Hit::BottomRight => HTBOTTOMRIGHT,
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry::{Point, Rect};

    use super::{FrameInsets, Hit, decide, extended_client_rect};

    fn frame() -> FrameInsets {
        FrameInsets {
            left: 8,
            top: 4,
            right: 8,
            bottom: 8,
        }
    }

    #[test]
    fn a_restored_client_keeps_the_top_edge_for_the_caption_strip() {
        let window = Rect::new(0, 0, 800, 600);
        assert_eq!(
            extended_client_rect(window, frame(), false),
            Rect::new(8, 0, 792, 592)
        );
    }

    #[test]
    fn maximized_overhang_is_inset_away() {
        // A maximized window overhangs the monitor by the frame on every side.
        let window = Rect::new(-8, -4, 1928, 1044);
        assert_eq!(
            extended_client_rect(window, frame(), true),
            Rect::new(0, 0, 1920, 1036)
        );
    }

    #[test]
    fn borders_win_over_the_caption_strip() {
        let client = Rect::new(8, 4, 792, 592);
        let frame = frame();
        let height = 32;
        // The top-left corner is a resize handle, not the caption.
        assert_eq!(
            decide(Point::new(2, 2), client, frame, height, false),
            Hit::TopLeft
        );
        assert_eq!(
            decide(Point::new(400, 2), client, frame, height, false),
            Hit::Top
        );
        assert_eq!(
            decide(Point::new(2, 300), client, frame, height, false),
            Hit::Left
        );
        assert_eq!(
            decide(Point::new(790, 300), client, frame, height, false),
            Hit::Right
        );
        assert_eq!(
            decide(Point::new(400, 590), client, frame, height, false),
            Hit::Bottom
        );
    }

    #[test]
    fn free_strip_drags_and_widgets_stay_client() {
        let client = Rect::new(8, 4, 792, 592);
        let frame = frame();
        let height = 32;
        assert_eq!(
            decide(Point::new(400, 20), client, frame, height, false),
            Hit::Caption
        );
        assert_eq!(
            decide(Point::new(400, 20), client, frame, height, true),
            Hit::Client,
            "an interactive widget accepts the click instead of dragging"
        );
        assert_eq!(
            decide(Point::new(400, 100), client, frame, height, false),
            Hit::Client,
            "content below the strip is client area"
        );
    }
}
