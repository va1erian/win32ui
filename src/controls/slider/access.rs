#![forbid(unsafe_code)]

//! The slider's accessibility node: a settable range at the slider's own
//! value, with the keyboard step as the small change.

use crate::accessibility::{Node, Role};

use super::state::SliderState;

/// The node describing `state`. The name comes from the control's accessible
/// name ([`ControlExt::set_accessible_name`](crate::ControlExt::set_accessible_name)).
pub(super) fn node(state: &SliderState) -> Node {
    let node = Node::new(Role::Slider, "")
        .range(state.min(), state.max(), state.value(), state.small_step)
        .enabled(state.enabled);
    node.focusable(state.is_focused())
}
