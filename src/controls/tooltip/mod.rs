#![forbid(unsafe_code)]

//! The shared tooltip window: one `tooltips_class32` window per top-level
//! window, created lazily on the first tooltip and owned by that window.
//!
//! Widgets never see this type. [`ControlExt::set_tooltip`](crate::ControlExt::set_tooltip)
//! and [`WidgetCx::set_tooltip_region`](crate::WidgetCx::set_tooltip_region)
//! add their tool to the shared window; a [`ToolbarItem`](crate::ToolbarItem)
//! adds one region tool per button.
//!
//! On a dark theme the tooltip is owner-drawn through the documented
//! `NM_CUSTOMDRAW` notification (see `sys::tooltip`), because the visual-style
//! path ignores the tip colours and `DarkMode_Explorer` alone does not darken
//! a tooltip. On the light theme the native tooltip draws itself.

#[cfg(test)]
mod tests;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use windows::Win32::UI::Controls::{CDDS_POSTPAINT, CDDS_PREPAINT, NM_CUSTOMDRAW};

use crate::controls::registry::{self, ControlEvents, ControlKind};
use crate::gdi::{Canvas, Font, TextFormat};
use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;
use crate::sys;
use crate::theme::{self, Theme};
use crate::units::dip;

thread_local! {
    /// The shared tooltip window per top-level window, keyed by the root
    /// `HWND`.
    static TOOLTIPS: RefCell<HashMap<usize, Rc<RefCell<TooltipWindow>>>> =
        RefCell::new(HashMap::new());
}

/// One tool added to the shared tooltip.
struct Tool {
    /// The widget the tool belongs to.
    owner: Hwnd,
    /// `true` when the tool is keyed by its own `HWND` (a whole-widget
    /// tooltip), `false` for a region tooltip keyed by `slot`.
    id_is_hwnd: bool,
    /// `TOOLINFO.uId` for a region tooltip; the owner handle for a whole-widget
    /// tooltip (where it is ignored).
    slot: usize,
    /// The NUL-terminated text, kept alive for the tool's lifetime.
    wide: Box<[u16]>,
    /// The text as `str`, so an unchanged update sends nothing.
    text: String,
    /// The last region, so an unchanged update sends nothing.
    rect: Rect,
}

impl Tool {
    /// Whether `screen` (a screen point) is inside the tool's area.
    fn contains(&self, screen: Point) -> bool {
        let origin = sys::tooltip::client_origin(self.owner);
        let rect = if self.id_is_hwnd {
            let client = sys::window::client_rect(self.owner);
            Rect::new(
                origin.x,
                origin.y,
                origin.x + client.width(),
                origin.y + client.height(),
            )
        } else {
            Rect::new(
                origin.x + self.rect.left,
                origin.y + self.rect.top,
                origin.x + self.rect.right,
                origin.y + self.rect.bottom,
            )
        };
        rect.contains(screen)
    }
}

/// The shared tooltip window for one top-level window.
struct TooltipWindow {
    hwnd: Hwnd,
    dark: Cell<bool>,
    theme: RefCell<Theme>,
    /// The font comctl32 sizes the tooltip with and the owner-draw paints with;
    /// created at the tooltip's DPI and recreated on a DPI change.
    font: RefCell<Option<Font>>,
    /// Tools keyed by `(tool owner HWND, slot)`.
    tools: RefCell<HashMap<(usize, usize), Tool>>,
}

impl TooltipWindow {
    /// Applies a theme live: recolours the native parts and repaints, without
    /// recreating the tooltip or its tools.
    fn apply(&self, theme: &Theme) {
        self.dark.set(theme.is_dark);
        *self.theme.borrow_mut() = *theme;
        sys::tooltip::set_theme(self.hwnd, theme.is_dark, theme.raised, theme.text);
        // `SetWindowTheme` restores the default font, so the sizing font is
        // re-applied after every theme change, not only at creation.
        self.apply_font();
        sys::window::invalidate(self.hwnd);
    }

    /// Sets the font comctl32 sizes the tooltip with and the owner-draw paints
    /// with, so the two always agree. Long tips wrap at a DPI-scaled width
    /// instead of clipping.
    fn apply_font(&self) {
        let dpi = sys::dpi::window_dpi(self.hwnd);
        let Ok(font) = Font::system_ui(dpi) else {
            return;
        };
        sys::tooltip::apply_font_metrics(self.hwnd, font.raw(), max_tip_width(dpi));
        self.font.replace(Some(font));
    }

