#![forbid(unsafe_code)]

//! A native `EDIT` control: single-line, multi-line, password and search
//! fields.
//!
//! The edit is the real Win32 `EDIT` class, so accessibility and IME come for
//! free. Its text colours come from the window's central `WM_CTLCOLOREDIT`
//! answer; a multi-line edit's scroll bar is darkened with `DarkMode_Explorer`,
//! single-line chrome with `DarkMode_CFD`.
//!
//! ```no_run
//! # use win32ui::prelude::*;
//! # enum Msg { Search(String) }
//! # fn build(ui: &mut Ui<Msg>) -> win32ui::Result<()> {
//! Edit::single_line(ui)?.cue("Search mail").on_change(|t| Some(Msg::Search(t.to_owned())));
//! # Ok(())
//! # }
//! ```

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::control::{AsControl, Control, ControlExt, HasText};
use crate::controls::edit_events::{self, EditEvents};
use crate::controls::edit_text::{from_native, to_native};
use crate::controls::{create_child, next_id, registry, style};
use crate::error::Result;
use crate::gdi::Font;
use crate::geometry::Rect;
use crate::message::Message;
use crate::sys;
use crate::sys::edit as sys_edit;
use crate::theme::{Theme, Themed};
use crate::units::dip;

/// A text-entry field over a native `EDIT`.
///
/// Build one with [`Edit::single_line`], [`Edit::multi_line`] or
/// [`Edit::password`], then chain the builders and event mappers. It holds the
/// app's `Msg` type because its events are mapped to it through closures.
pub struct Edit<M> {
    // The Enter-submit subclass first: struct fields drop in declaration
    // order, so this removes the subclass before `control` destroys the HWND.
    _submit: Option<sys_edit::ReturnSubclass>,
    control: Control,
    events: Rc<RefCell<EditEvents<M>>>,
    sink: Ui<M>,
    multiline: bool,
}

