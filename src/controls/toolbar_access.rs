#![forbid(unsafe_code)]

//! The toolbar's accessibility tree: a tool bar whose children are its
//! buttons, each invokable and positioned where it is drawn.

use crate::accessibility::{Node, Role};
use crate::controls::toolbar::ToolbarWidget;

/// The node describing `toolbar`: one invokable button per item.
pub(super) fn node<M>(toolbar: &ToolbarWidget<M>) -> Node {
    let rects = toolbar.rects();
    let buttons = toolbar.items.iter().zip(rects).map(|(item, rect)| {
        let button = Node::new(Role::Button, item.label.clone())
            .invokable()
            .bounds(rect);
        match item.tooltip_text() {
            Some(help) => button.help(help),
            None => button,
        }
    });
    Node::new(Role::ToolBar, "").children(buttons)
}
