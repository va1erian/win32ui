//! UI Automation providers (#26): a custom widget describes itself with a
//! `Node` tree, and a real UI Automation client reads and drives it.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use common::uia::{self, spawn_client};
use win32ui::Size;
use win32ui::accessibility::{AccessCx, Action, Node, Role};
use win32ui::gdi::Canvas;
use win32ui::prelude::*;

/// A widget with a button, a check box, a slider and a selectable row.
struct Probe {
    log: Rc<RefCell<Vec<String>>>,
    volume: RefCell<f64>,
    shuffled: RefCell<bool>,
}

impl CustomWidget for Probe {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn preferred_size(&self, _dpi: u32) -> Option<Size> {
        Some(Size::new(200, 100))
    }

    fn accessibility(&self, _cx: &AccessCx) -> Option<Node> {
        Some(
            Node::new(Role::Group, "Probe").children([
                Node::new(Role::Button, "Play")
                    .id("play")
                    .invokable()
                    .focusable(false)
                    .bounds(Rect::new(0, 0, 40, 20)),
                Node::new(Role::CheckBox, "Shuffle")
                    .checked(*self.shuffled.borrow())
                    .bounds(Rect::new(40, 0, 100, 20)),
                Node::new(Role::Slider, "Volume")
                    .range(0.0, 10.0, *self.volume.borrow(), 1.0)
                    .bounds(Rect::new(0, 20, 100, 40)),
                Node::new(Role::ListItem, "Row").selected(false),
            ]),
        )
    }

    fn accessibility_action(&self, path: &[usize], action: Action, _cx: &mut WidgetCx<()>) -> bool {
        self.log.borrow_mut().push(format!("{path:?} {action:?}"));
        match (path, action) {
            ([1], Action::Toggle) => {
                let mut on = self.shuffled.borrow_mut();
                *on = !*on;
                true
            }
            ([2], Action::SetRange(value)) => {
                *self.volume.borrow_mut() = value;
                true
            }
            ([0], Action::Invoke) | ([3], Action::Select) => true,
            _ => false,
        }
    }
}

struct Shell;

impl App for Shell {
    type Msg = ();

    fn update(&mut self, _msg: (), ui: &mut Ui<()>) {
        ui.quit();
    }
}

/// The tree a client sees matches the widget's nodes, and every action a
/// client performs reaches the widget with the right path.
#[test]
fn custom_widget_is_visible_and_drivable() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let log_for_make = Rc::clone(&log);
    let client = std::cell::RefCell::new(None);

    let Some(run) = run_app_with_watchdog("win32ui.uia.custom", |ui| {
        let probe = Probe {
            log: log_for_make,
            volume: RefCell::new(3.0),
            shuffled: RefCell::new(false),
        };
        let custom = Custom::<Probe, ()>::new(ui, probe).unwrap();
        std::mem::forget(custom);
        let proxy = ui.proxy();
        let done = move || {
            let _ = proxy.send(());
        };
        *client.borrow_mut() = Some(spawn_client("win32ui.uia.custom", done, |client| {
            let tree = client.dump();
            let play = client.find("Play").expect("Play button");
            let invoked = uia::invoke(&play);
            let shuffle = client.find("Shuffle").expect("Shuffle check box");
            let toggled = uia::toggle(&shuffle);
            let volume = client.find("Volume").expect("Volume slider");
            let before = uia::range(&volume);
            let after = uia::set_range(&volume, 7.0);
            let row = client.find("Row").expect("Row item");
            let selected = uia::select(&row);
            (tree, invoked, toggled, before, after, selected)
        }));
        Shell
    }) else {
        return;
    };
    assert!(
        !run.timed_out,
        "the watchdog fired before the client finished"
    );

    let (tree, invoked, toggled, before, after, selected) = client
        .borrow_mut()
        .take()
        .expect("client thread")
        .join()
        .expect("client thread panicked")
        .expect("client connected");

    let roles: Vec<(String, i32)> = tree
        .iter()
        .map(|n| (n.name.clone(), n.control_type))
        .collect();
    for (name, control_type) in [
        ("Play", 50000),
        ("Shuffle", 50002),
        ("Volume", 50015),
        ("Row", 50007),
    ] {
        assert!(
            roles.contains(&(name.to_string(), control_type)),
            "missing {name} ({control_type}) in {roles:?}"
        );
    }
    let play = tree.iter().find(|n| n.name == "Play").unwrap();
    assert_eq!(play.automation_id, "play");

    assert!(invoked, "Invoke reached the widget");
    assert_eq!(toggled, Some(1), "the check box reads on after Toggle");
    assert_eq!(before, Some((3.0, 10.0)));
    assert_eq!(after, Some(7.0));
    assert_eq!(selected, Some(false), "the widget did not report selection");

    let log = log.borrow();
    assert!(log.iter().any(|l| l.contains("[0] Invoke")), "{log:?}");
    assert!(log.iter().any(|l| l.contains("[1] Toggle")), "{log:?}");
    assert!(
        log.iter().any(|l| l.contains("[2] SetRange(7.0)")),
        "{log:?}"
    );
    assert!(log.iter().any(|l| l.contains("[3] Select")), "{log:?}");
}

