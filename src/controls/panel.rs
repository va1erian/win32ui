#![forbid(unsafe_code)]

//! A container that owns a layout subtree of standard controls.
//!
//! [`Panel`] is the missing half of a scrollable form: standard controls
//! ([`Button`](crate::Button), [`Edit`](crate::Edit), …) always parent to the
//! top-level window, so they cannot scroll with a [`ScrollView`](crate::ScrollView).
//! A panel owns a plain child window, and the controls created through its
//! [`Panel::ui`] handle become *its* children, positioned by the panel's own
//! [`Layout`](crate::Layout) and clipped to its client area. Hand the panel to
//! [`ScrollView::set_content`](crate::ScrollView::set_content) and the whole
//! form follows the scroll offset.
//!
//! ```ignore
//! let panel = Panel::new(ui)?;
//! let mut form = panel.ui(ui);
//! let name = Edit::single_line(&mut form).cue("Name");
//! let save = Button::new(&mut form, "Save").on_click(|| Some(Msg::Save));
//! panel.set_layout(column![name, save].spacing(dip(8.0)));
//! let view = ScrollView::new(ui)?;
//! view.set_content(&panel);
//! view.set_content_height(dip(640.0));
//! ```

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::Layout;
use crate::app::Ui;
use crate::controls::control::{AsControl, Control};
use crate::controls::style;
use crate::error::Result;
use crate::geometry::Rect;
use crate::hwnd::Hwnd;
use crate::message::{LResult, Message};
use crate::sys;
use crate::theme::{Theme, Themed};
use crate::window::{Window, WindowClass, WindowExStyle, WindowHandler, WindowStyle};

/// The state a panel's window and the [`Panel`] handle share.
pub(crate) struct PanelShared {
    hwnd: Cell<Hwnd>,
    layout: RefCell<Option<Layout>>,
}

impl PanelShared {
    fn new() -> PanelShared {
        PanelShared {
            hwnd: Cell::new(Hwnd::NULL),
            layout: RefCell::new(None),
        }
    }

    /// Lays the panel's subtree out inside its current client area and moves
    /// every live leaf with a single batched `DeferWindowPos` move.
    fn relayout(&self) {
        let hwnd = self.hwnd.get();
        if !hwnd.is_alive() {
            return;
        }
        let Some(layout) = self.layout.borrow().clone() else {
            return;
        };
        let dpi = sys::dpi::window_dpi(hwnd);
        let client = sys::window::client_rect(hwnd);
        let placed: Vec<_> = layout
            .compute(client, dpi)
            .into_iter()
            .filter(|placed| placed.handle.hwnd().is_alive())
            .collect();
        let moves: Vec<(Hwnd, Rect)> = placed
            .iter()
            .map(|placed| (placed.handle.hwnd(), placed.rect))
            .collect();
        for placed in &placed {
            placed.handle.set_bounds(placed.rect);
        }
        sys::layout::apply(&moves);
    }
}

/// A child window that owns a layout subtree of standard controls.
///
/// Build one with [`Panel::new`], create its children through [`Panel::ui`],
/// install their positions with [`Panel::set_layout`], then hand it to
/// [`ScrollView::set_content`](crate::ScrollView::set_content). Through
/// [`AsControl`]/[`ControlExt`] it can be placed in an outer layout like any
/// other widget.
pub struct Panel {
    shared: Rc<PanelShared>,
    /// Owns the child window (and its class registration); dropping it
    /// destroys the window, which destroys the children it parents.
    _window: Window,
    control: Control,
}

impl Panel {
    /// Creates an empty panel as a child of the window behind `ui`, adopting
    /// `ui`'s theme. If `ui` is itself scoped to another panel, the new panel
    /// nests inside it.
    pub fn new<M: 'static>(ui: &mut Ui<M>) -> Result<Panel> {
        crate::sys::control::init_common_controls()?;
        let background = ui.theme().background;
        let shared = Rc::new(PanelShared::new());
        let class = WindowClass::register("win32ui.panel", background)?;
        let window = Window::create(
            class,
            Some(ui.hwnd()),
            WindowStyle::new()
                .child()
                .visible()
                .clip_children()
                .with(style::WS_CLIPSIBLINGS),
            WindowExStyle::new().control_parent(),
            Rect::default(),
            "panel",
            PanelHandler {
                shared: Rc::clone(&shared),
            },
        )?;
        let hwnd = window.hwnd();
        shared.hwnd.set(hwnd);
        // The panel's own `WM_CTLCOLOR*` answers must come from its theme, so a
        // button or edit parented to it is not drawn against the window color.
        crate::theme::set_window_theme(hwnd, ui.theme());

        let control = Control::borrowed(hwnd, Rect::default());
        let panel = Panel {
            shared,
            _window: window,
            control,
        };
        theme_callback(ui.hwnd(), hwnd);
        Ok(panel)
    }

    /// A handle scoped to this panel: widgets created through it become the
    /// panel's children. Keep it alive only while building the form.
    pub fn ui<M: 'static>(&self, ui: &Ui<M>) -> Ui<M> {
        ui.with_parent(self.control.hwnd())
    }

    /// Installs the panel's layout subtree and lays it out immediately. The
    /// tree is re-run whenever the panel is resized (for example by scrolling).
    ///
    /// Only widgets and nested [`Layout`]s belong in a panel's tree; a
    /// [`Tabs`](crate::Tabs) or [`Split`](crate::Split) node must be hosted by
    /// the top-level window instead.
    pub fn set_layout(&self, layout: Layout) {
        *self.shared.layout.borrow_mut() = Some(layout);
        self.shared.relayout();
    }

    /// Lays the panel's subtree out again, after changing a dependency such as
    /// a child's visibility.
    pub fn relayout(&self) {
        self.shared.relayout();
    }

    /// The panel's client area, in device pixels.
    pub fn client_rect(&self) -> Rect {
        sys::window::client_rect(self.control.hwnd())
    }
}

impl AsControl for Panel {
    fn control(&self) -> &Control {
        &self.control
    }
}

impl Themed for Panel {
    fn apply_theme(&self, theme: &Theme) {
        crate::theme::set_window_theme(self.control.hwnd(), *theme);
        sys::set_class_background(self.control.hwnd(), theme.background);
        sys::window::invalidate(self.control.hwnd());
    }
}

impl Drop for Panel {
    fn drop(&mut self) {
        crate::theme::unregister_themed(self.control.hwnd());
    }
}

/// Registers the panel's re-theme callback against the top-level window.
fn theme_callback(parent: Hwnd, hwnd: Hwnd) {
    crate::theme::register_themed(
        parent,
        hwnd,
        Rc::new(move |applied| {
            crate::theme::set_window_theme(hwnd, *applied);
            sys::set_class_background(hwnd, applied.background);
            sys::window::invalidate(hwnd);
        }),
    );
}

/// The panel's window procedure: re-run its subtree when it is resized.
struct PanelHandler {
    shared: Rc<PanelShared>,
}

impl WindowHandler for PanelHandler {
    fn message(&self, _window: &Window, message: Message) -> Option<LResult> {
        match message {
            Message::Size { .. } => {
                self.shared.relayout();
                Some(0)
            }
            _ => None,
        }
    }
}