impl<M: 'static> Edit<M> {
    /// Creates a single-line edit, adopting `ui`'s theme.
    pub fn single_line(ui: &mut Ui<M>) -> Result<Edit<M>> {
        Edit::create(ui, sys_edit::STYLE_AUTOHSCROLL, 1, false)
    }

    /// Creates a multi-line edit that wraps by default, adopting `ui`'s theme.
    pub fn multi_line(ui: &mut Ui<M>) -> Result<Edit<M>> {
        let style = sys_edit::STYLE_MULTILINE | sys_edit::STYLE_AUTOVSCROLL | style::WS_VSCROLL;
        Edit::create(ui, style, 4, true)
    }

    /// Creates a password edit: the typed characters are masked with `ES_PASSWORD`.
    pub fn password(ui: &mut Ui<M>) -> Result<Edit<M>> {
        Edit::create(
            ui,
            sys_edit::STYLE_PASSWORD | sys_edit::STYLE_AUTOHSCROLL,
            1,
            false,
        )
    }

    fn create(ui: &mut Ui<M>, extra_style: u32, lines: i32, multiline: bool) -> Result<Edit<M>> {
        let dpi = ui.dpi();
        let parent = ui.hwnd();
        let style = style::WS_CHILD
            | style::WS_VISIBLE
            | style::WS_TABSTOP
            | style::WS_BORDER
            | extra_style;
        let ex_style = if multiline {
            style::WS_EX_CLIENTEDGE
        } else {
            0
        };
        let hwnd = create_child(
            "Edit",
            "EDIT",
            parent,
            style,
            ex_style,
            next_id(),
            Rect::default(),
        )?;

        // For multi-line edits, toggle the native client edge based on theme.
        // In dark mode, keep only the thin WS_BORDER to match single-line edits.
        if multiline {
            sys::client_edge::set(hwnd, !ui.theme().is_dark);
        }

        // Size the natural height from the UI font, so a layout that does not
        // size the widget still gets a sensible height at any DPI.
        let font = Font::system_ui(dpi)?;
        let line = font.pixel_height().max(1);
        let padding = dip(10.0).to_px(dpi).value();
        let bounds = Rect::new(0, 0, 0, line * lines.max(1) + padding);
        sys::window::move_window(hwnd, bounds);

        let mut edit = Edit {
            _submit: None,
            control: Control::own(hwnd, bounds),
            events: Rc::new(RefCell::new(EditEvents::new())),
            sink: ui.clone(),
            multiline,
        };
        edit.set_font(font);
        edit.apply_theme(&ui.theme());
        edit.register_mapper();

        crate::theme::register_themed(
            parent,
            hwnd,
            Rc::new(move |applied| {
                sys::apply_native_theme(hwnd, edit_native_kind(multiline), applied.is_dark);
                if multiline {
                    sys::client_edge::set(hwnd, !applied.is_dark);
                }
                sys::window::invalidate(hwnd);
            }),
        );

        // Enter only means "submit" on a single-line edit; on a multi-line edit
        // it inserts a newline, so no subclass is installed.
        if !multiline {
            let events = Rc::clone(&edit.events);
            let sink = edit.sink.clone();
            edit._submit = sys_edit::ReturnSubclass::install(
                hwnd,
                Box::new(move || {
                    let mapped = events.borrow().on_submit.as_ref().and_then(|f| f());
                    if let Some(msg) = mapped {
                        sink.emit(msg);
                    }
                }),
            );
        }
        Ok(edit)
    }

    /// Maps a text change (`EN_CHANGE`) to a message, given the new text.
    pub fn on_change(self, f: impl Fn(&str) -> Option<M> + 'static) -> Edit<M> {
        self.events.borrow_mut().on_change = Some(Box::new(f));
        self
    }

    /// Maps Enter on a single-line edit to a message.
    pub fn on_submit(self, f: impl Fn() -> Option<M> + 'static) -> Edit<M> {
        self.events.borrow_mut().on_submit = Some(Box::new(f));
        self
    }

    /// Maps a focus gain or loss (`EN_SETFOCUS`/`EN_KILLFOCUS`) to a message.
    pub fn on_focus(self, f: impl Fn(bool) -> Option<M> + 'static) -> Edit<M> {
        self.events.borrow_mut().on_focus = Some(Box::new(f));
        self
    }

    /// Shows `text` as a placeholder while the edit is empty. Chainable.
    pub fn cue(self, text: &str) -> Edit<M> {
        self.set_cue(text);
        self
    }

    /// Sets read-only mode. Chainable.
    pub fn read_only(self, read_only: bool) -> Edit<M> {
        self.set_read_only(read_only);
        self
    }

    /// Restricts input to digits (`ES_NUMBER`). Chainable.
    pub fn number_only(self, number_only: bool) -> Edit<M> {
        self.set_number_only(number_only);
        self
    }

    /// Limits the text to `length` characters. Chainable.
    pub fn max_length(self, length: usize) -> Edit<M> {
        self.set_max_length(length);
        self
    }

    /// Turns word wrap on or off on a multi-line edit. Chainable.
    pub fn word_wrap(self, wrap: bool) -> Edit<M> {
        self.set_word_wrap(wrap);
        self
    }

    /// Shows `text` as a placeholder while the edit is empty.
    pub fn set_cue(&self, text: &str) {
        sys_edit::set_cue(self.control.hwnd(), text, true);
    }

    /// Sets read-only mode.
    pub fn set_read_only(&self, read_only: bool) {
        sys_edit::set_read_only(self.control.hwnd(), read_only);
    }

    /// Restricts input to digits (`ES_NUMBER`).
    pub fn set_number_only(&self, number_only: bool) {
        sys_edit::set_number_only(self.control.hwnd(), number_only);
    }

    /// Limits the text to `length` characters.
    pub fn set_max_length(&self, length: usize) {
        sys_edit::limit_text(self.control.hwnd(), length);
    }

    /// Turns word wrap on or off on a multi-line edit.
    pub fn set_word_wrap(&self, wrap: bool) {
        sys_edit::set_word_wrap(self.control.hwnd(), wrap);
    }

    /// Selects the whole text.
    pub fn select_all(&self) {
        sys_edit::select_all(self.control.hwnd());
    }

    /// The current selection, in UTF-16 code units.
    pub fn selection(&self) -> Range<usize> {
        sys_edit::selection(self.control.hwnd())
    }

    /// Selects `selection`.
    pub fn set_selection(&self, selection: Range<usize>) {
        sys_edit::set_selection(self.control.hwnd(), selection);
    }

    /// Replaces the current selection (or inserts at the caret) with `text`.
    pub fn replace_selection(&self, text: &str) {
        sys_edit::replace_selection(self.control.hwnd(), &to_native(text));
    }

    /// Whether this is a multi-line edit.
    pub fn is_multiline(&self) -> bool {
        self.multiline
    }

    /// Registers the `WM_COMMAND` mapper that turns this edit's notifications
    /// into the app's `Msg`.
    fn register_mapper(&self) {
        let hwnd = self.control.hwnd();
        let events = Rc::clone(&self.events);
        let sink = self.sink.clone();
        let mapper: Rc<dyn Fn(&Message) -> bool> = Rc::new(move |message| {
            let mapped = edit_events::command_message(
                message,
                hwnd,
                || from_native(&sys_edit::text(hwnd)),
                &events.borrow(),
            );
            match mapped {
                None => false,
                Some(msg) => {
                    if let Some(msg) = msg {
                        sink.emit(msg);
                    }
                    true
                }
            }
        });
        registry::register_app_events(hwnd, mapper);
    }
}

/// The `SetWindowTheme` kind an edit of this shape needs: `DarkMode_Explorer`
/// darkens a multi-line edit's scroll bar, `DarkMode_CFD` the single-line
/// chrome.
fn edit_native_kind(multiline: bool) -> sys::NativeControlKind {
    if multiline {
        sys::NativeControlKind::Scrollable
    } else {
        sys::NativeControlKind::Button
    }
}

impl<M> AsControl for Edit<M> {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl<M> Themed for Edit<M> {
    fn apply_theme(&self, theme: &Theme) {
        // The text and background come from the central `WM_CTLCOLOREDIT`
        // answer; the native chrome (border, scroll bar) follows the theme.
        sys::apply_native_theme(
            self.control.hwnd(),
            edit_native_kind(self.multiline),
            theme.is_dark,
        );
        if self.multiline {
            sys::client_edge::set(self.control.hwnd(), !theme.is_dark);
        }
        sys::window::invalidate(self.control.hwnd());
    }
}

impl<M> Drop for Edit<M> {
    fn drop(&mut self) {
        registry::unregister_app_events(self.control.hwnd());
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

impl<M> HasText for Edit<M> {
    fn text(&self) -> String {
        from_native(&sys_edit::text(self.control.hwnd()))
    }

    fn set_text(&self, text: &str) {
        sys_edit::set_text(self.control.hwnd(), &to_native(text));
    }
}
