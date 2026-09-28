//! The demo's Drag tab: drag rows from the library list into the queue list
//! (an insertion marker shows where they land), reorder the queue by dragging
//! its own rows, drop rows on the basket (a `Custom` drop target), or drop
//! files from Explorer on either list or the basket.

use std::cell::Cell;

use win32ui::column;
use win32ui::gdi::{Canvas, TextFormat};
use win32ui::prelude::*;
use win32ui::row;

use super::{Msg, StatusWriter};

/// One draggable row.
#[derive(Clone)]
pub(super) struct Item {
    title: String,
}

/// The rows of one list.
struct Rows(Vec<Item>);

impl ListModel for Rows {
    type Item = Item;

    fn len(&self) -> usize {
        self.0.len()
    }

    fn get(&self, index: usize) -> Option<&Item> {
        self.0.as_slice().get(index)
    }
}

/// Where a drag started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Source {
    Library,
    Queue,
}

/// The messages of the Drag tab.
pub(super) enum DndMsg {
    /// The user began dragging these rows of `Source`.
    Begin(Source, Vec<usize>),
    /// Something was dropped on the queue list.
    QueueDrop(ListDrop),
    /// A payload or files were dropped on the basket.
    BasketDrop(BasketEvent),
}

/// What the basket received.
pub(super) enum BasketEvent {
    Payload(Vec<u8>),
    Files(usize),
}

/// A custom drop target: it lights up while a drag is over it, and counts what
/// it received.
struct Basket {
    hot: Cell<bool>,
    count: Cell<usize>,
}

impl CustomWidget for Basket {
    type Event = BasketEvent;

    fn paint(&self, canvas: &Canvas, bounds: Rect, theme: &Theme) {
        canvas.fill_rect(bounds, theme.background);
        canvas.outline(
            bounds,
            if self.hot.get() {
                theme.accent
            } else {
                theme.border
            },
        );
        let text = match (self.hot.get(), self.count.get()) {
            (true, _) => "Drop here".to_string(),
            (false, 0) => "Basket: drop rows or files".to_string(),
            (false, count) => format!("Basket: {count} received"),
        };
        canvas.draw_text(
            bounds,
            &text,
            theme.text,
            TextFormat::left()
                .center()
                .vcenter()
                .single_line()
                .no_prefix(),
        );
    }

    fn drag(&self, event: DragEvent<'_>, cx: &mut WidgetCx<BasketEvent>) -> DropEffect {
        match event {
            DragEvent::Enter(info) | DragEvent::Over(info) => {
                let accepted = info.data.has_payload() || info.data.has_files();
                if self.hot.replace(accepted) != accepted {
                    cx.invalidate();
                }
                if accepted {
                    DropEffect::Copy
                } else {
                    DropEffect::None
                }
            }
            DragEvent::Leave => {
                self.hot.set(false);
                cx.invalidate();
                DropEffect::None
            }
            DragEvent::Drop(info) => {
                self.hot.set(false);
                cx.invalidate();
                if let Some(payload) = info.data.payload() {
                    cx.emit(BasketEvent::Payload(payload));
                } else {
                    let files = info.data.files();
                    if files.is_empty() {
                        return DropEffect::None;
                    }
                    cx.emit(BasketEvent::Files(files.len()));
                }
                DropEffect::Copy
            }
        }
    }
}

/// The Drag tab's widgets and rows.
pub(super) struct DndTab {
    library: ListView<Item, Msg>,
    queue: ListView<Item, Msg>,
    basket: Custom<Basket, Msg>,
    caption: Label,
    queue_rows: Vec<Item>,
}

/// The drag payload: `L` or `Q` for the source list, then the dragged rows as
/// comma-separated indices. It is opaque to win32ui.
fn encode(source: Source, rows: &[usize]) -> Vec<u8> {
    let tag = if source == Source::Library { 'L' } else { 'Q' };
    let list: Vec<String> = rows.iter().map(usize::to_string).collect();
    format!("{tag}{}", list.join(",")).into_bytes()
}

fn decode(payload: &[u8]) -> Option<(Source, Vec<usize>)> {
    let text = std::str::from_utf8(payload).ok()?;
    let source = match text.chars().next()? {
        'L' => Source::Library,
        'Q' => Source::Queue,
        _ => return None,
    };
    let rows = text[1..]
        .split(',')
        .filter_map(|row| row.parse().ok())
        .collect();
    Some((source, rows))
}

fn library_rows() -> Vec<Item> {
    [
        "Aurora", "Borealis", "Cascade", "Dusk", "Ember", "Flux", "Glacier", "Harbor",
    ]
    .iter()
    .map(|title| Item {
        title: (*title).to_string(),
    })
    .collect()
}

