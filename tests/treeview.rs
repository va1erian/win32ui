//! The typed `TreeView<K>`: lazy loading, expansion/selection by key, and the
//! keyed refresh that keeps them across a model change.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

#[derive(Clone)]
struct Folder {
    id: u32,
    name: String,
    unread: u32,
    children: Vec<Folder>,
}

struct Folders {
    roots: Rc<RefCell<Vec<Folder>>>,
}

impl TreeModel for Folders {
    type Key = u32;

    fn children(&self, parent: Option<&u32>) -> Vec<Node<u32>> {
        let roots = self.roots.borrow();
        let list: &[Folder] = match parent {
            None => roots.as_slice(),
            Some(id) => find(roots.as_slice(), *id)
                .map(|folder| folder.children.as_slice())
                .unwrap_or(&[]),
        };
        list.iter()
            .map(|folder| {
                if folder.children.is_empty() {
                    Node::leaf(folder.id, folder.name.as_str())
                } else {
                    Node::branch(folder.id, folder.name.as_str())
                }
            })
            .collect()
    }
}

fn find(folders: &[Folder], id: u32) -> Option<&Folder> {
    for folder in folders {
        if folder.id == id {
            return Some(folder);
        }
        if let Some(found) = find(&folder.children, id) {
            return Some(found);
        }
    }
    None
}

fn fixture() -> Vec<Folder> {
    vec![
        Folder {
            id: 1,
            name: "Inbox".to_string(),
            unread: 3,
            children: vec![
                Folder {
                    id: 11,
                    name: "Work".to_string(),
                    unread: 2,
                    children: Vec::new(),
                },
                Folder {
                    id: 12,
                    name: "Personal".to_string(),
                    unread: 1,
                    children: Vec::new(),
                },
            ],
        },
        Folder {
            id: 2,
            name: "Sent".to_string(),
            unread: 0,
            children: Vec::new(),
        },
    ]
}

#[derive(Default)]
struct Observed {
    roots: Cell<i32>,
    after_expand: Cell<i32>,
    selected: Cell<Option<u32>>,
    after_refresh: Cell<i32>,
    still_selected: Cell<Option<u32>>,
    still_expanded: Cell<bool>,
    /// The last `on_toggle` payload; stays `None` for a programmatic expand.
    folded: Cell<Option<(u32, bool)>>,
}

enum Msg {
    Start,
    TreeSelect,
    TreeFold(u32, bool),
}

struct TestApp {
    tree: TreeView<u32, Msg>,
    roots: Rc<RefCell<Vec<Folder>>>,
    observed: Rc<Observed>,
}

impl App for TestApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Start => {
                self.observed.roots.set(self.tree.node_count());

                self.tree.expand(&1, true);
                self.observed.after_expand.set(self.tree.node_count());

                self.tree.select(&11);
                self.observed.selected.set(self.tree.selected());

                // Change the model underneath the tree, then refresh: the
                // loaded expansion and selection must survive.
                self.roots.borrow_mut()[0].unread += 1;
                self.tree.refresh();
                self.observed.after_refresh.set(self.tree.node_count());
                self.observed.still_selected.set(self.tree.selected());
                self.observed.still_expanded.set(self.tree.is_expanded(&1));
                ui.quit();
            }
            Msg::TreeSelect => {}
            Msg::TreeFold(id, expanded) => self.observed.folded.set(Some((id, expanded))),
        }
    }
}

#[test]
fn typed_tree_loads_expands_and_refreshes_in_place() {
    let roots = Rc::new(RefCell::new(fixture()));
    let observed = Rc::new(Observed::default());

    let roots_for_make = Rc::clone(&roots);
    let observed_for_make = Rc::clone(&observed);

    let Some(run) = run_app_with_watchdog("win32ui.treeview", move |ui| {
        let tree = TreeView::new(
            ui,
            Folders {
                roots: Rc::clone(&roots_for_make),
            },
        )
        .expect("tree")
        .style(|id| NodeStyle::new().badge(id.to_string()))
        .on_select(|_| Some(Msg::TreeSelect))
        .on_toggle(|id, expanded| Some(Msg::TreeFold(*id, expanded)));
        ui.emit(Msg::Start);
        TestApp {
            tree,
            roots: roots_for_make,
            observed: observed_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(observed.roots.get(), 2, "the two roots did not load");
    assert_eq!(
        observed.after_expand.get(),
        4,
        "expanding the inbox did not add its two children"
    );
    assert_eq!(
        observed.selected.get(),
        Some(11),
        "select(&11) did not select the work folder"
    );
    assert_eq!(
        observed.after_refresh.get(),
        4,
        "refresh rebuilt the tree instead of diffing in place"
    );
    assert_eq!(
        observed.still_selected.get(),
        Some(11),
        "refresh lost the selection"
    );
    assert!(
        observed.still_expanded.get(),
        "refresh collapsed the expanded branch"
    );
    assert_eq!(
        observed.folded.get(),
        None,
        "a programmatic expand must not emit on_toggle"
    );
}
