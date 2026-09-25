#![forbid(unsafe_code)]

//! [`MaterialTopBar`]: an interactive bar drawn on the window's top backdrop
//! band instead of in a child window.
//!
//! Like the top strip menu and the bottom
//! [`MaterialStatusBar`](crate::MaterialStatusBar), the bar is not a child
//! window: the app installs one, reserves its band with
//! [`Ui::material_top_bar_height`](crate::Ui::material_top_bar_height), the
//! window's extended frame grows over the band, and the top-level transparent
//! Direct2D surface paints the items over the material. On a window whose
//! material cannot be shown the band is filled opaque from
//! [`Theme::surface`](crate::Theme::surface) instead, and the items paint with
//! the normal theme tokens.
//!
//! The bar's items are set as a list on each sync; painting and layout allocate
//! nothing per frame. A [`Native`](TopBarItem::native) item is a slot the app
//! fills with its own child control (an `Edit` search box), which keeps native
//! IME and accessibility. The app either hands the child to the slot with
//! [`TopBarItem::child`], so the bar keeps it positioned across resizes, or
//! positions it itself in the slot returned by
//! [`Ui::material_top_bar_slot`](crate::Ui::material_top_bar_slot). Either way
//! the direct child at the slot paints opaquely (GDI's zero alpha would let DWM
//! drop it over the material); a control nested in another child is not.

mod access;
mod input;
mod layout;
mod native;
mod paint;
mod state;

use std::ops::RangeInclusive;
use std::rc::Rc;

use crate::app::Ui;
use crate::controls::AsControl;
use crate::error::{Error, Result};
use crate::units::Dip;

pub(crate) use access::TopBarAccess;
pub(crate) use state::TopBarState;

/// The Segoe Fluent Icons / Segoe MDL2 Assets glyphs the bundled controls use.
///
/// The values are the documented codepoints of the Segoe Fluent Icons family
/// (play, pause and stop are the ones the issue names). Any other glyph can be
/// passed straight to [`TopBarItem::icon_button`] as a `char`.
pub struct Fluent;

impl Fluent {
    /// `E768`: play.
    pub const PLAY: char = '\u{E768}';
    /// `E769`: pause.
    pub const PAUSE: char = '\u{E769}';
    /// `E71A`: stop.
    pub const STOP: char = '\u{E71A}';
    /// `E892`: previous track.
    pub const PREVIOUS: char = '\u{E892}';
    /// `E893`: next track.
    pub const NEXT: char = '\u{E893}';
    /// `E8EE`: repeat.
    pub const REPEAT: char = '\u{E8EE}';
    /// `E8B1`: shuffle.
    pub const SHUFFLE: char = '\u{E8B1}';
}

/// How the top bar identifies an item across syncs, so a value can be updated
/// without rebuilding the list. On the app's side, a single `enum` mapped to
/// `TopBarId::new` usually reads better than loose numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TopBarId(u64);

impl TopBarId {
    /// A `TopBarId` that no item uses: a spacer carries it.
    pub const NONE: TopBarId = TopBarId(0);

    /// Creates an id from an app-chosen number.
    pub const fn new(value: u64) -> TopBarId {
        TopBarId(value)
    }

    /// The app-chosen number.
    pub const fn value(self) -> u64 {
        self.0
    }
}

impl From<u32> for TopBarId {
    fn from(value: u32) -> TopBarId {
        TopBarId(value as u64)
    }
}

impl From<u64> for TopBarId {
    fn from(value: u64) -> TopBarId {
        TopBarId(value)
    }
}

/// What one [`TopBarItem`] is.
pub(crate) enum TopBarSpec {
    /// An icon button that raises a click.
    Icon(char),
    /// An icon button with a checked state that raises a toggle.
    Toggle(char),
    /// A horizontal slider over `(min, max)`, starting at `value`.
    Slider { value: f64, min: f64, max: f64 },
    /// A text label.
    Label(String),
    /// A fixed or flexible gap.
    Spacer { fill: bool },
    /// A slot the app fills with its own child window.
    Native,
}

/// One item of a [`MaterialTopBar`], built with the constructors below and the
/// chaining setters.
pub struct TopBarItem {
    pub(crate) id: TopBarId,
    pub(crate) spec: TopBarSpec,
    pub(crate) tooltip: Option<String>,
    pub(crate) checked: bool,
    pub(crate) enabled: bool,
    pub(crate) width: Option<Dip>,
    pub(crate) expand: bool,
    pub(crate) height: Option<Dip>,
    pub(crate) child: Option<native::Child>,
}

