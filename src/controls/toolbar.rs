#![forbid(unsafe_code)]

//! An owner-drawn toolbar: a child window that paints a row of buttons itself
//! (background, hover/pressed states, icon and label) and reports clicks to
//! its parent as ordinary `WM_COMMAND`s.

use std::cell::RefCell;
use std::rc::Rc;

use crate::color::Color;
use crate::error::Result;
use crate::gdi::{Bitmap, Font, Paint, TextFormat};
use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;
use crate::message::{Message, MouseButton};
use crate::sys;
use crate::theme::Theme;
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle, dpi_scale};

const WM_COMMAND: u32 = 0x0111;
const BN_CLICKED: u16 = 0;

/// One toolbar button.
pub struct ToolbarItem {
    /// Identifier reported back through `WM_COMMAND`.
    pub id: u16,
    /// Button label.
    pub label: String,
    /// Optional icon, drawn to the left of the label.
    pub icon: Option<Bitmap>,
}

impl ToolbarItem {
    /// A button with a label and no icon.
    pub fn new(id: u16, label: impl Into<String>) -> ToolbarItem {
        ToolbarItem {
            id,
            label: label.into(),
            icon: None,
        }
    }

    /// Adds an icon.
    pub fn with_icon(mut self, icon: Bitmap) -> ToolbarItem {
        self.icon = Some(icon);
        self
    }
}

/// Colours for the toolbar.
#[derive(Clone, Copy, Debug)]
pub struct ToolbarTheme {
    /// Toolbar background.
    pub background: Color,
    /// Idle button background.
    pub button: Color,
    /// Hovered button background.
    pub button_hover: Color,
    /// Pressed button background.
    pub button_pressed: Color,
    /// Button label colour.
    pub text: Color,
    /// Separator/border colour.
    pub border: Color,
}

impl ToolbarTheme {
    /// Derives a toolbar palette from the app [`Theme`].
    pub fn from_theme(theme: &Theme) -> ToolbarTheme {
        ToolbarTheme {
            background: theme.surface,
            button: theme.surface,
            button_hover: theme.surface.lerp(theme.accent, 0.22),
            button_pressed: theme.selection,
            text: theme.text,
            border: theme.border,
        }
    }
}

struct ToolbarState {
    parent: Hwnd,
    items: Vec<ToolbarItem>,
    theme: ToolbarTheme,
    font: Font,
    widths: Vec<i32>,
    rects: Vec<Rect>,
    bounds: Rect,
    height: i32,
    hover: Option<usize>,
    pressed: Option<usize>,
}

impl ToolbarState {
    fn new(parent: Hwnd, items: Vec<ToolbarItem>, theme: ToolbarTheme, dpi: u32) -> Result<Self> {
        let font = Font::system_ui(dpi)?;
        let padding = dpi_scale(10, dpi);
        let icon = dpi_scale(16, dpi);
        let gap = dpi_scale(6, dpi);
        let height = font.pixel_height() + dpi_scale(12, dpi);

        let widths = items
            .iter()
            .map(|item| {
                let text = sys::gdi::measure_text(font.raw(), &item.label).width;
                let icon_width = if item.icon.is_some() { icon + gap } else { 0 };
                (text + icon_width + padding * 2).max(dpi_scale(32, dpi))
            })
            .collect();

        let mut state = ToolbarState {
            parent,
            items,
            theme,
            font,
            widths,
            rects: Vec::new(),
            bounds: Rect::new(0, 0, 0, height),
            height,
            hover: None,
            pressed: None,
        };
        state.layout(0);
        Ok(state)
    }

    fn layout(&mut self, width: i32) {
        self.bounds = Rect::new(0, 0, width.max(0), self.height);
        self.rects = Vec::with_capacity(self.items.len());
        let mut x = 0;
        for width in &self.widths {
            self.rects.push(Rect::new(x, 0, x + width, self.height));
            x += width;
        }
    }

    fn height(&self) -> i32 {
        self.height
    }

    fn hit_test(&self, x: i32, y: i32) -> Option<usize> {
        let point = Point::new(x, y);
        self.rects.iter().position(|rect| rect.contains(point))
    }