/// What the built-in controls report and accept, driven through the same
/// client: an owner-drawn button, a radio group and a slider.
#[derive(Clone, Debug, PartialEq)]
enum Msg {
    Clicked,
    Picked(&'static str),
    Volume(f64),
    Done,
}

struct Controls {
    log: Rc<RefCell<Vec<Msg>>>,
}

impl App for Controls {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        if msg == Msg::Done {
            ui.quit();
        } else {
            self.log.borrow_mut().push(msg);
        }
    }
}

/// Owner-drawn buttons, radio options and sliders are visible with the right
/// role, name and state, and a client can drive them.
#[test]
fn built_in_controls_are_visible_and_drivable() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let log_for_make = Rc::clone(&log);
    let client = RefCell::new(None);

    let Some(run) = run_app_with_watchdog("win32ui.uia.controls", |ui| {
        let button = Button::new(ui, "Send")
            .unwrap()
            .on_click(|| Some(Msg::Clicked));
        button.set_accessible_id("send");
        let radios = RadioGroup::new(ui, [("Small", "small"), ("Large", "large")])
            .unwrap()
            .on_select(|value| Some(Msg::Picked(value)));
        let slider = Slider::new(ui, 0.0..=10.0)
            .unwrap()
            .value(2.0)
            .on_change(|value| Some(Msg::Volume(value)))
            .on_commit(|value| Some(Msg::Volume(value)));
        slider.set_accessible_name("Volume");
        slider.set_accessible_id("volume");
        std::mem::forget((button, radios, slider));

        let proxy = ui.proxy();
        let done = move || {
            let _ = proxy.send(Msg::Done);
        };
        let click_proxy = ui.proxy();
        *client.borrow_mut() = Some(spawn_client("win32ui.uia.controls", done, move |client| {
            let _ = &click_proxy;
            let tree = client.dump();
            let send = client.find("Send").expect("Send button");
            let invoked = uia::invoke(&send);
            let large = client.find("Large").expect("Large radio");
            let selected = uia::select(&large);
            let small = client.find("Small").expect("Small radio");
            let small_selected = uia::is_selected(&small);
            let volume = client.find("Volume").expect("Volume slider");
            let range = uia::range(&volume);
            let set = uia::set_range(&volume, 8.0);
            (tree, invoked, selected, small_selected, range, set)
        }));
        Controls { log: log_for_make }
    }) else {
        return;
    };
    assert!(
        !run.timed_out,
        "the watchdog fired before the client finished"
    );

    let (tree, invoked, selected, small_selected, range, set) = client
        .borrow_mut()
        .take()
        .unwrap()
        .join()
        .expect("client thread panicked")
        .expect("client connected");

    let has = |name: &str, control_type: i32| {
        tree.iter()
            .any(|n| n.name == name && n.control_type == control_type)
    };
    assert!(has("Send", 50000), "button: {tree:?}");
    assert!(has("Small", 50013), "radio: {tree:?}");
    assert!(has("Large", 50013), "radio: {tree:?}");
    assert!(has("Volume", 50015), "slider: {tree:?}");
    let send = tree.iter().find(|n| n.name == "Send").unwrap();
    assert_eq!(send.automation_id, "send");

    assert!(invoked);
    eprintln!("LOG {:?}", log.borrow());
    assert_eq!(selected, Some(true), "the picked radio reports selected");
    assert_eq!(small_selected, Some(false), "the other radio deselected");
    assert_eq!(range.map(|r| r.0), Some(2.0));
    assert_eq!(set, Some(8.0));

    let log = log.borrow();
    assert!(log.contains(&Msg::Clicked), "{log:?}");
    assert!(log.contains(&Msg::Picked("large")), "{log:?}");
    assert!(log.contains(&Msg::Volume(8.0)), "{log:?}");
}

