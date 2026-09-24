//! ListView behaviour that needs a real window: the UTF-16 owner-data path,
//! targeted model updates, and range selection through owner data.

#![cfg(windows)]

mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

/// Assorted text that has historically tripped up the ANSI/UTF-16 boundary:
/// CJK, emoji beyond the BMP, combining marks, RTL, flags and a long run.
const WEIRD: &[&str] = &[
    "日本語のアルバム",
    "🎵 Émoji 🎶 𝄞",
    "Ω≈ç√∫˜µ≤≥÷",
    "العربية − Ελληνικά − עברית",
    "e\u{301}\u{327} combining",
    "𝔘𝔫𝔦𝔠𝔬𝔡𝔢 𝕗𝕒𝕟𝕔𝕪",
    "🇫🇷🇯🇵 flags",
    "NUL-free\u{200b}zero-width",
];

/// One typed row: both columns borrow from these fields, so the owner-data
/// path never allocates per cell.
struct UnicodeRow {
    title: String,
    detail: String,
}

fn unicode_rows() -> Vec<UnicodeRow> {
    let long = "長".repeat(400);
    let mut rows: Vec<UnicodeRow> = WEIRD
        .iter()
        .map(|text| UnicodeRow {
            title: text.to_string(),
            detail: format!("{text} / 1"),
        })
        .collect();
    rows.push(UnicodeRow {
        title: long,
        detail: String::new(),
    });
    rows
}

enum Msg {
    Start,
}

struct UnicodeApp;

impl App for UnicodeApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Start = msg;
        ui.quit();
    }
}

#[test]
fn unicode_cell_text_round_trips() {
    let round_tripped = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let round_tripped_for_make = Rc::clone(&round_tripped);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.unicode", move |ui| {
        let list = ListView::new(ui)
            .map(|list| {
                list.column("Title", dip(300.0), |row: &UnicodeRow| row.title.as_str())
                    .column("Detail", dip(200.0), |row: &UnicodeRow| row.detail.as_str())
            })
            .ok();
        if let Some(list) = &list {
            list.set_model(unicode_rows());
            // Read back before the loop starts, exercising the UTF-16
            // owner-data path through the typed accessors.
            let long = "長".repeat(400);
            let mut ok = true;
            for (index, expected) in WEIRD.iter().enumerate() {
                if &list.cell_text(index, 0) != expected {
                    ok = false;
                }
            }
            if list.cell_text(0, 1) != format!("{} / 1", WEIRD[0]) {
                ok = false;
            }
            if list.cell_text(WEIRD.len(), 0) != long {
                ok = false;
            }
            if list.cell_text(WEIRD.len(), 1) != String::new() {
                ok = false;
            }
            round_tripped_for_make.set(ok);
        }
        if list.is_none() {
            ui.quit();
        } else {
            created_for_make.set(true);
            ui.emit(Msg::Start);
        }
        UnicodeApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(round_tripped.get(), "UTF-16 cell text did not round-trip");
}

struct BackgroundApp {
    list: Option<ListView<UnicodeRow, Msg>>,
    matches: Rc<Cell<bool>>,
}

impl App for BackgroundApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        let Msg::Start = msg;
        let Some(list) = &self.list else {
            return;
        };
        self.matches
            .set(list.background_color() == Theme::dark().background);
        ui.quit();
    }
}

