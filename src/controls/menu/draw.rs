#![forbid(unsafe_code)]

//! Owner-drawn menu items: dark popups and menu bars without any undocumented
//! API. Windows sends `WM_MEASUREITEM`/`WM_DRAWITEM` for `MF_OWNERDRAW` items;
//! these helpers size and paint one item from the window's theme tokens.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Graphics::Gdi::HDC;
use windows::Win32::UI::Controls::{ODS_DISABLED, ODS_NOACCEL, ODS_SELECTED};

use crate::color::Color;
use crate::gdi::{Canvas, Font, TextFormat};
use crate::geometry::{Point, Rect, Size};
use crate::sys;
use crate::theme::Theme;
use crate::units::dip;

use super::RenderItem;

/// Theme colours for painting owner-drawn menu items.
pub(crate) struct MenuPaint {
    /// Popup/bar background.
    pub background: Color,
    /// Highlighted item background.
    pub selection: Color,
    /// Item label.
    pub text: Color,
    /// Shortcut text.
    pub shortcut: Color,
    /// Disabled label and shortcut text.
    pub text_disabled: Color,
    /// Separator line.
    pub border: Color,
}

impl MenuPaint {
    /// Derives a menu palette from the app [`Theme`].
    pub(crate) fn from_theme(theme: &Theme) -> MenuPaint {
        MenuPaint {
            background: theme.raised,
            selection: theme.selection,
            text: theme.text,
            shortcut: theme.text_secondary,
            text_disabled: theme.text_disabled,
            border: theme.border,
        }
    }
}

thread_local! {
    /// One UI font per DPI, reused for every owner-drawn item so a popup does
    /// not create a GDI font per item.
    static MENU_FONT: RefCell<Option<(u32, Rc<Font>)>> = const { RefCell::new(None) };
}

/// The cached menu font for `dpi`, if it could be created.
fn font(dpi: u32) -> Option<Rc<Font>> {
    MENU_FONT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some((cached, font)) = slot.as_ref()
            && *cached == dpi
        {
            return Some(Rc::clone(font));
        }
        let font = Rc::new(Font::system_ui(dpi).ok()?);
        *slot = Some((dpi, Rc::clone(&font)));
        Some(font)
    })
}

/// The natural size of one owner-drawn item, reported from `WM_MEASUREITEM`.
pub(crate) fn measure(item: &RenderItem, dpi: u32) -> Size {
    let fallback = Size::new(dip(140.0).to_px(dpi).value(), dip(24.0).to_px(dpi).value());
    if item.separator {
        return Size::new(fallback.width, dip(7.0).to_px(dpi).value());
    }
    let Some(font) = font(dpi) else {
        return fallback;
    };
    // Native menus size text without the mnemonic `&`; stripping it here keeps
    // the measured width equal to what is actually drawn.
    let label = sys::gdi::measure_text(font.raw(), &without_mnemonics(item.label)).width;
    if item.bar_item {
        let (height, padding) = sys::menu::bar_item_metrics(dpi);
        return Size::new((label + padding * 2).max(1), height);
    }
    let gutter = gutter(dpi);
    let pad = dip(16.0).to_px(dpi).value();
    let shortcut = item
        .shortcut
        .map(|shortcut| sys::gdi::measure_text(font.raw(), &shortcut.to_string()).width)
        .unwrap_or(0);
    let gap = if shortcut > 0 {
        dip(24.0).to_px(dpi).value()
    } else {
        0
    };
    let arrow = if item.submenu {
        dip(20.0).to_px(dpi).value()
    } else {
        0
    };
    let height =
        (font.pixel_height() + dip(8.0).to_px(dpi).value()).max(dip(24.0).to_px(dpi).value());
    Size::new(gutter + label + gap + shortcut + pad + arrow, height)
}

