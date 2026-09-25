#![forbid(unsafe_code)]

//! The toolbar's accessibility tree: a tool bar whose children are its
//! buttons, each invokable and positioned where it is drawn.

use crate::accessibility::{AccessCx, Node, Role};
use crate::controls::toolbar::ToolbarWidget;

/// The node describing `toolbar`: one invokable button per button item
/// (separators and spacers are not exposed).
pub(crate) fn node<M>(toolbar: &ToolbarWidget<M>, cx: &AccessCx) -> Node {
    let rects = toolbar.accessibility_rects(cx.bounds(), cx.dpi());
    let buttons = toolbar
        .items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.is_button())
        .filter_map(|(index, item)| Some((item, *rects.get(index)?)))
        .map(|(item, rect)| {
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
