#![forbid(unsafe_code)]

//! An owner-drawn toolbar: a child window that paints a row of buttons itself
//! (background, hover/pressed states, icon and label) and maps clicks to the
//! app's `Msg` through each item's `on_click` closure.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::Ui;
use crate::color::Color;
use crate::controls::control::{AsControl, Control};
use crate::error::Result;
use crate::gdi::{Bitmap, Font, Paint, TextFormat};
use crate::geometry::{Point, Rect};
use crate::message::{Message, MouseButton};
use crate::sys;
use crate::theme::Theme;
use crate::units::dip;
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

/// One toolbar button.
pub struct ToolbarItem<M> {
    label: String,
    icon: Option<Bitmap>,
    on_click: Option<Box<dyn Fn() -> Option<M>>>,
}

impl<M> ToolbarItem<M> {
    /// A button with a label and no icon.
    pub fn new(label: impl Into<String>) -> ToolbarItem<M> {
        ToolbarItem {
            label: label.into(),
            icon: None,
            on_click: None,
        }
    }

    /// Adds an icon.
    pub fn with_icon(mut self, icon: Bitmap) -> ToolbarItem<M> {
        self.icon = Some(icon);
        self
    }

    /// Maps a click on this button to an app message.
    pub fn on_click(mut self, f: impl Fn() -> Option<M> + 'static) -> ToolbarItem<M> {
        self.on_click = Some(Box::new(f));
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

struct ToolbarState<M> {
    items: Vec<ToolbarItem<M>>,
    theme: ToolbarTheme,
    font: Font,
    dpi: u32,
    widths: Vec<i32>,
    rects: Vec<Rect>,
    bounds: Rect,
    height: i32,
    hover: Option<usize>,
    pressed: Option<usize>,
    ui: Ui<M>,
}

impl<M> ToolbarState<M> {
    fn new(items: Vec<ToolbarItem<M>>, theme: ToolbarTheme, dpi: u32, ui: Ui<M>) -> Result<Self> {
        let font = Font::system_ui(dpi)?;
        let padding = dip(10.0).to_px(dpi).value();
        let icon = dip(16.0).to_px(dpi).value();
        let gap = dip(6.0).to_px(dpi).value();
        let height = font.pixel_height() + dip(12.0).to_px(dpi).value();

        let widths = items
            .iter()
            .map(|item| {
                let text = sys::gdi::measure_text(font.raw(), &item.label).width;
                let icon_width = if item.icon.is_some() { icon + gap } else { 0 };
                (text + icon_width + padding * 2).max(dip(32.0).to_px(dpi).value())
            })
            .collect();

        let mut state = ToolbarState {
            items,
            theme,
            font,
            dpi,
            widths,
            rects: Vec::new(),
            bounds: Rect::new(0, 0, 0, height),
            height,
            hover: None,
            pressed: None,
            ui,
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
        let radius = dip(4.0).to_px(self.dpi).value();
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
            let button = rect.shrink(dip(2.0).to_px(self.dpi).value());
            canvas.round_rect(button, radius, background, None);

            let mut text_rect = button.shrink(dip(6.0).to_px(self.dpi).value());
            if let Some(icon) = &self.items[index].icon {
                let icon_size = icon.size();
                let inset = dip(6.0).to_px(self.dpi).value();
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

struct ToolbarHandler<M> {
    state: Rc<RefCell<ToolbarState<M>>>,
}

impl<M: 'static> ToolbarHandler<M> {
    fn fire_click(&self, index: usize) {
        let state = self.state.borrow();
        let Some(item) = state.items.get(index) else {
            return;
        };
        let Some(on_click) = &item.on_click else {
            return;
        };
        if let Some(msg) = on_click() {
            state.ui.emit(msg);
        }
    }
}

impl<M: 'static> WindowHandler for ToolbarHandler<M> {
    fn message(&self, window: &Window, message: Message) -> Option<isize> {
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
                let clicked = {
                    let mut state = self.state.borrow_mut();
                    let pressed = state.pressed.take();
                    let hit = state.hit_test(x, y);
                    match (pressed, hit) {
                        (Some(pressed), Some(hit)) if pressed == hit => Some(hit),
                        _ => None,
                    }
                };
                if let Some(index) = clicked {
                    self.fire_click(index);
                }
                window.invalidate();
                Some(0)
            }
            _ => None,
        }
    }
}

/// An owner-drawn toolbar control.
pub struct Toolbar<M> {
    window: Window,
    control: Control,
    state: Rc<RefCell<ToolbarState<M>>>,
}

impl<M: 'static> Toolbar<M> {
    /// Creates the toolbar as a child of the window behind `ui`.
    pub fn new(
        ui: &mut Ui<M>,
        items: Vec<ToolbarItem<M>>,
        theme: ToolbarTheme,
    ) -> Result<Toolbar<M>> {
        let state = Rc::new(RefCell::new(ToolbarState::new(
            items,
            theme,
            ui.dpi(),
            ui.clone(),
        )?));
        let class = WindowClass::register("win32ui.toolbar", theme.background)?;
        let handler = ToolbarHandler {
            state: Rc::clone(&state),
        };
        let bounds = Rect::new(0, 0, 0, state.borrow().height());
        let window = Window::create(
            class,
            Some(ui.hwnd()),
            WindowStyle::new().child().visible(),
            WindowExStyle::new(),
            bounds,
            "",
            handler,
        )?;
        let control = Control::borrowed(window.hwnd(), bounds);
        Ok(Toolbar {
            window,
            control,
            state,
        })
    }

    /// The toolbar's natural height.
    pub fn height(&self) -> i32 {
        self.state.borrow().height()
    }

    /// Schedules a repaint.
    pub fn invalidate(&self) {
        self.window.invalidate();
    }
}

impl<M> AsControl for Toolbar<M> {
    fn control(&self) -> &Control {
        &self.control
    }
}
