#![forbid(unsafe_code)]

//! The material top bar's accessibility section: a tool bar whose children are
//! the bar's buttons, toggles, sliders and labels, laid out where they are
//! painted. Native slots are skipped: the child control in a slot is a real
//! window and describes itself.

use std::rc::{Rc, Weak};

use super::state::{Item, Kind, TopBarState};
use super::{Fluent, TopBarEvent};
use crate::accessibility::registry::Source;
use crate::accessibility::{Action, Node, Role};
use crate::app::core::Core;

/// The accessibility source of a [`MaterialTopBar`](super::MaterialTopBar).
pub(crate) struct TopBarAccess<M> {
    state: Rc<TopBarState>,
    core: Weak<Core<M>>,
}

impl<M> TopBarAccess<M> {
    pub(crate) fn new(state: Rc<TopBarState>, core: Weak<Core<M>>) -> TopBarAccess<M> {
        TopBarAccess { state, core }
    }
}

/// The name a screen reader announces for a bundled [`Fluent`] glyph.
fn glyph_name(glyph: &str) -> &'static str {
    let mut chars = glyph.chars();
    match (chars.next(), chars.next()) {
        (Some(Fluent::PLAY), None) => "Play",
        (Some(Fluent::PAUSE), None) => "Pause",
        (Some(Fluent::STOP), None) => "Stop",
        (Some(Fluent::PREVIOUS), None) => "Previous",
        (Some(Fluent::NEXT), None) => "Next",
        (Some(Fluent::REPEAT), None) => "Repeat",
        (Some(Fluent::SHUFFLE), None) => "Shuffle",
        _ => "",
    }
}

/// Whether an item is exposed: spacers are gaps, native slots are windows.
fn exposed(item: &Item) -> bool {
    !matches!(item.kind, Kind::Spacer | Kind::Native)
}

fn describe(item: &Item, focused: bool) -> Node {
    let glyph = item.glyph.as_ref().map_or("", |glyph| glyph.text());
    let name = item
        .tooltip
        .clone()
        .filter(|tooltip| !tooltip.is_empty())
        .unwrap_or_else(|| glyph_name(glyph).to_string());
    let node = match item.kind {
        Kind::Icon => Node::new(Role::Button, name).invokable(),
        Kind::Toggle => Node::new(Role::CheckBox, name).checked(item.checked),
        Kind::Slider => {
            let slider = item.slider.as_ref();
            let (min, max, value) = slider.map_or((0.0, 1.0, 0.0), |slider| {
                (slider.min(), slider.max(), slider.current())
            });
            Node::new(Role::Slider, name).range(min, max, value, (max - min) / 100.0)
        }
        _ => Node::new(
            Role::Text,
            item.text.as_ref().map_or("", |text| text.text()),
        ),
    };
    let node = node
        .id(format!("top-bar-{}", item.id.value()))
        .enabled(item.enabled);
    if matches!(item.kind, Kind::Label) {
        node
    } else {
        node.focusable(focused)
    }
}

impl<M: 'static> Source for TopBarAccess<M> {
    fn snapshot(&self) -> Option<Node> {
        let items = self.state.items.try_borrow().ok()?;
        let rects = self.state.rects.try_borrow().ok()?;
        let focus = self.state.focus.get();
        let children = items
            .iter()
            .enumerate()
            .filter(|(_, item)| exposed(item))
            .map(|(index, item)| {
                let node = describe(item, focus == Some(index));
                match rects.get(index) {
                    Some(rect) => node.bounds(*rect),
                    None => node,
                }
            })
            .collect::<Vec<_>>();
        Some(Node::new(Role::ToolBar, "Top bar").children(children))
    }

    fn perform(&self, path: &[usize], action: Action) -> bool {
        let [child] = path else {
            return false;
        };
        let (id, kind, enabled, checked) = {
            let Ok(items) = self.state.items.try_borrow() else {
                return false;
            };
            let Some(item) = items.iter().filter(|item| exposed(item)).nth(*child) else {
                return false;
            };
            (item.id, item.kind, item.enabled, item.checked)
        };
        if !enabled {
            return false;
        }
        let event = match (kind, action) {
            (Kind::Icon, Action::Invoke) => TopBarEvent::Click(id),
            (Kind::Toggle, Action::Toggle | Action::Invoke) => {
                self.state.set_checked(id, !checked);
                TopBarEvent::Toggle {
                    id,
                    checked: !checked,
                }
            }
            (Kind::Slider, Action::SetRange(value)) => {
                self.state.set_value(id, value);
                if let Some(core) = self.core.upgrade() {
                    core.emit_top_bar(Some(TopBarEvent::SliderChange { id, value }));
                }
                TopBarEvent::SliderCommit { id, value }
            }
            _ => return false,
        };
        match self.core.upgrade() {
            Some(core) => {
                core.emit_top_bar(Some(event));
                true
            }
            None => false,
        }
    }
}