/// Paints one owner-drawn item into the `WM_DRAWITEM` device context. `state`
/// is the raw `ODS_*` flags; selected, disabled and `ODS_NOACCEL` are read.
pub(crate) fn paint_item(
    dc: isize,
    area: Rect,
    state: u32,
    item: &RenderItem,
    paint: &MenuPaint,
    dpi: u32,
) {
    let canvas = Canvas::new(HDC(dc as *mut core::ffi::c_void));
    if item.separator {
        let middle = area.top + area.height() / 2;
        canvas.fill_rect(
            Rect::new(
                area.left + dip(8.0).to_px(dpi).value(),
                middle,
                area.right,
                middle + 1,
            ),
            paint.border,
        );
        return;
    }

    let selected = state & ODS_SELECTED.0 != 0;
    let disabled = state & ODS_DISABLED.0 != 0 || !item.enabled;
    if selected {
        // A rounded, inset highlight, anti-aliased like the glyphs.
        let inset = dip(2.0).to_px(dpi).value();
        let radius = dip(4.0).to_px(dpi).value();
        canvas.round_rect(
            Rect::new(
                area.left + inset,
                area.top + 1,
                area.right - inset,
                area.bottom - 1,
            ),
            radius,
            paint.selection,
            None,
        );
    } else {
        canvas.fill_rect(area, paint.background);
    }

    let text = if disabled {
        paint.text_disabled
    } else {
        paint.text
    };
    let shortcut_color = if disabled {
        paint.text_disabled
    } else {
        paint.shortcut
    };
    let center = area.top + area.height() / 2;

    // A menu-bar item has no leading check/radio gutter; a popup item reserves
    // one. Only popups can carry checks, so skip the gutter and glyphs on bars.
    if !item.bar_item {
        let gutter = gutter(dpi);
        let glyph_x = area.left + dip(6.0).to_px(dpi).value();
        if item.radio && item.checked {
            draw_radio(&canvas, glyph_x, center, dpi, disabled, paint);
        } else if item.checked {
            draw_check(&canvas, glyph_x, center, dpi, text);
        }

        let Some(font) = font(dpi) else {
            return;
        };
        let arrow = if item.submenu {
            dip(20.0).to_px(dpi).value()
        } else {
            0
        };
        let pad = dip(16.0).to_px(dpi).value();
        let mut label_rect = Rect::new(
            area.left + gutter,
            area.top,
            area.right - pad - arrow,
            area.bottom,
        );
        if let Some(shortcut) = item.shortcut {
            let text_value = shortcut.to_string();
            let width = sys::gdi::measure_text(font.raw(), &text_value).width;
            let right = label_rect.right - dip(8.0).to_px(dpi).value();
            canvas.with_font(&font, |canvas| {
                canvas.draw_text(
                    Rect::new(right - width, area.top, right, area.bottom),
                    &text_value,
                    shortcut_color,
                    TextFormat::left().right().vcenter().single_line(),
                );
            });
            label_rect.right = (right - width - dip(8.0).to_px(dpi).value()).max(label_rect.left);
        }
        draw_label(&canvas, &font, label_rect, item.label, state, text);
        if item.submenu {
            draw_chevron(&canvas, area, center, dpi, text);
        }
        return;
    }

    // A menu-bar item: native metrics, no gutter, no shortcut, no chevron.
    let Some(font) = font(dpi) else {
        return;
    };
    let (_, padding) = sys::menu::bar_item_metrics(dpi);
    let label_rect = Rect::new(
        area.left + padding,
        area.top,
        area.right - padding,
        area.bottom,
    );
    draw_label(&canvas, &font, label_rect, item.label, state, text);
}

/// Draws an item label, underlining the mnemonic only when the system wants
/// keyboard cues. Windows conveys that through `ODS_NOACCEL` (set from the
/// `WM_UPDATEUISTATE` cue state and the `SPI_GETKEYBOARDCUES` "always
/// underlined" preference), so when it is set the `&` markers are stripped
/// instead of drawn as underlines.
fn draw_label(canvas: &Canvas, font: &Font, rect: Rect, label: &str, state: u32, color: Color) {
    let format = TextFormat::left().vcenter().single_line().end_ellipsis();
    if underline_mnemonics(state) {
        canvas.with_font(font, |canvas| {
            canvas.draw_text(rect, label, color, format);
        });
    } else {
        let hidden = without_mnemonics(label);
        canvas.with_font(font, |canvas| {
            canvas.draw_text(rect, &hidden, color, format.no_prefix());
        });
    }
}

