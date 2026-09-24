#![forbid(unsafe_code)]

//! The strip-menu half of [`Core`](super::Core): install, layout, paint, open
//! and input for a menu drawn on the extended title strip.
//!
//! Split out so `core/mod.rs` stays under the file-size limit; the state these
//! methods read and write is still the private fields of `Core`.

use super::Core;
use crate::app::spec::MenuStripPlacement;
use crate::app::title_menu::{OpenAction, TitleBarMenu};
use crate::controls::menu::Menu;
use crate::d2d::{D2dCanvas, D2dSurface, RectF, Rgba};
use crate::geometry::Point;
use crate::message::{Key, Modifiers};
use crate::sys;

impl<M: 'static> Core<M> {
    /// Records whether the spec asked for the menu on the acrylic strip.
    pub(crate) fn set_menu_in_strip(&self, on: bool) {
        self.menu_in_strip.set(on);
    }

    /// Whether the spec asked for the menu on the acrylic strip.
    pub(crate) fn menu_in_strip(&self) -> bool {
        self.menu_in_strip.get()
    }

    /// Records where the strip menu is drawn (stacked row or inline).
    pub(crate) fn set_menu_strip_placement(&self, placement: MenuStripPlacement) {
        self.menu_strip_placement.set(placement);
    }

    /// Where the strip menu is drawn.
    pub(crate) fn menu_strip_placement(&self) -> MenuStripPlacement {
        self.menu_strip_placement.get()
    }

    /// Installs the strip menu, publishes its clickable row for hit-testing and
    /// re-extends the frame over the now-taller strip.
    pub(crate) fn install_title_menu(&self, menu: TitleBarMenu<M>) {
        *self.title_menu.borrow_mut() = Some(menu);
        self.refresh_menu_strip();
    }

    /// Whether a strip menu is active.
    pub(crate) fn has_title_menu(&self) -> bool {
        self.title_menu.borrow().is_some()
    }

    /// Whether the window paints a material surface (the strip menu and/or the
    /// bottom status bar) and so must repaint its transparent Direct2D layer.
    pub(crate) fn has_material_surface(&self) -> bool {
        self.has_title_menu() || self.has_material_status_bar()
    }

    /// Recomputes the strip menu's row height and item rectangles for the
    /// window's current DPI and re-applies the extended frame.
    pub(crate) fn refresh_menu_strip(&self) {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() {
            return;
        }
        let dpi = sys::dpi::window_dpi(hwnd);
        let caption = sys::nc::caption_strip_height(hwnd);
        let title = sys::window::get_title(hwnd);
        let layout = {
            let menu = self.title_menu.borrow();
            let Some(menu) = menu.as_ref() else {
                return;
            };
            let row = menu.relayout(dpi, caption, &title);
            (row, menu.layout_rects())
        };
        crate::window::nc::set_menu_strip(hwnd, layout.0, layout.1);
        sys::nc::apply_extended_frame(hwnd);
    }

    /// The index of the strip menu item under `point` (client coordinates), if
    /// any.
    pub(crate) fn title_menu_hit(&self, point: Point) -> Option<usize> {
        self.title_menu
            .borrow()
            .as_ref()
            .and_then(|menu| menu.hit(point))
    }

    /// Sets (or clears) the strip menu's hovered item.
    pub(crate) fn title_menu_set_hover(&self, index: Option<usize>) {
        if let Some(menu) = self.title_menu.borrow().as_ref() {
            menu.set_hover(index);
        }
    }

    /// Sets (or clears) the strip menu's pressed item.
    pub(crate) fn title_menu_set_pressed(&self, index: Option<usize>) {
        if let Some(menu) = self.title_menu.borrow().as_ref() {
            menu.set_pressed(index);
        }
    }

    /// Paints the strip menu onto the window's transparent Direct2D surface.
    /// Returns whether a strip menu is active (and the surface could draw).
    pub(crate) fn try_paint_material(&self) -> bool {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() || !self.has_material_surface() {
            return false;
        }
        let mut slot = self.strip_surface.borrow_mut();
        if slot.is_none() {
            *slot = D2dSurface::transparent(hwnd).ok();
        }
        let Some(surface) = slot.as_ref() else {
            return false;
        };
        let client = sys::window::client_rect(hwnd);
        surface.resize(client.width(), client.height());
        let Ok(mut canvas) = surface.begin_draw() else {
            return false;
        };
        self.paint_material(&mut canvas);
        let _ = canvas.end_draw();
        true
    }

    /// Resizes the material surface (call on `WM_SIZE`). The render target is
    /// dropped so the next frame rebuilds it at the new size, which keeps the
    /// surface correct across maximize/minimize/restore.
    pub(crate) fn resize_strip(&self, width: i32, height: i32) {
        if let Some(surface) = self.strip_surface.borrow().as_ref() {
            surface.resize(width, height);
            surface.discard_target();
        }
    }

    /// Draws the whole client: the top strip and the bottom status band are
    /// transparent (so DWM's material shows and their content is drawn over
    /// it), and the content between them is filled with the opaque theme
    /// background. Direct2D's `EndDraw` presents the whole surface, so every
    /// pixel (including the content behind the child controls) must be painted.
    fn paint_material(&self, canvas: &mut D2dCanvas) {
        let hwnd = self.hwnd.get();
        let dpi = sys::dpi::window_dpi(hwnd);
        let theme = self.theme.get();
        let client = sys::window::client_rect(hwnd);
        let scale = dpi as f32 / 96.0;
        let width = client.width() as f32 / scale;
        let strip = crate::window::nc::strip_height(hwnd);
        let band = crate::window::nc::status_bar(hwnd);
        canvas.clear_rgba(Rgba::TRANSPARENT);
        let content_bottom = (client.height() - band).max(strip);
        if content_bottom > strip {
            canvas.fill_rect_rgba(
                RectF::new(
                    0.0,
                    strip as f32 / scale,
                    width,
                    content_bottom as f32 / scale,
                ),
                Rgba::from(theme.background),
            );
        }
        {
            let menu = self.title_menu.borrow();
            if let Some(menu) = menu.as_ref() {
                menu.paint(canvas, dpi, &theme);
            }
        }
        self.paint_material_status_bar(canvas, dpi, &theme);
    }

    /// Opens the `index`-th strip menu item: raises a plain item's message, or
    /// runs the submenu as a native popup anchored below the item.
    pub(crate) fn open_title_menu(&self, index: usize) -> bool {
        let Some(action) = self
            .title_menu
            .borrow()
            .as_ref()
            .and_then(|menu| menu.open(index))
        else {
            return false;
        };
        match action {
            OpenAction::Command(msg) => {
                self.enqueue(msg);
                true
            }
            OpenAction::Popup(menu) => {
                self.popup_strip_menu(&menu, index);
                true
            }
        }
    }

    /// Tracks `menu` as a native popup, anchored under the `index`-th item.
    fn popup_strip_menu(&self, menu: &Menu<M>, index: usize) {
        let hwnd = self.hwnd.get();
        let Some(at) = self.strip_item_screen_point(index) else {
            return;
        };
        let theme = self.theme.get();
        let handle = menu.build(false, theme.is_dark, theme.raised);
        let previous = self.set_popup(Some(menu.clone()));
        let command = sys::menu::track_popup(handle, hwnd, at);
        self.set_popup(previous);
        menu.destroy_handle();
        if let Some(action) = command.and_then(|id| menu.find_action(id)) {
            self.enqueue(action());
        }
    }

    /// The screen point just below the `index`-th item, where its popup opens.
    fn strip_item_screen_point(&self, index: usize) -> Option<Point> {
        let hwnd = self.hwnd.get();
        let rect = self
            .title_menu
            .borrow()
            .as_ref()
            .and_then(|menu| menu.item_rect(index))?;
        Some(sys::window::client_to_screen(
            hwnd,
            Point::new(rect.left, rect.bottom),
        ))
    }

    /// Handles a keyboard message for the strip menu, returning whether it was
    /// consumed. Alt activates keyboard navigation; once active, the arrows
    /// move the focus, Enter/Down opens the focused menu and Escape closes it.
    /// A system key with Alt held opens the item whose mnemonic matches.
    pub(crate) fn title_menu_key(&self, key: Key, modifiers: Modifiers, system: bool) -> bool {
        let hwnd = self.hwnd.get();
        if hwnd.is_null() || !self.has_title_menu() {
            return false;
        }
        let mut open = None;
        let handled = {
            let menu = self.title_menu.borrow();
            let Some(menu) = menu.as_ref() else {
                return false;
            };
            let active = menu.is_active();
            match key {
                Key::MENU => {
                    menu.set_cues(true);
                    menu.set_active(true);
                    menu.set_focus(menu.first_enabled());
                    true
                }
                Key::ESCAPE => {
                    menu.set_active(false);
                    menu.set_cues(false);
                    true
                }
                Key::LEFT if active => {
                    menu.move_focus(-1);
                    true
                }
                Key::RIGHT if active => {
                    menu.move_focus(1);
                    true
                }
                Key::DOWN | Key::RETURN if active => {
                    open = menu.focused();
                    true
                }
                _ if system && modifiers.alt => {
                    open = key_char(key).and_then(|ch| menu.mnemonic_index(ch));
                    open.is_some()
                }
                _ => false,
            }
        };
        if let Some(index) = open {
            self.open_title_menu(index);
        }
        if handled {
            sys::window::invalidate(hwnd);
        }
        handled
    }

    /// Handles the Alt key being released: keyboard navigation and the mnemonic
    /// underlines are hidden again (an open popup keeps tracking).
    pub(crate) fn title_menu_on_alt_up(&self) {
        if let Some(menu) = self.title_menu.borrow().as_ref() {
            menu.set_active(false);
            menu.set_cues(false);
        }
        sys::window::invalidate(self.hwnd.get());
    }
}

/// The character a letter or digit virtual key produces, lowercased.
fn key_char(key: Key) -> Option<char> {
    match key.code() {
        0x41..=0x5A => Some((b'a' + (key.code() as u8 - 0x41)) as char),
        0x30..=0x39 => Some((b'0' + (key.code() as u8 - 0x30)) as char),
        _ => None,
    }
}
