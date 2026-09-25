#![forbid(unsafe_code)]

//! The progress bar's accessibility source: a read-only range, or no value at
//! all while the marquee (indeterminate) animation runs.

use std::cell::RefCell;
use std::rc::Weak;

use crate::accessibility::registry::Source;
use crate::accessibility::{Action, Node, Role};

use super::state::ProgressBarState;

pub(super) struct ProgressAccess {
    pub(super) state: Weak<RefCell<ProgressBarState>>,
}

impl Source for ProgressAccess {
    fn snapshot(&self) -> Option<Node> {
        let state = self.state.upgrade()?;
        let state = state.try_borrow().ok()?;
        let node = Node::new(Role::ProgressBar, "");
        Some(if state.marquee {
            node
        } else {
            node.read_only_range(
                f64::from(state.min),
                f64::from(state.max),
                f64::from(state.value),
            )
        })
    }

    fn perform(&self, _path: &[usize], _action: Action) -> bool {
        false
    }
}
