#![forbid(unsafe_code)]

//! Which windows have an accessibility source, and how the UI Automation
//! provider (in `sys::uia`) reaches it.
//!
//! The registry is thread-local: providers are created with COM threading, so
//! UI Automation calls them back on the window's own thread, where the widget
//! it describes lives.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use super::{Action, Node, Role};
use crate::hwnd::Hwnd;

/// A window's accessibility source: it snapshots the node tree and performs
/// client actions on it.
pub(crate) trait Source {
    /// The current tree, or `None` while the widget cannot be read (it is
    /// mid-update or gone).
    fn snapshot(&self) -> Option<Node>;

    /// Performs `action` on the node at `path`. Returns whether it was handled.
    fn perform(&self, path: &[usize], action: Action) -> bool;
}

/// Labels the app attached to a window from outside its widget: they replace
/// the root node's name and id in every snapshot.
#[derive(Clone, Default)]
pub(crate) struct Labels {
    pub(crate) name: Option<String>,
    /// A name used only when the widget reports none and the app set none.
    pub(crate) fallback_name: Option<String>,
    pub(crate) id: Option<String>,
}

thread_local! {
    static COMPOSITES: RefCell<HashMap<usize, Rc<Composite>>> = RefCell::new(HashMap::new());
    static LABELS: RefCell<HashMap<usize, Labels>> = RefCell::new(HashMap::new());
    static SOURCES: RefCell<HashMap<usize, Rc<dyn Source>>> = RefCell::new(HashMap::new());
}

/// Registers `source` as `hwnd`'s accessibility source.
pub(crate) fn register(hwnd: Hwnd, source: Rc<dyn Source>) {
    SOURCES.with(|sources| {
        sources.borrow_mut().insert(hwnd.raw(), source);
    });
}

/// Removes `hwnd`'s source (the window is going away).
pub(crate) fn forget(hwnd: Hwnd) {
    SOURCES.with(|sources| {
        sources.borrow_mut().remove(&hwnd.raw());
    });
    LABELS.with(|labels| {
        labels.borrow_mut().remove(&hwnd.raw());
    });
    COMPOSITES.with(|composites| {
        composites.borrow_mut().remove(&hwnd.raw());
    });
}

/// The source registered for `hwnd`, if any.
pub(crate) fn source(hwnd: Hwnd) -> Option<Rc<dyn Source>> {
    SOURCES.with(|sources| sources.borrow().get(&hwnd.raw()).cloned())
}

/// Sets `hwnd`'s accessible name.
pub(crate) fn set_name(hwnd: Hwnd, name: &str) {
    LABELS.with(|labels| {
        labels.borrow_mut().entry(hwnd.raw()).or_default().name = Some(name.to_string());
    });
}

/// Sets the name `hwnd` falls back to when neither the app nor the widget
/// gives one (an edit's cue banner).
pub(crate) fn set_fallback_name(hwnd: Hwnd, name: &str) {
    LABELS.with(|labels| {
        labels
            .borrow_mut()
            .entry(hwnd.raw())
            .or_default()
            .fallback_name = Some(name.to_string());
    });
}

/// Sets `hwnd`'s automation id.
pub(crate) fn set_id(hwnd: Hwnd, id: &str) {
    LABELS.with(|labels| {
        labels.borrow_mut().entry(hwnd.raw()).or_default().id = Some(id.to_string());
    });
}

/// The labels set for `hwnd`.
pub(crate) fn labels(hwnd: Hwnd) -> Labels {
    LABELS.with(|labels| {
        labels
            .borrow()
            .get(&hwnd.raw())
            .cloned()
            .unwrap_or_default()
    })
}

/// A top-level window's source when several parts of it (a top bar, a status
/// bar) each describe themselves: the root is the window, and each section's
/// tree is one child, in the order the sections were added.
struct Composite {
    sections: RefCell<Vec<(&'static str, Rc<dyn Source>)>>,
}

impl Source for Composite {
    fn snapshot(&self) -> Option<Node> {
        let children = self
            .sections
            .borrow()
            .iter()
            .map(|(_, section)| section)
            // A section that cannot be read keeps its slot, so the paths of
            // the others do not shift.
            .map(|section| {
                section
                    .snapshot()
                    .unwrap_or_else(|| Node::new(Role::Group, ""))
            })
            .collect::<Vec<_>>();
        Some(Node::new(Role::Window, "").children(children))
    }

    fn perform(&self, path: &[usize], action: Action) -> bool {
        let Some((section, rest)) = path.split_first() else {
            return false;
        };
        // The section's own root is its node at the empty path, so the rest of
        // the path is relative to it: `[section]` addresses the root itself.
        let section = self
            .sections
            .borrow()
            .get(*section)
            .map(|(_, section)| Rc::clone(section));
        section.is_some_and(|section| section.perform(rest, action))
    }
}

/// Adds `section` (named `key`) to the window `hwnd`: its tree becomes a child
/// of the window's accessibility root. Used by the window-level bars.
pub(crate) fn add_section(hwnd: Hwnd, key: &'static str, section: Rc<dyn Source>) {
    let composite = COMPOSITES.with(|composites| {
        let mut composites = composites.borrow_mut();
        let entry = composites.entry(hwnd.raw()).or_insert_with(|| {
            Rc::new(Composite {
                sections: RefCell::new(Vec::new()),
            })
        });
        Rc::clone(entry)
    });
    // Installing a part again (a rebuilt menu bar) replaces its section and
    // keeps its position.
    let mut sections = composite.sections.borrow_mut();
    match sections.iter_mut().find(|(existing, _)| *existing == key) {
        Some(slot) => slot.1 = section,
        None => sections.push((key, section)),
    }
    drop(sections);
    register(hwnd, composite);
}