/// A ListView exposes its visible rows as items with cells, and a client can
/// select and activate a row.
#[derive(Clone, Debug, PartialEq)]
enum ListMsg {
    Selected(Vec<usize>),
    Opened(usize),
    Done,
}

struct Names(Vec<[&'static str; 2]>);

impl ListModel for Names {
    type Item = [&'static str; 2];

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<&[&'static str; 2]> {
        self.0.as_slice().get(index)
    }
}

struct ListApp {
    log: Rc<RefCell<Vec<ListMsg>>>,
}

impl App for ListApp {
    type Msg = ListMsg;

    fn update(&mut self, msg: ListMsg, ui: &mut Ui<ListMsg>) {
        if msg == ListMsg::Done {
            ui.quit();
        } else {
            self.log.borrow_mut().push(msg);
        }
    }
}

#[test]
fn list_view_rows_are_visible_and_drivable() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let log_for_make = Rc::clone(&log);
    let client = RefCell::new(None);

    let Some(run) = run_app_with_watchdog("win32ui.uia.list", |ui| {
        let list = ListView::new(ui)
            .unwrap()
            .column("Name", dip(120.0), |row: &[&'static str; 2]| row[0])
            .column("Kind", dip(80.0), |row: &[&'static str; 2]| row[1])
            .on_select(|rows| Some(ListMsg::Selected(rows.to_vec())))
            .on_activate(|row| Some(ListMsg::Opened(row)));
        list.set_model(Names(vec![
            ["alpha", "one"],
            ["beta", "two"],
            ["gamma", "three"],
        ]));
        list.set_accessible_name("Files");
        list.set_bounds(Rect::new(0, 0, 300, 200));
        std::mem::forget(list);

        let proxy = ui.proxy();
        let done = move || {
            let _ = proxy.send(ListMsg::Done);
        };
        *client.borrow_mut() = Some(spawn_client("win32ui.uia.list", done, |client| {
            let tree = client.dump();
            let beta = client.find("beta, two").expect("beta row");
            let selected = uia::select(&beta);
            let invoked = uia::invoke(&beta);
            (tree, selected, invoked)
        }));
        ListApp { log: log_for_make }
    }) else {
        return;
    };
    assert!(
        !run.timed_out,
        "the watchdog fired before the client finished"
    );

    let (tree, selected, invoked) = client
        .borrow_mut()
        .take()
        .unwrap()
        .join()
        .expect("client thread panicked")
        .expect("client connected");

    let has = |name: &str, control_type: i32| {
        tree.iter()
            .any(|n| n.name == name && n.control_type == control_type)
    };
    assert!(has("Files", 50008), "list: {tree:?}");
    for row in ["alpha, one", "beta, two", "gamma, three"] {
        assert!(has(row, 50007), "row {row}: {tree:?}");
    }
    assert!(has("two", 50020), "cell text: {tree:?}");
    assert_eq!(selected, Some(true));
    assert!(invoked);
    let log = log.borrow();
    assert!(log.contains(&ListMsg::Selected(vec![1])), "{log:?}");
    assert!(log.contains(&ListMsg::Opened(1)), "{log:?}");
}

/// Labels, check boxes, edits and combo boxes report their text and state and
/// take a client's actions.
#[derive(Clone, Debug, PartialEq)]
enum FormMsg {
    Toggled(bool),
    Typed(String),
    Picked(&'static str),
    Done,
}

struct FormApp {
    log: Rc<RefCell<Vec<FormMsg>>>,
}

impl App for FormApp {
    type Msg = FormMsg;

    fn update(&mut self, msg: FormMsg, ui: &mut Ui<FormMsg>) {
        if msg == FormMsg::Done {
            ui.quit();
        } else {
            self.log.borrow_mut().push(msg);
        }
    }
}

#[test]
fn form_controls_are_visible_and_drivable() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let log_for_make = Rc::clone(&log);
    let client = RefCell::new(None);

    let Some(run) = run_app_with_watchdog("win32ui.uia.form", |ui| {
        let label = Label::new(ui, Rect::new(0, 0, 80, 20), "Status").unwrap();
        let check = CheckBox::new(ui, "Remember me")
            .unwrap()
            .on_toggle(|on| Some(FormMsg::Toggled(on)));
        let edit = Edit::single_line(ui)
            .unwrap()
            .cue("Search")
            .on_change(|text| Some(FormMsg::Typed(text.to_string())));
        edit.set_bounds(Rect::new(0, 0, 100, 20));
        let combo = ComboBox::new(ui, [("Small", "small"), ("Large", "large")])
            .unwrap()
            .on_select(|value| Some(FormMsg::Picked(value)));
        combo.set_bounds(Rect::new(0, 30, 100, 200));
        combo.set_selected(&"small");
        std::mem::forget((label, check, edit, combo));

        let proxy = ui.proxy();
        let done = move || {
            let _ = proxy.send(FormMsg::Done);
        };
        *client.borrow_mut() = Some(spawn_client("win32ui.uia.form", done, |client| {
            let tree = client.dump();
            let check = client.find("Remember me").expect("check box");
            let toggled = uia::toggle(&check);
            let edit = client.find("Search").expect("edit");
            let typed = uia::set_value(&edit, "hello");
            let text = uia::value(&edit);
            let large = client.find("Large").expect("combo choice");
            let picked = uia::select(&large);
            (tree, toggled, typed, text, picked)
        }));
        FormApp { log: log_for_make }
    }) else {
        return;
    };
    assert!(
        !run.timed_out,
        "the watchdog fired before the client finished"
    );

    let (tree, toggled, typed, text, picked) = client
        .borrow_mut()
        .take()
        .unwrap()
        .join()
        .expect("client thread panicked")
        .expect("client connected");

    let has = |name: &str, control_type: i32| {
        tree.iter()
            .any(|n| n.name == name && n.control_type == control_type)
    };
    assert!(has("Status", 50020), "label: {tree:?}");
    assert!(has("Remember me", 50002), "check box: {tree:?}");
    assert!(has("Search", 50004), "edit named by its cue: {tree:?}");
    assert!(has("Large", 50007), "combo choice: {tree:?}");
    assert_eq!(toggled, Some(1));
    assert!(typed);
    assert_eq!(text.as_deref(), Some("hello"));
    assert_eq!(picked, Some(true));
    let log = log.borrow();
    assert!(log.contains(&FormMsg::Toggled(true)), "{log:?}");
    assert!(log.contains(&FormMsg::Typed("hello".into())), "{log:?}");
    assert!(log.contains(&FormMsg::Picked("large")), "{log:?}");
}