    /// Adds or updates a tool, keyed by `(owner, slot)`. `rect` is `None` for a
    /// whole-widget tooltip.
    fn set_tool(&self, owner: Hwnd, slot: usize, id_is_hwnd: bool, rect: Option<Rect>, text: &str) {
        let key = (owner.raw(), slot);
        let rect = rect.unwrap_or_default();
        let mut tools = self.tools.borrow_mut();
        if let Some(tool) = tools.get_mut(&key) {
            if tool.text != text {
                tool.wide = wide(text);
                tool.text = text.to_string();
                sys::tooltip::update_tool_text(self.hwnd, owner, slot, id_is_hwnd, &tool.wide);
            }
            if !id_is_hwnd && tool.rect != rect {
                tool.rect = rect;
                sys::tooltip::update_tool_rect(self.hwnd, owner, slot, rect);
            }
            return;
        }
        let text_wide = wide(text);
        let added = if id_is_hwnd {
            sys::tooltip::add_control_tool(self.hwnd, owner, &text_wide)
        } else {
            sys::tooltip::add_region_tool(self.hwnd, owner, slot, rect, &text_wide)
        };
        if added {
            tools.insert(
                key,
                Tool {
                    owner,
                    id_is_hwnd,
                    slot,
                    wide: text_wide,
                    text: text.to_string(),
                    rect,
                },
            );
        }
    }

    /// The text of the tool whose area contains the cursor, for the paint.
    ///
    /// The shown text is read from our own tool list, never by sending a
    /// `TTM_*` message back to the control: a tooltip blocked in
    /// `SendMessage` for its `NM_CUSTOMDRAW` faults if it is re-entered.
    fn tool_under_cursor(&self) -> String {
        let cursor = sys::tooltip::cursor_position();
        self.tools
            .borrow()
            .values()
            .find(|tool| tool.contains(cursor))
            .map(|tool| tool.text.clone())
            .unwrap_or_default()
    }

    /// Removes every tool owned by `owner`.
    fn remove_widget(&self, owner: Hwnd) {
        let removed: Vec<(usize, usize)> = self
            .tools
            .borrow()
            .keys()
            .filter(|(hwnd, _)| *hwnd == owner.raw())
            .copied()
            .collect();
        for key in removed {
            if let Some(tool) = self.tools.borrow_mut().remove(&key) {
                sys::tooltip::remove_tool(self.hwnd, owner, tool.slot, tool.id_is_hwnd);
            }
        }
    }
}

impl ControlEvents for TooltipWindow {
    fn kind(&self) -> ControlKind {
        ControlKind::Tooltip
    }

    fn on_notification(
        &mut self,
        _hwnd: Hwnd,
        code: u32,
        _wparam: usize,
        lparam: isize,
    ) -> Option<isize> {
        if code != NM_CUSTOMDRAW {
            return None;
        }
        // The light tooltip is native; only consume the notification so it does
        // not reach the application as an opaque `Notify::Other`.
        if !self.dark.get() {
            return Some(0);
        }
        let theme = *self.theme.borrow();
        let dpi = sys::dpi::window_dpi(self.hwnd);
        let pad = padding(dpi);
        let text = self.tool_under_cursor();

        Some(sys::tooltip::custom_draw(lparam, |draw| match draw.stage {
            stage if stage == CDDS_PREPAINT.0 => sys::tooltip::TooltipDrawResult::NotifyPostPaint,
            stage if stage == CDDS_POSTPAINT.0 => {
                // The native prepaint already drew the light body; cover it and
                // draw the text ourselves from theme tokens. An empty text is
                // skipped: `DrawTextW` faults on an empty buffer.
                let canvas = Canvas::new(draw.hdc);
                canvas.fill_rect(draw.rect, theme.raised);
                canvas.outline(draw.rect, theme.border);
                if !text.is_empty() {
                    // Paint in the font comctl32 sized the window with, so the
                    // full text (shortcut suffix included) always fits. Some
                    // themes reserve padding around the text and some do not, so
                    // the inset is applied only while it still leaves the whole
                    // text visible; a tip too wide for one line wraps (the window
                    // is wrapped by `TTM_SETMAXTIPWIDTH`).
                    let font = self.font.borrow();
                    let paint = |canvas: &Canvas| {
                        let needed = sys::gdi::text_extent(draw.hdc, &text).width;
                        let inset = text_inset(draw.rect.width(), needed, pad);
                        let rect = draw.rect.shrink(inset);
                        let format = if needed <= rect.width() {
                            TextFormat::left().vcenter().single_line().no_prefix()
                        } else {
                            TextFormat::left().word_wrap().no_prefix()
                        };
                        canvas.draw_text(rect, &text, theme.text, format);
                    };
                    match font.as_ref() {
                        Some(font) => canvas.with_font(font, |canvas| paint(canvas)),
                        None => paint(&canvas),
                    }
                }
                sys::tooltip::TooltipDrawResult::SkipDefault
            }
            _ => sys::tooltip::TooltipDrawResult::Default,
        }))
    }
}

/// Sets a whole-widget tooltip on `owner`.
pub(crate) fn set_control_tooltip(owner: Hwnd, text: &str) {
    let Some(state) = shared_for(owner) else {
        return;
    };
    state
        .borrow()
        .set_tool(owner, owner.raw(), true, None, text);
}