impl TopBarItem {
    /// An icon button: `glyph` is a Segoe Fluent Icons codepoint
    /// (see [`Fluent`]).
    pub fn icon_button(id: impl Into<TopBarId>, glyph: char) -> TopBarItem {
        TopBarItem {
            id: id.into(),
            spec: TopBarSpec::Icon(glyph),
            tooltip: None,
            checked: false,
            enabled: true,
            width: None,
            expand: false,
            height: None,
            child: None,
        }
    }

    /// A toggle button with a checked state.
    pub fn toggle(id: impl Into<TopBarId>, glyph: char) -> TopBarItem {
        TopBarItem {
            id: id.into(),
            spec: TopBarSpec::Toggle(glyph),
            tooltip: None,
            checked: false,
            enabled: true,
            width: None,
            expand: false,
            height: None,
            child: None,
        }
    }

    /// A horizontal slider over `range`, starting at `value` (clamped).
    pub fn slider(id: impl Into<TopBarId>, value: f64, range: RangeInclusive<f64>) -> TopBarItem {
        TopBarItem {
            id: id.into(),
            spec: TopBarSpec::Slider {
                value,
                min: *range.start(),
                max: *range.end(),
            },
            tooltip: None,
            checked: false,
            enabled: true,
            width: None,
            expand: false,
            height: None,
            child: None,
        }
    }

    /// A text label, vertically centred in the band.
    pub fn label(id: impl Into<TopBarId>, text: impl Into<String>) -> TopBarItem {
        TopBarItem {
            id: id.into(),
            spec: TopBarSpec::Label(text.into()),
            tooltip: None,
            checked: false,
            enabled: true,
            width: None,
            expand: false,
            height: None,
            child: None,
        }
    }

    /// A fixed gap between items.
    pub fn spacer() -> TopBarItem {
        TopBarItem {
            id: TopBarId::NONE,
            spec: TopBarSpec::Spacer { fill: false },
            tooltip: None,
            checked: false,
            enabled: true,
            width: None,
            expand: false,
            height: None,
            child: None,
        }
    }

    /// A flexible gap that absorbs the row's leftover width, pushing the items
    /// after it to the right edge.
    pub fn flexible_spacer() -> TopBarItem {
        TopBarItem {
            id: TopBarId::NONE,
            spec: TopBarSpec::Spacer { fill: true },
            tooltip: None,
            checked: false,
            enabled: true,
            width: None,
            expand: false,
            height: None,
            child: None,
        }
    }

    /// A slot of `width` the app fills with its own child window (a native
    /// `Edit` search box). The bar leaves the slot empty and exposes its
    /// rectangle through
    /// [`Ui::material_top_bar_slot`](crate::Ui::material_top_bar_slot).
    pub fn native(id: impl Into<TopBarId>, width: Dip) -> TopBarItem {
        TopBarItem {
            id: id.into(),
            spec: TopBarSpec::Native,
            tooltip: None,
            checked: false,
            enabled: true,
            width: Some(width),
            expand: false,
            height: None,
            child: None,
        }
    }

    /// Sets whether a toggle is checked. Ignored by other kinds.
    pub fn checked(mut self, checked: bool) -> TopBarItem {
        self.checked = checked;
        self
    }

    /// Enables or disables the item. A disabled item ignores input and paints
    /// muted.
    pub fn enabled(mut self, enabled: bool) -> TopBarItem {
        self.enabled = enabled;
        self
    }

    /// Sets the tooltip shown while the pointer rests on the item.
    pub fn tooltip(mut self, text: impl Into<String>) -> TopBarItem {
        self.tooltip = Some(text.into());
        self
    }

    /// Overrides the item's width in the row, in design units. Sliders default
    /// to a sensible width; a label defaults to its text width.
    pub fn width(mut self, width: Dip) -> TopBarItem {
        self.width = Some(width);
        self
    }

    /// Sets a native slot's height, in design units, centred vertically in the
    /// band. Without it the slot fills the band minus a small vertical inset;
    /// set it to the child's natural height (a single-line `Edit`) so the child
    /// is not stretched.
    ///
    /// On an icon or toggle button it sets a compact pill height instead of the
    /// square default, centred in the band; the glyph scales down with it
    /// (capped at its default size). Other kinds ignore it.
    pub fn height(mut self, height: Dip) -> TopBarItem {
        self.height = Some(height);
        self
    }

    /// Hosts `control` in a native slot: the bar moves and resizes it to the
    /// slot on every layout (window resize, DPI change, item change), so the
    /// app never has to track the slot's rectangle. The control stays owned by
    /// the app and must outlive the items that name it. Ignored by other kinds.
    pub fn child(mut self, control: &impl AsControl) -> TopBarItem {
        self.child = Some(native::Child::of(control));
        self
    }