    fn draw(&self, canvas: &crate::gdi::Canvas) {
        canvas.fill_rect(self.bounds, self.theme.background);
        let radius = dpi_scale(4, 96);
        for (index, rect) in self.rects.iter().enumerate() {
            if index >= self.items.len() {
                break;
            }
            let background = if self.pressed == Some(index) {
                self.theme.button_pressed
            } else if self.hover == Some(index) {
                self.theme.button_hover
            } else {
                self.theme.button
            };
            let button = rect.shrink(dpi_scale(2, 96));
            canvas.round_rect(button, radius, background, None);

            let mut text_rect = button.shrink(dpi_scale(6, 96));
            if let Some(icon) = &self.items[index].icon {
                let icon_size = icon.size();
                let inset = dpi_scale(6, 96);
                let top = button.top + (button.height() - icon_size.height) / 2;
                let icon_rect = Rect::new(
                    button.left + inset,
                    top,
                    button.left + inset + icon_size.width,
                    top + icon_size.height,
                );
                canvas.draw_bitmap(icon, icon_rect);
                text_rect.left = icon_rect.right + inset;
            }
            canvas.with_font(&self.font, |canvas| {
                canvas.draw_text(
                    text_rect,
                    &self.items[index].label,
                    self.theme.text,
                    TextFormat::left().single_line().vcenter().end_ellipsis(),
                );
            });
        }
        canvas.fill_rect(
            Rect::new(
                0,
                self.bounds.bottom - 1,
                self.bounds.right,
                self.bounds.bottom,
            ),
            self.theme.border,
        );
    }
}

struct ToolbarHandler {
    state: Rc<RefCell<ToolbarState>>,
}

impl WindowHandler for ToolbarHandler {
    fn message(&mut self, window: &Window, message: Message) -> Option<isize> {
        match message {
            Message::Paint => {
                if let Some(paint) = Paint::begin(window.hwnd()) {
                    let state = self.state.borrow();
                    state.draw(paint.canvas());
                }
                Some(0)
            }
            Message::Size { width, .. } => {
                self.state.borrow_mut().layout(width);
                Some(0)
            }
            Message::MouseMove { x, y } => {
                let mut state = self.state.borrow_mut();
                let hover = state.hit_test(x, y);
                if hover != state.hover {
                    state.hover = hover;
                    drop(state);
                    window.invalidate();
                }
                Some(0)
            }
            Message::MouseDown {
                x,
                y,
                button: MouseButton::Left,
            } => {
                let hit = self.state.borrow().hit_test(x, y);
                self.state.borrow_mut().pressed = hit;
                window.invalidate();
                Some(0)
            }
            Message::MouseUp {
                x,
                y,
                button: MouseButton::Left,
            } => {
                let id = {
                    let mut state = self.state.borrow_mut();
                    let pressed = state.pressed.take();
                    let hit = state.hit_test(x, y);
                    match (pressed, hit) {
                        (Some(pressed), Some(hit)) if pressed == hit => {
                            state.items.get(hit).map(|item| item.id)
                        }
                        _ => None,
                    }
                };
                if let Some(id) = id {
                    self.send_click(window.hwnd(), id);
                }
                window.invalidate();
                Some(0)
            }
            _ => None,
        }
    }
}

impl ToolbarHandler {
    fn send_click(&self, toolbar: Hwnd, id: u16) {
        let parent = self.state.borrow().parent;
        let wparam = id as usize | ((BN_CLICKED as usize) << 16);
        sys::window::send_message(parent, WM_COMMAND, wparam, toolbar.raw() as isize);
    }
}

/// An owner-drawn toolbar control.
pub struct Toolbar {
    window: Window,
    state: Rc<RefCell<ToolbarState>>,
}

impl Toolbar {
    /// Creates the toolbar as a child of `parent`.
    pub fn new(
        parent: Hwnd,
        items: Vec<ToolbarItem>,
        theme: ToolbarTheme,
        dpi: u32,
    ) -> Result<Toolbar> {
        let state = Rc::new(RefCell::new(ToolbarState::new(parent, items, theme, dpi)?));
        let class = WindowClass::register("emusic.toolbar", theme.background)?;
        let handler = ToolbarHandler {
            state: Rc::clone(&state),
        };
        let bounds = Rect::new(0, 0, 0, state.borrow().height());
        let window = Window::create(
            class,
            Some(parent),
            WindowStyle::new().child().visible(),
            WindowExStyle::new(),
            bounds,
            "",
            handler,
        )?;
        Ok(Toolbar { window, state })
    }

    /// The control handle.
    pub fn hwnd(&self) -> Hwnd {
        self.window.hwnd()
    }

    /// The toolbar's natural height.
    pub fn height(&self) -> i32 {
        self.state.borrow().height()
    }

    /// Moves/resizes the toolbar.
    pub fn set_bounds(&self, bounds: Rect) {
        self.window.set_bounds(bounds);
    }

    /// Schedules a repaint.
    pub fn invalidate(&self) {
        self.window.invalidate();
    }
}