/// Draws the popup submenu chevron at the right edge of `area`.
fn draw_chevron(canvas: &Canvas, area: Rect, center: i32, dpi: u32, color: Color) {
    let x = area.right - dip(12.0).to_px(dpi).value();
    let half = dip(3.0).to_px(dpi).value().max(3);
    canvas.line(
        Point::new(x - half, center - half),
        Point::new(x, center),
        color,
        1,
    );
    canvas.line(
        Point::new(x, center),
        Point::new(x - half, center + half),
        color,
        1,
    );
}

/// Whether the mnemonic underline should be drawn for an item in `state`.
///
/// `ODS_NOACCEL` (from `Winuser.h` via the `windows` crate, `0x0100`) is set
/// by the system when keyboard cues are hidden; `SPI_GETKEYBOARDCUES` is
/// reflected in the same flag, so this is the whole decision.
fn underline_mnemonics(state: u32) -> bool {
    state & ODS_NOACCEL.0 == 0
}

/// `label` with its mnemonic `&` markers removed. `&&` becomes a literal `&`.
fn without_mnemonics(label: &str) -> String {
    let mut out = String::with_capacity(label.len());
    let mut chars = label.chars();
    while let Some(ch) = chars.next() {
        if ch == '&' {
            match chars.next() {
                Some('&') => out.push('&'),
                Some(next) => out.push(next),
                None => {}
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// The leading gutter reserved for check/radio marks.
fn gutter(dpi: u32) -> i32 {
    dip(24.0).to_px(dpi).value()
}

/// Draws a tick mark starting near (`left`, `center`).
fn draw_check(canvas: &Canvas, left: i32, center: i32, dpi: u32, color: Color) {
    let width = dip(2.0).to_px(dpi).value().max(1);
    let step = dip(4.0).to_px(dpi).value().max(3);
    canvas.line(
        Point::new(left, center),
        Point::new(left + step, center + step),
        color,
        width,
    );
    canvas.line(
        Point::new(left + step, center + step),
        Point::new(left + step * 2 + dip(2.0).to_px(dpi).value(), center - step),
        color,
        width,
    );
}

/// Draws a radio dot centred at (`left`, `center`).
fn draw_radio(
    canvas: &Canvas,
    left: i32,
    center: i32,
    dpi: u32,
    disabled: bool,
    paint: &MenuPaint,
) {
    let radius = dip(5.0).to_px(dpi).value().max(4);
    let rect = Rect::new(left, center - radius, left + radius * 2, center + radius);
    let ring = if disabled {
        paint.text_disabled
    } else {
        paint.shortcut
    };
    canvas.round_rect(rect, radius, paint.background, Some(ring));
    let dot = radius / 2;
    canvas.round_rect(
        Rect::new(
            left + radius - dot,
            center - dot,
            left + radius + dot,
            center + dot,
        ),
        dot,
        if disabled {
            paint.text_disabled
        } else {
            paint.text
        },
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::{underline_mnemonics, without_mnemonics};
    use windows::Win32::UI::Controls::{ODS_DISABLED, ODS_NOACCEL};

    /// Mnemonics are underlined iff the system did not ask to hide them.
    #[test]
    fn underline_follows_keyboard_cues() {
        assert!(underline_mnemonics(0));
        assert!(underline_mnemonics(ODS_DISABLED.0));
        assert!(!underline_mnemonics(ODS_NOACCEL.0));
        assert!(!underline_mnemonics(ODS_NOACCEL.0 | ODS_DISABLED.0));
    }

    /// Hiding cues removes only the mnemonic `&`, keeping a literal `&&`.
    #[test]
    fn without_mnemonics_strips_markers() {
        assert_eq!(without_mnemonics("&File"), "File");
        assert_eq!(without_mnemonics("E&xit"), "Exit");
        assert_eq!(without_mnemonics("A && B"), "A & B");
        assert_eq!(without_mnemonics("Plain"), "Plain");
        assert_eq!(without_mnemonics("trailing&"), "trailing");
    }
}