    /// Lets the item grow to take the row's leftover width, so a seek slider
    /// can fill the space between the transport buttons and the right-hand
    /// controls. Several expanding items share the leftover equally.
    pub fn expand(mut self, expand: bool) -> TopBarItem {
        self.expand = expand;
        self
    }
}

/// An event a [`MaterialTopBar`] raises, mapped to the app's `Msg` through the
/// closure given to [`MaterialTopBar::on_event`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TopBarEvent {
    /// An icon button was clicked (or activated with the keyboard).
    Click(TopBarId),
    /// A toggle was flipped to `checked`.
    Toggle { id: TopBarId, checked: bool },
    /// A slider moved while dragging (last value wins per painted frame).
    SliderChange { id: TopBarId, value: f64 },
    /// A slider gesture ended (released, or a keyboard step).
    SliderCommit { id: TopBarId, value: f64 },
}

/// The default design height of the band, in device-independent pixels.
pub(crate) const DEFAULT_HEIGHT_DIP: f32 = 40.0;

/// An interactive bar painted on the window's top material band.
///
/// Build one with [`MaterialTopBar::new`] on a window using
/// [`TitleBar::Extended`](crate::TitleBar::Extended), reserve its height with
/// [`Ui::material_top_bar_height`](crate::Ui::material_top_bar_height) as a top
/// layout margin, set its items with [`set_items`](MaterialTopBar::set_items)
/// on each sync, and map its events with [`on_event`](MaterialTopBar::on_event).
pub struct MaterialTopBar<M: 'static> {
    state: Rc<TopBarState>,
    ui: Ui<M>,
}

impl<M: 'static> MaterialTopBar<M> {
    /// Creates the bar and installs it on `ui`'s window. The window must use an
    /// extended title bar; the material only shows with an active
    /// [`Backdrop`](crate::Backdrop). Returns an error when DirectWrite is
    /// unavailable (use an ordinary child row instead).
    pub fn new(ui: &mut Ui<M>) -> Result<MaterialTopBar<M>> {
        if !crate::window::nc::is_extended(ui.hwnd()) {
            return Err(Error::WindowConfig(
                "the material top bar needs TitleBar::Extended",
            ));
        }
        let state =
            TopBarState::new().ok_or(Error::Direct2d("directwrite unavailable for the top bar"))?;
        ui.install_material_top_bar(Rc::clone(&state));
        Ok(MaterialTopBar {
            state,
            ui: ui.clone(),
        })
    }

    /// Sets the band's design height (default 40 dip), preserved across DPI
    /// changes. Clamped to at least 1 dip.
    pub fn set_height(&self, height: Dip) {
        self.state.set_height_dip(height.value().max(1.0));
        self.ui.refresh_material_top_bar();
    }

    /// Replaces the bar's items. The list is flattened once here; painting and
    /// layout never touch it again.
    pub fn set_items(&self, items: Vec<TopBarItem>) {
        self.state.set_items(items);
        self.ui.refresh_material_top_bar();
    }

    /// Sets a slider or label item's value (see [`set_text`](Self::set_text)
    /// for a label). Ignored while the user drags that slider.
    pub fn set_value(&self, id: impl Into<TopBarId>, value: f64) {
        if self.state.set_value(id.into(), value) {
            self.invalidate();
        }
    }

    /// Sets a toggle's checked state.
    pub fn set_checked(&self, id: impl Into<TopBarId>, checked: bool) {
        if self.state.set_checked(id.into(), checked) {
            self.invalidate();
        }
    }

    /// Enables or disables an item.
    pub fn set_enabled(&self, id: impl Into<TopBarId>, enabled: bool) {
        if self.state.set_enabled(id.into(), enabled) {
            self.invalidate();
        }
    }

    /// Sets a label's text. The bar is re-laid out, because a label's width
    /// changes the row.
    pub fn set_text(&self, id: impl Into<TopBarId>, text: &str) {
        if self.state.set_text(id.into(), text) {
            self.ui.refresh_material_top_bar();
        }
    }

    /// Maps the bar's events to the app's messages. Replaces any previous
    /// mapping.
    pub fn on_event(self, f: impl Fn(TopBarEvent) -> Option<M> + 'static) -> MaterialTopBar<M> {
        self.ui.set_material_top_bar_events(f);
        self
    }

    fn invalidate(&self) {
        crate::sys::window::invalidate(self.ui.hwnd());
    }
}