#[test]
fn background_colour_is_applied() {
    let matches = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let matches_for_make = Rc::clone(&matches);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.listbg", move |ui| {
        ui.set_theme(Theme::dark());
        let list = ListView::new(ui)
            .map(|list| list.column("A", dip(80.0), |row: &UnicodeRow| row.title.as_str()))
            .ok();
        if let Some(list) = &list {
            list.set_model(unicode_rows());
            created_for_make.set(true);
            ui.emit(Msg::Start);
        } else {
            ui.quit();
        }
        BackgroundApp {
            list,
            matches: matches_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(
        matches.get(),
        "the list view did not apply the theme background"
    );
}

struct NameRow {
    name: String,
}

fn names(words: &[&str]) -> Vec<NameRow> {
    words
        .iter()
        .map(|word| NameRow {
            name: word.to_string(),
        })
        .collect()
}

enum ModelMsg {
    Start,
    Check,
}

struct ModelApp {
    list: Option<ListView<NameRow, ModelMsg>>,
    checks: Rc<RefCell<Vec<bool>>>,
}

impl App for ModelApp {
    type Msg = ModelMsg;

    fn update(&mut self, msg: ModelMsg, ui: &mut Ui<ModelMsg>) {
        let Some(list) = &self.list else {
            ui.quit();
            return;
        };
        match msg {
            ModelMsg::Start => {
                let mut checks = self.checks.borrow_mut();
                // The rows handed over at construction.
                checks.push(list.cell_text(0, 0) == "a");
                checks.push(list.cell_text(2, 0) == "c");
                // A wholesale model swap refreshes the count and the cells.
                list.set_model(names(&["a", "b", "c", "d", "e"]));
                checks.push(list.cell_text(4, 0) == "e");
                checks.push(list.cell_text(5, 0).is_empty());
                // Targeted updates repaint without moving the count.
                list.rows_changed(1..3);
                checks.push(list.cell_text(1, 0) == "b");
                list.rows_inserted(0..2);
                checks.push(list.cell_text(0, 0) == "a");
                list.rows_removed(0..0);
                checks.push(list.cell_text(4, 0) == "e");
                // Shrinking the model hides the dropped rows.
                list.set_model(names(&["a", "b"]));
                list.rows_removed(2..5);
                checks.push(list.cell_text(1, 0) == "b");
                checks.push(list.cell_text(2, 0).is_empty());
                drop(checks);
                ui.emit(ModelMsg::Check);
            }
            ModelMsg::Check => ui.quit(),
        }
    }
}

/// Swapping the model and issuing targeted updates refreshes exactly the rows
/// the app names.
#[test]
fn model_changes_update_the_view() {
    let checks = Rc::new(RefCell::new(Vec::new()));
    let created = Rc::new(Cell::new(false));

    let checks_for_make = Rc::clone(&checks);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.listmodel", move |ui| {
        let list = ListView::new(ui)
            .map(|list| list.column("Name", dip(120.0), |row: &NameRow| row.name.as_str()))
            .ok();
        if let Some(list) = &list {
            list.set_model(names(&["a", "b", "c"]));
            created_for_make.set(true);
            ui.emit(ModelMsg::Start);
        } else {
            ui.quit();
        }
        ModelApp {
            list,
            checks: checks_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(
        !checks.borrow().is_empty() && checks.borrow().iter().all(|&ok| ok),
        "a model change did not reach the view: {:?}",
        checks.borrow()
    );
}

enum SelectMsg {
    Start,
    Got(Vec<usize>),
}

struct SelectApp {
    list: Option<ListView<NameRow, SelectMsg>>,
    got: Rc<RefCell<Vec<Vec<usize>>>>,
    states_match: Rc<Cell<bool>>,
}

impl App for SelectApp {
    type Msg = SelectMsg;

    fn update(&mut self, msg: SelectMsg, ui: &mut Ui<SelectMsg>) {
        let Some(list) = &self.list else {
            ui.quit();
            return;
        };
        match msg {
            SelectMsg::Start => list.set_selection(&[1, 2, 3]),
            SelectMsg::Got(rows) => {
                // The control's live state always matches the reported one.
                self.states_match.set(list.selection() == rows);
                self.got.borrow_mut().push(rows);
                match self.got.borrow().len() {
                    // Re-selecting the same rows reports nothing.
                    1 => {
                        list.set_selection(&[3, 2, 1, 1]);
                        list.set_selection(&[2]);
                    }
                    // Clearing the selection reports the empty set once.
                    2 => list.set_selection(&[]),
                    _ => ui.quit(),
                }
            }
        }
    }
}

/// Multi-select goes through the owner-data range path and coalesces: one
/// programmatic change is exactly one message, a no-op change is silent, and
/// clearing reports the empty selection.
#[test]
fn range_selection_coalesces_into_one_event() {
    let got = Rc::new(RefCell::new(Vec::new()));
    let states_match = Rc::new(Cell::new(true));
    let created = Rc::new(Cell::new(false));

    let got_for_make = Rc::clone(&got);
    let states_for_make = Rc::clone(&states_match);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.listsel", move |ui| {
        let list = ListView::new(ui)
            .map(|list| {
                list.column("Name", dip(120.0), |row: &NameRow| row.name.as_str())
                    .multi_select(true)
                    .on_select(|rows| Some(SelectMsg::Got(rows.to_vec())))
            })
            .ok();
        if let Some(list) = &list {
            list.set_model(names(&["a", "b", "c", "d", "e"]));
            created_for_make.set(true);
            ui.emit(SelectMsg::Start);
        } else {
            ui.quit();
        }
        SelectApp {
            list,
            got: got_for_make,
            states_match: states_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert_eq!(
        *got.borrow(),
        vec![vec![1, 2, 3], vec![2], Vec::<usize>::new()],
        "selection changes did not coalesce into one event each"
    );
    assert!(
        states_match.get(),
        "the control's selection did not match a report"
    );
}

enum RowMsg {
    Start,
}

struct RowApp {
    list: Option<ListView<NameRow, RowMsg>>,
    ok: Rc<Cell<bool>>,
}

impl App for RowApp {
    type Msg = RowMsg;

    fn update(&mut self, msg: RowMsg, ui: &mut Ui<RowMsg>) {
        let RowMsg::Start = msg;
        if let Some(list) = &self.list {
            // The builders wire up without panicking, and reading a cell back
            // still drives the owner-data path with a `row_painter`/`row_style`
            // installed and a non-default row height.
            let ok = list.cell_text(0, 0) == "a" && list.cell_text(2, 0) == "c";
            self.ok.set(ok);
        }
        ui.quit();
    }
}

/// `row_style`, `row_painter`, `row_height` and `zebra` are chainable builders
/// that do not disturb the ordinary owner-data cell/selection path.
#[test]
fn row_appearance_builders_do_not_disturb_the_view() {
    let ok = Rc::new(Cell::new(false));
    let created = Rc::new(Cell::new(false));

    let ok_for_make = Rc::clone(&ok);
    let created_for_make = Rc::clone(&created);
    let Some(run) = run_app_with_watchdog("win32ui.listrowstyle", move |ui| {
        let list = ListView::new(ui)
            .map(|list| {
                list.column("Name", dip(120.0), |row: &NameRow| row.name.as_str())
                    .row_height(dip(24.0))
                    .zebra(true)
                    .row_style(|row: &NameRow| {
                        RowStyle::new()
                            .bold(row.name == "b")
                            .accent_bar(Color::rgb(255, 0, 0))
                    })
                    .row_painter(|row: &NameRow, canvas, rect, state| {
                        // Only take over "special" rows; everything else falls
                        // back to the default painting (exercising `row_style`
                        // for those rows too).
                        if row.name != "special" {
                            return false;
                        }
                        canvas.fill_rect(rect, Color::rgb(0, 0, 0));
                        let _ = state.selected;
                        true
                    })
            })
            .ok();
        if let Some(list) = &list {
            list.set_model(names(&["a", "b", "c"]));
            created_for_make.set(true);
            ui.emit(RowMsg::Start);
        } else {
            ui.quit();
        }
        RowApp {
            list,
            ok: ok_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if !created.get() {
        return;
    }
    assert!(
        ok.get(),
        "row appearance builders broke the cell/model path"
    );
}
