//! A UI Automation element as a path into a window's accessibility tree, and
//! the property lookups the providers build on. Every lookup takes a fresh
//! snapshot from the window's [`Source`](crate::accessibility::registry::Source),
//! so an element can never describe a widget that has since changed.

use crate::accessibility::registry::{self, Source};
use crate::accessibility::{Action, Node, Role};
use crate::geometry::{Point, Rect};
use crate::hwnd::Hwnd;

/// One element: the window it lives in and the index path to its node.
#[derive(Clone)]
pub(super) struct Target {
    pub(super) hwnd: Hwnd,
    pub(super) path: Vec<usize>,
}

impl Target {
    pub(super) fn root(hwnd: Hwnd) -> Target {
        Target {
            hwnd,
            path: Vec::new(),
        }
    }

    pub(super) fn is_root(&self) -> bool {
        self.path.is_empty()
    }

    fn source(&self) -> Option<std::rc::Rc<dyn Source>> {
        registry::source(self.hwnd)
    }

    /// The child at `index`, as a target.
    pub(super) fn child(&self, index: usize) -> Target {
        let mut path = self.path.clone();
        path.push(index);
        Target {
            hwnd: self.hwnd,
            path,
        }
    }

    /// The parent target, or `None` for the root.
    pub(super) fn parent(&self) -> Option<Target> {
        let mut path = self.path.clone();
        path.pop()?;
        Some(Target {
            hwnd: self.hwnd,
            path,
        })
    }

    /// A fresh copy of this element's node, if it still exists.
    pub(super) fn node(&self) -> Option<Node> {
        let tree = self.snapshot()?;
        tree.at(&self.path).cloned()
    }

    /// The window's tree with the app's name and id labels applied to its root.
    fn snapshot(&self) -> Option<Node> {
        let mut tree = self.source()?.snapshot()?;
        let labels = registry::labels(self.hwnd);
        if let Some(name) = labels.name {
            tree.name = name;
        } else if tree.name.is_empty()
            && let Some(name) = labels.fallback_name
        {
            tree.name = name;
        }
        if labels.id.is_some() {
            tree.id = labels.id;
        }
        Some(tree)
    }

    /// How many children the element has, and this element's index among its
    /// parent's children.
    pub(super) fn sibling_context(&self) -> Option<(usize, usize)> {
        let index = *self.path.last()?;
        let tree = self.snapshot()?;
        let parent = tree.at(&self.path[..self.path.len() - 1])?;
        Some((index, parent.children.len()))
    }

    /// The element's bounds in screen pixels.
    pub(super) fn screen_bounds(&self) -> Option<Rect> {
        let tree = self.snapshot()?;
        let client = crate::sys::window::client_rect(self.hwnd);
        let mut bounds = tree.bounds.unwrap_or(client);
        let mut node = &tree;
        for index in &self.path {
            node = node.children.get(*index)?;
            if let Some(child) = node.bounds {
                bounds = child;
            }
        }
        let origin = crate::sys::window::client_to_screen(self.hwnd, Point::new(0, 0));
        Some(bounds.offset(origin.x, origin.y))
    }

    /// The deepest node containing the screen point, as a target.
    pub(super) fn hit_test(&self, x: i32, y: i32) -> Option<Target> {
        let tree = self.snapshot()?;
        let origin = crate::sys::window::client_to_screen(self.hwnd, Point::new(0, 0));
        let point = Point::new(x - origin.x, y - origin.y);
        let client = crate::sys::window::client_rect(self.hwnd);
        let mut path = Vec::new();
        let mut node = &tree;
        let mut bounds = tree.bounds.unwrap_or(client);
        'descend: loop {
            for (index, child) in node.children.iter().enumerate().rev() {
                let child_bounds = child.bounds.unwrap_or(bounds);
                if child_bounds.contains(point) {
                    path.push(index);
                    node = child;
                    bounds = child_bounds;
                    continue 'descend;
                }
            }
            break;
        }
        Some(Target {
            hwnd: self.hwnd,
            path,
        })
    }

    /// The path of the node holding the keyboard focus, if any.
    pub(super) fn focused(&self) -> Option<Target> {
        fn find(node: &Node, path: &mut Vec<usize>) -> bool {
            for (index, child) in node.children.iter().enumerate() {
                path.push(index);
                if child.focused || find(child, path) {
                    return true;
                }
                path.pop();
            }
            false
        }
        let tree = self.snapshot()?;
        let mut path = Vec::new();
        if tree.focused {
            return Some(Target::root(self.hwnd));
        }
        find(&tree, &mut path).then_some(Target {
            hwnd: self.hwnd,
            path,
        })
    }

    /// Asks the widget to perform `action` on this element.
    pub(super) fn perform(&self, action: Action) -> bool {
        self.source()
            .is_some_and(|source| source.perform(&self.path, action))
    }
}

/// The UI Automation control-type id for a role.
pub(super) fn control_type(role: Role) -> i32 {
    use windows::Win32::UI::Accessibility::*;
    match role {
        Role::Button => UIA_ButtonControlTypeId,
        Role::CheckBox => UIA_CheckBoxControlTypeId,
        Role::RadioButton => UIA_RadioButtonControlTypeId,
        Role::Slider => UIA_SliderControlTypeId,
        Role::ProgressBar => UIA_ProgressBarControlTypeId,
        Role::Text => UIA_TextControlTypeId,
        Role::Edit => UIA_EditControlTypeId,
        Role::ComboBox => UIA_ComboBoxControlTypeId,
        Role::List => UIA_ListControlTypeId,
        Role::ListItem => UIA_ListItemControlTypeId,
        Role::Tab => UIA_TabControlTypeId,
        Role::TabItem => UIA_TabItemControlTypeId,
        Role::ToolBar => UIA_ToolBarControlTypeId,
        Role::MenuBar => UIA_MenuBarControlTypeId,
        Role::MenuItem => UIA_MenuItemControlTypeId,
        Role::StatusBar => UIA_StatusBarControlTypeId,
        Role::Group => UIA_GroupControlTypeId,
        Role::Image => UIA_ImageControlTypeId,
        Role::Document => UIA_DocumentControlTypeId,
        Role::Separator => UIA_SeparatorControlTypeId,
        Role::Window => UIA_WindowControlTypeId,
        Role::Custom => UIA_CustomControlTypeId,
    }
    .0
}