impl DndTab {
    pub(super) fn build(ui: &mut Ui<Msg>) -> DndTab {
        let library = ListView::new(ui)
            .expect("library list")
            .column("Library (drag me)", Fill, |row: &Item| row.title.as_str())
            .multi_select(true)
            .on_begin_drag(|rows, _| Some(Msg::Dnd(DndMsg::Begin(Source::Library, rows.to_vec()))));
        library.set_model(Rows(library_rows()));

        let queue = ListView::new(ui)
            .expect("queue list")
            .column("Queue (drop, reorder)", Fill, |row: &Item| {
                row.title.as_str()
            })
            .multi_select(true)
            .on_begin_drag(|rows, _| Some(Msg::Dnd(DndMsg::Begin(Source::Queue, rows.to_vec()))))
            .on_drop(|drop| Some(Msg::Dnd(DndMsg::QueueDrop(drop))))
            .expect("queue drop target");
        // `WIN32UI_DEMO_DND_MARK=1` starts with queued rows and the insertion
        // marker shown, so a screenshot run can capture the drag feedback.
        let mark_for_screenshot = std::env::var_os("WIN32UI_DEMO_DND_MARK").is_some();
        let queue_rows = if mark_for_screenshot {
            library_rows().into_iter().take(4).collect()
        } else {
            Vec::new()
        };
        queue.set_model(Rows(queue_rows.clone()));
        if mark_for_screenshot {
            queue.set_insert_mark(Some((2, false)));
        }

        let basket = Custom::new(
            ui,
            Basket {
                hot: Cell::new(false),
                count: Cell::new(0),
            },
        )
        .expect("basket")
        .on_event(|event| Some(Msg::Dnd(DndMsg::BasketDrop(event))))
        .accept_drops()
        .expect("basket drop target");

        DndTab {
            library,
            queue,
            basket,
            caption: Label::new(ui, Rect::default(), "Drag rows between the lists")
                .expect("drag caption"),
            queue_rows,
        }
    }

    pub(super) fn page(&self) -> Layout {
        column![
            self.caption.height(dip(20.0)),
            row![
                self.library.fill(1),
                self.queue.fill(1),
                self.basket.width(dip(200.0)),
            ]
            .spacing(dip(6.0))
            .fill(1),
        ]
        .spacing(dip(6.0))
    }

    /// Handles the tab's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg, status: &dyn StatusWriter) -> bool {
        let Msg::Dnd(msg) = msg else {
            return false;
        };
        match msg {
            DndMsg::Begin(source, rows) => {
                let list = match source {
                    Source::Library => &self.library,
                    Source::Queue => &self.queue,
                };
                let effects = DropEffects::COPY | DropEffects::MOVE;
                let effect = list.begin_drag(&encode(*source, rows), effects, None);
                status.set_text(0, &format!("Drag ended: {effect:?}"));
            }
            DndMsg::QueueDrop(drop) => self.queue_drop(drop, status),
            DndMsg::BasketDrop(event) => {
                let received = match event {
                    BasketEvent::Payload(payload) => decode(payload).map_or(1, |(_, r)| r.len()),
                    BasketEvent::Files(count) => *count,
                };
                let total = self.basket.widget().borrow().count.get() + received;
                self.basket.widget().borrow().count.set(total);
                self.basket.invalidate();
                status.set_text(0, &format!("Basket received {received}"));
            }
        }
        true
    }

    fn queue_drop(&mut self, drop: &ListDrop, status: &dyn StatusWriter) {
        let mut at = drop.index;
        let dropped: Vec<Item> = match drop.payload.as_deref().and_then(decode) {
            Some((Source::Library, rows)) => {
                let library = library_rows();
                rows.iter()
                    .filter_map(|row| library.as_slice().get(*row).cloned())
                    .collect()
            }
            Some((Source::Queue, mut rows)) => {
                // A reorder: lift the dragged rows out, then insert them where
                // the marker was, shifted by the ones that sat above it.
                rows.sort_unstable();
                rows.dedup();
                let lifted: Vec<Item> = rows
                    .iter()
                    .filter_map(|row| self.queue_rows.as_slice().get(*row).cloned())
                    .collect();
                at -= rows.iter().filter(|row| **row < at).count();
                for row in rows.iter().rev() {
                    if *row < self.queue_rows.len() {
                        self.queue_rows.remove(*row);
                    }
                }
                lifted
            }
            None => drop
                .files
                .iter()
                .filter_map(|path| path.file_name())
                .map(|name| Item {
                    title: name.to_string_lossy().into_owned(),
                })
                .collect(),
        };
        let count = dropped.len();
        let at = at.min(self.queue_rows.len());
        self.queue_rows.splice(at..at, dropped);
        self.queue.set_model(Rows(self.queue_rows.clone()));
        self.queue
            .set_selection(&(at..at + count).collect::<Vec<_>>());
        status.set_text(0, &format!("Queue: {count} row(s) at {at}"));
    }
}
