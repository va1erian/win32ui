#![forbid(unsafe_code)]

//! [`MaterialStatusBar`]: a status bar drawn on the window's bottom backdrop
//! band instead of in a child window.
//!
//! A child `StatusBar` window can't show the parent's DWM material, so this bar
//! lives in the top-level window's transparent Direct2D surface (see
//! [`Ui::material_status_bar_height`]). It is not a layout item: the app
//! reserves the band with a bottom margin, and the bar's parts and text are
//! mapped to it. On a window whose material cannot be shown, the band is painted
//! opaque from the same theme tokens, matching the child bar.

use std::rc::Rc;

use crate::app::Ui;
use crate::error::{Error, Result};
use crate::sys;

pub(crate) use super::core::MaterialStatusBarState;

/// A status bar painted on the window's bottom material band.
///
/// Build one with [`MaterialStatusBar::new`], reserve its height with
/// [`Ui::material_status_bar_height`] as a bottom layout margin, and drive it
/// with [`set_parts`](MaterialStatusBar::set_parts) /
/// [`set_text`](MaterialStatusBar::set_text) exactly like [`StatusBar`](crate::StatusBar).
pub struct MaterialStatusBar<M: 'static> {
    state: Rc<MaterialStatusBarState>,
    ui: Ui<M>,
}

impl<M: 'static> MaterialStatusBar<M> {
    /// Creates the bar and installs it on `ui`'s window. The window must use an
    /// extended title bar; the material only shows with an active
    /// [`Backdrop`](crate::Backdrop). Returns an error when DirectWrite is
    /// unavailable (use the child [`StatusBar`](crate::StatusBar) instead).
    pub fn new(ui: &mut Ui<M>) -> Result<MaterialStatusBar<M>> {
        if !crate::window::nc::is_extended(ui.hwnd()) {
            return Err(Error::WindowConfig(
                "the material status bar needs TitleBar::Extended",
            ));
        }
        let state = MaterialStatusBarState::new().ok_or(Error::Direct2d(
            "directwrite unavailable for the status bar",
        ))?;
        ui.install_material_status_bar(Rc::clone(&state));
        Ok(MaterialStatusBar {
            state,
            ui: ui.clone(),
        })
    }

    /// Splits the bar into parts whose right edges are given in client
    /// coordinates. Use a negative edge (e.g. `-1`) for "extend to the right".
    pub fn set_parts(&self, edges: &[i32]) {
        *self.state.parts.borrow_mut() = edges.to_vec();
        self.invalidate();
    }

    /// Sets the text shown in one part.
    pub fn set_text(&self, part: usize, text: &str) {
        {
            let mut texts = self.state.texts.borrow_mut();
            if texts.len() <= part {
                texts.resize(part + 1, String::new());
            }
            texts[part] = text.to_string();
        }
        self.state.rebuild();
        self.invalidate();
    }

    fn invalidate(&self) {
        sys::window::invalidate(self.ui.hwnd());
    }
}

/// The accessibility source of a [`MaterialStatusBar`]: a status bar whose
/// children are the parts' texts.
pub(crate) struct StatusBarAccess {
    state: Rc<MaterialStatusBarState>,
}

impl StatusBarAccess {
    pub(crate) fn new(state: Rc<MaterialStatusBarState>) -> StatusBarAccess {
        StatusBarAccess { state }
    }
}

impl crate::accessibility::registry::Source for StatusBarAccess {
    fn snapshot(&self) -> Option<crate::accessibility::Node> {
        use crate::accessibility::{Node, Role};
        let parts = self.state.parts.try_borrow().ok()?;
        let texts = self.state.texts.try_borrow().ok()?;
        let children = (0..parts.len())
            .map(|index| Node::new(Role::Text, texts.get(index).cloned().unwrap_or_default()));
        Some(Node::new(Role::StatusBar, "Status bar").children(children))
    }

    fn perform(&self, _path: &[usize], _action: crate::accessibility::Action) -> bool {
        false
    }
}
