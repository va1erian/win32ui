#![forbid(unsafe_code)]

//! An owner-drawn status bar.
//!
//! The native `msctls_statusbar32` can't be given a dark palette (it exposes
//! no text-colour API), so this is a small custom child window that paints its
//! parts itself, matching the egui frontend.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::color::Color;
use crate::controls::control::{AsControl, Control};
use crate::error::Result;
use crate::gdi::{Font, Paint, TextFormat};
use crate::geometry::Rect;
use crate::message::Message;
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::units::dip;
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

/// Colours for the status bar.
#[derive(Clone, Copy, Debug)]
pub struct StatusBarTheme {
    /// Background.
    pub background: Color,
    /// Text.
    pub text: Color,
    /// Separator/border colour.
    pub border: Color,
}

impl StatusBarTheme {
    /// Derives a palette from the app [`Theme`]. Override any field after
    /// calling this for a custom look.
    pub fn from_theme(theme: &Theme) -> StatusBarTheme {
        StatusBarTheme {
            background: theme.surface,
            text: theme.text_secondary,
            border: theme.border,
        }
    }
}

struct StatusBarState {
    parts: Vec<i32>,
    texts: Vec<String>,
    theme: StatusBarTheme,
    font: Font,
    bounds: Rect,
}

impl StatusBarState {
    fn part_edges(&self) -> Vec<i32> {
        self.parts
            .iter()
            .map(|edge| if *edge < 0 { self.bounds.right } else { *edge })
            .collect()
    }

    fn draw(&self, canvas: &crate::gdi::Canvas) {
        canvas.fill_rect(self.bounds, self.theme.background);
        canvas.fill_rect(
            Rect::new(
                self.bounds.left,
                self.bounds.top,
                self.bounds.right,
                self.bounds.top + 1,
            ),
            self.theme.border,
        );

        let mut left = self.bounds.left;
        for (index, &right) in self.part_edges().iter().enumerate() {
            if index > 0 && right > left {
                canvas.fill_rect(
                    Rect::new(left, self.bounds.top + 3, left + 1, self.bounds.bottom - 3),
                    self.theme.border,
                );
            }
            let text = self.texts.get(index).map(String::as_str).unwrap_or("");
            if !text.is_empty() {
                let cell = Rect::new(left + 8, self.bounds.top, right - 4, self.bounds.bottom);
                canvas.with_font(&self.font, |canvas| {
                    canvas.draw_text(
                        cell,
                        text,
                        self.theme.text,
                        TextFormat::left()
                            .single_line()
                            .vcenter()
                            .end_ellipsis()
                            .no_prefix(),
                    );
                });
            }
            left = right;
        }
    }
}

struct StatusBarHandler {
    state: Rc<RefCell<StatusBarState>>,
}

impl WindowHandler for StatusBarHandler {
    fn message(&self, window: &Window, message: Message) -> Option<isize> {
        match message {
            Message::Paint => {
                if let Some(paint) = Paint::begin(window.hwnd()) {
                    self.state.borrow().draw(paint.canvas());
                }
                Some(0)
            }
            Message::Size { width, height } => {
                self.state.borrow_mut().bounds = Rect::new(0, 0, width, height);
                Some(0)
            }
            _ => None,
        }
    }
}

/// An owner-drawn status bar with parts and text.
pub struct StatusBar {
    window: Window,
    control: Control,
    state: Rc<RefCell<StatusBarState>>,
}

impl StatusBar {
    /// Creates the bar as a child of the window behind `ui`, adopting `ui`'s
    /// theme. Use [`Themed::apply_theme`] for a one-off override.
    pub fn new<M: 'static>(ui: &mut Ui<M>) -> Result<StatusBar> {
        let theme = StatusBarTheme::from_theme(&ui.theme());
        let dpi = ui.dpi();
        let font = Font::system_ui(dpi)?;
        let height = dip(22.0).to_px(dpi).value();
        let state = Rc::new(RefCell::new(StatusBarState {
            parts: vec![-1],
            texts: Vec::new(),
            theme,
            font,
            bounds: Rect::new(0, 0, 0, height),
        }));
        let class = WindowClass::register("win32ui.statusbar", theme.background)?;
        let handler = StatusBarHandler {
            state: Rc::clone(&state),
        };
        let window = Window::create(
            class,
            Some(ui.hwnd()),
            WindowStyle::new().child().visible(),
            WindowExStyle::new(),
            Rect::new(0, 0, 0, height),
            "",
            handler,
        )?;
        let control = Control::borrowed(window.hwnd(), Rect::new(0, 0, 0, height));
        {
            let weak = Rc::downgrade(&state);
            let hwnd = control.hwnd();
            let parent = ui.hwnd();
            crate::theme::register_themed(
                parent,
                hwnd,
                Rc::new(move |applied| {
                    if let Some(state) = weak.upgrade() {
                        state.borrow_mut().theme = StatusBarTheme::from_theme(applied);
                        sys::set_class_background(hwnd, applied.surface);
                        sys::window::invalidate(hwnd);
                    }
                }),
            );
        }
        Ok(StatusBar {
            window,
            control,
            state,
        })
    }

    /// Splits the bar into parts whose right edges are given in client
    /// coordinates. Use a negative edge (e.g. `-1`) for "extend to the right".
    pub fn set_parts(&self, edges: &[i32]) {
        self.state.borrow_mut().parts = edges.to_vec();
        self.window.invalidate();
    }

    /// Sets the text shown in one part.
    pub fn set_text(&self, part: usize, text: &str) {
        {
            let mut state = self.state.borrow_mut();
            if state.texts.len() <= part {
                state.texts.resize(part + 1, String::new());
            }
            state.texts[part] = text.to_string();
        }
        self.window.invalidate();
    }
}

impl AsControl for StatusBar {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl Themed for StatusBar {
    fn apply_theme(&self, theme: &Theme) {
        self.state.borrow_mut().theme = StatusBarTheme::from_theme(theme);
        sys::set_class_background(self.control.hwnd(), theme.surface);
        self.window.invalidate();
    }
}

impl Drop for StatusBar {
    fn drop(&mut self) {
        crate::theme::unregister_themed(self.control.hwnd());
    }
}