/// Sets a region tooltip on `owner`, keyed by `slot` so a widget can update the
/// same tool as the pointer moves.
pub(crate) fn set_region_tooltip(owner: Hwnd, slot: usize, rect: Rect, text: &str) {
    let Some(state) = shared_for(owner) else {
        return;
    };
    state
        .borrow()
        .set_tool(owner, slot, false, Some(rect), text);
}

/// Removes every tooltip `owner` added to its top-level window's shared
/// tooltip. Called when a widget is dropped, before its window is destroyed.
pub(crate) fn forget_widget(owner: Hwnd) {
    let root = sys::tooltip::root_window(owner);
    if root.is_null() {
        return;
    }
    let state = TOOLTIPS.with(|map| map.borrow().get(&root.raw()).cloned());
    if let Some(state) = state {
        state.borrow().remove_widget(owner);
    }
}

/// Drops the shared tooltip of a top-level window. Called from
/// `WM_NCDESTROY`, after which the owned tooltip window is already gone.
pub(crate) fn forget_window(root: Hwnd) {
    let state = TOOLTIPS.with(|map| map.borrow_mut().remove(&root.raw()));
    if let Some(state) = state {
        let state = state.borrow();
        registry::unregister(state.hwnd);
        theme::unregister_themed(state.hwnd);
        sys::window::destroy(state.hwnd);
    }
}

/// The shared tooltip's handle for `root`, for tests.
#[cfg(test)]
pub(crate) fn shared_hwnd(root: Hwnd) -> Option<Hwnd> {
    TOOLTIPS.with(|map| {
        map.borrow()
            .get(&root.raw())
            .map(|state| state.borrow().hwnd)
    })
}

/// The shared tooltip of `owner`'s top-level window, creating it on first use.
fn shared_for(owner: Hwnd) -> Option<Rc<RefCell<TooltipWindow>>> {
    shared(sys::tooltip::root_window(owner))
}

fn shared(root: Hwnd) -> Option<Rc<RefCell<TooltipWindow>>> {
    if root.is_null() {
        return None;
    }
    if let Some(existing) = TOOLTIPS.with(|map| map.borrow().get(&root.raw()).cloned()) {
        return Some(existing);
    }
    let hwnd = sys::tooltip::create(root).ok()?;
    let theme = theme::window_theme(root);
    let state = Rc::new(RefCell::new(TooltipWindow {
        hwnd,
        dark: Cell::new(theme.is_dark),
        theme: RefCell::new(theme),
        font: RefCell::new(None),
        tools: RefCell::new(HashMap::new()),
    }));
    // Own the dark custom draw: the tooltip sends `NM_CUSTOMDRAW` to its
    // parent, which offers it to the registry before the application sees it.
    registry::register(hwnd, Rc::clone(&state) as Rc<RefCell<dyn ControlEvents>>);
    // A DPI change resizes the tooltip but keeps the old font, so the subclass
    // recreates it at the new DPI (`sys::tooltip::subclass_dpi`).
    sys::tooltip::subclass_dpi(hwnd);
    // Re-theme live with the top-level window. The weak handle keeps the
    // tooltip from outliving its window's registry entry.
    let weak = Rc::downgrade(&state);
    theme::register_themed(
        root,
        hwnd,
        Rc::new(move |applied| {
            if let Some(state) = weak.upgrade()
                && let Ok(state) = state.try_borrow()
            {
                state.apply(applied);
            }
        }),
    );
    // `apply` also installs the sizing font (and re-installs it after the
    // theme, which resets it).
    state.borrow().apply(&theme);
    TOOLTIPS.with(|map| map.borrow_mut().insert(root.raw(), Rc::clone(&state)));
    Some(state)
}

/// Recreates the shared tooltip's font at the new DPI. Called from the
/// tooltip's subclass procedure (see `sys::tooltip::subclass_dpi`).
pub(crate) fn reapply_dpi(tooltip: Hwnd) {
    let state = TOOLTIPS.with(|map| {
        map.borrow()
            .values()
            .find(|state| state.borrow().hwnd == tooltip)
            .cloned()
    });
    if let Some(state) = state
        && let Ok(state) = state.try_borrow()
    {
        state.apply_font();
    }
}

/// The text inset the tooltip is painted with, in device pixels.
fn padding(dpi: u32) -> i32 {
    dip(5.0).to_px(dpi).value()
}

/// The horizontal inset applied to the paint: the full `pad` only while it
/// still leaves the whole `text` visible. Theme tooltips reserve that padding to
/// different degrees — some add none at all — and the text must never be clipped
/// to keep padding that the window does not actually have.
fn text_inset(window: i32, text: i32, pad: i32) -> i32 {
    if window >= text + pad * 2 { pad } else { 0 }
}

/// The width past which a long tip wraps, in device pixels.
fn max_tip_width(dpi: u32) -> i32 {
    dip(420.0).to_px(dpi).value()
}

/// The NUL-terminated UTF-16 encoding of `text`.
fn wide(text: &str) -> Box<[u16]> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}
