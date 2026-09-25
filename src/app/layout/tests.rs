//! Pure tree-to-rects tests: no window is created, so the arithmetic is tested
//! in isolation from Win32.

use std::cell::Cell;
use std::rc::Rc;

use super::split::Split;
use super::tabs::Tabs;
use super::*;
use crate::hwnd::Hwnd;
use crate::units::dip;

/// A fake widget at `bounds`, for pure tree-to-rects tests.
fn leaf(bounds: Rect) -> WidgetHandle {
    WidgetHandle::new(
        Hwnd::NULL,
        Rc::new(Cell::new(bounds)),
        Rc::new(Cell::new(true)),
    )
}

fn hidden(bounds: Rect) -> WidgetHandle {
    WidgetHandle::new(
        Hwnd::NULL,
        Rc::new(Cell::new(bounds)),
        Rc::new(Cell::new(false)),
    )
}

fn widget(handle: WidgetHandle, sizing: Sizing) -> LayoutItem {
    LayoutItem {
        content: Content::Widget(handle),
        sizing,
    }
}

#[test]
fn column_uses_natural_size_and_fills_the_rest() {
    let mut layout = Layout::column();
    layout
        .slots
        .push(widget(leaf(Rect::new(0, 0, 0, 20)), Sizing::Auto));
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(1)));

    let placed = layout.compute(Rect::new(0, 0, 100, 60), 96);
    assert_eq!(placed.len(), 2);
    assert_eq!(placed[0].rect, Rect::new(0, 0, 100, 20));
    assert_eq!(placed[1].rect, Rect::new(0, 20, 100, 60));
}

#[test]
fn row_shares_leftover_by_weight() {
    let mut layout = Layout::row();
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fixed(dip(30.0))));
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(1)));
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(2)));

    let placed = layout.compute(Rect::new(0, 0, 90, 40), 96);
    assert_eq!(placed[0].rect, Rect::new(0, 0, 30, 40));
    assert_eq!(placed[1].rect, Rect::new(30, 0, 50, 40));
    assert_eq!(placed[2].rect, Rect::new(50, 0, 90, 40));
}

#[test]
fn spacing_and_margins_apply() {
    let mut layout = Layout::column()
        .margins(Insets::all(dip(4.0)))
        .spacing(dip(2.0));
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fixed(dip(10.0))));
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(1)));

    let placed = layout.compute(Rect::new(0, 0, 100, 100), 96);
    assert_eq!(placed[0].rect, Rect::new(4, 4, 96, 14));
    assert_eq!(placed[1].rect, Rect::new(4, 16, 96, 96));
}

#[test]
fn a_top_margin_reserves_the_title_bar_strip() {
    // The extended title bar reserves a top strip for the caption buttons and
    // menu bar; the first content row (the toolbar) must start below it, so the
    // two rectangles never intersect.
    let mut layout = Layout::column().margins(Insets::new(dip(0.0), dip(40.0), dip(0.0), dip(0.0)));
    layout
        .slots
        .push(widget(leaf(Rect::new(0, 0, 0, 28)), Sizing::Auto));

    let placed = layout.compute(Rect::new(0, 0, 200, 100), 96);
    assert_eq!(
        placed[0].rect,
        Rect::new(0, 40, 200, 68),
        "the toolbar must start below the reserved strip"
    );
}

#[test]
fn nested_rows_recurse() {
    let mut inner = Layout::row();
    inner
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fixed(dip(20.0))));
    inner
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(1)));

    let mut outer = Layout::column();
    outer
        .slots
        .push(widget(leaf(Rect::new(0, 0, 0, 10)), Sizing::Auto));
    outer.slots.push(LayoutItem {
        content: Content::Nested(Box::new(inner)),
        sizing: Sizing::Fill(1),
    });

    let placed = outer.compute(Rect::new(0, 0, 100, 60), 96);
    assert_eq!(placed.len(), 3);
    assert_eq!(placed[0].rect, Rect::new(0, 0, 100, 10));
    assert_eq!(placed[1].rect, Rect::new(0, 10, 20, 60));
    assert_eq!(placed[2].rect, Rect::new(20, 10, 100, 60));
}

#[test]
fn hidden_widgets_take_no_space() {
    let mut layout = Layout::column();
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fixed(dip(10.0))));
    layout
        .slots
        .push(widget(hidden(Rect::new(0, 0, 0, 50)), Sizing::Auto));
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(1)));

    let placed = layout.compute(Rect::new(0, 0, 100, 60), 96);
    assert_eq!(placed.len(), 2, "the hidden widget must be dropped");
    assert_eq!(placed[0].rect, Rect::new(0, 0, 100, 10));
    assert_eq!(placed[1].rect, Rect::new(0, 10, 100, 60));
}

#[test]
fn a_nested_layout_of_only_hidden_widgets_collapses() {
    let mut hidden_row = Layout::row();
    hidden_row
        .slots
        .push(widget(hidden(Rect::default()), Sizing::Fill(1)));

    let mut layout = Layout::column();
    layout.slots.push(LayoutItem {
        content: Content::Nested(Box::new(hidden_row)),
        sizing: Sizing::Fill(1),
    });
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(1)));

    let placed = layout.compute(Rect::new(0, 0, 100, 60), 96);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].rect, Rect::new(0, 0, 100, 60));
}

#[test]
fn width_and_height_follow_their_axis() {
    // In a row, `.height` constrains the cross axis; the main axis stays the
    // widget's natural width.
    let mut row = Layout::row();
    row.slots.push(widget(
        leaf(Rect::new(0, 0, 30, 0)),
        Sizing::Height(dip(15.0)),
    ));
    row.slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(1)));

    let placed = row.compute(Rect::new(0, 0, 100, 40), 96);
    assert_eq!(placed[0].rect, Rect::new(0, 0, 30, 15));
    assert_eq!(placed[1].rect, Rect::new(30, 0, 100, 40));

    // In a column, `.width` constrains the cross axis.
    let mut column = Layout::column();
    column.slots.push(widget(
        leaf(Rect::new(0, 0, 0, 10)),
        Sizing::Width(dip(25.0)),
    ));
    column
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fill(1)));

    let placed = column.compute(Rect::new(0, 0, 100, 40), 96);
    assert_eq!(placed[0].rect, Rect::new(0, 0, 25, 10));
    assert_eq!(placed[1].rect, Rect::new(0, 10, 100, 40));
}

#[test]
fn macros_build_the_same_tree_as_the_builders() {
    let tree = crate::column![Layout::row(), Layout::row().fill(1)].spacing(dip(4.0));
    assert_eq!(tree.slots.len(), 2);
    assert_eq!(tree.spacing, dip(4.0));
}

fn split_layout(split: Split) -> Layout {
    Layout::row().item(split)
}

#[test]
fn split_row_places_panes_around_an_initial_position() {
    let split = Split::row()
        .a(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .b(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .position(dip(30.0));
    let placed = split_layout(split).compute(Rect::new(0, 0, 100, 40), 96);
    assert_eq!(
        placed.len(),
        2,
        "the divider is not placed without a window"
    );
    assert_eq!(placed[0].rect, Rect::new(0, 0, 30, 40), "first pane");
    assert_eq!(placed[1].rect, Rect::new(35, 0, 100, 40), "second pane");
}

#[test]
fn split_columns_default_to_the_middle() {
    let split = Split::column()
        .a(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .b(widget(leaf(Rect::default()), Sizing::Fill(1)));
    let placed = Layout::column()
        .item(split)
        .compute(Rect::new(0, 0, 40, 205), 96);
    assert_eq!(placed[0].rect, Rect::new(0, 0, 40, 100));
    assert_eq!(placed[1].rect, Rect::new(0, 105, 40, 205));
}

#[test]
fn split_minimums_clamp_the_position() {
    let split = Split::row()
        .a(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .b(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .position(dip(5.0))
        .min(dip(20.0), dip(30.0));
    let placed = split_layout(split).compute(Rect::new(0, 0, 100, 40), 96);
    assert_eq!(
        placed[0].rect,
        Rect::new(0, 0, 20, 40),
        "clamped up to min_a"
    );

    let split = Split::row()
        .a(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .b(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .position(dip(95.0))
        .min(dip(20.0), dip(30.0));
    let placed = split_layout(split).compute(Rect::new(0, 0, 100, 40), 96);
    assert_eq!(
        placed[0].rect,
        Rect::new(0, 0, 65, 40),
        "clamped down to leave min_b and the divider"
    );
    assert_eq!(placed[1].rect, Rect::new(70, 0, 100, 40));
}

#[test]
fn split_position_b_anchors_the_second_pane() {
    let split = Split::row()
        .a(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .b(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .position_b(dip(30.0));
    let placed = split_layout(split).compute(Rect::new(0, 0, 100, 40), 96);
    assert_eq!(
        placed[0].rect,
        Rect::new(0, 0, 65, 40),
        "the first pane takes the space the anchored second pane leaves"
    );
    assert_eq!(placed[1].rect, Rect::new(70, 0, 100, 40), "second pane");
    assert_eq!(
        placed[1].rect.width(),
        30,
        "the second pane keeps its width"
    );
}

#[test]
fn split_position_b_clamps_the_second_pane() {
    let split = Split::column()
        .a(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .b(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .position_b(dip(5.0))
        .min(dip(20.0), dip(30.0));
    let placed = Layout::column()
        .item(split)
        .compute(Rect::new(0, 0, 40, 205), 96);
    assert_eq!(
        placed[0].rect.height(),
        170,
        "the first pane keeps min_a as the second is clamped up to min_b"
    );
    assert_eq!(placed[1].rect, Rect::new(0, 175, 40, 205), "second pane");
}

#[test]
fn split_collapses_to_the_visible_pane() {
    let split = Split::row()
        .a(widget(hidden(Rect::default()), Sizing::Fill(1)))
        .b(widget(leaf(Rect::default()), Sizing::Fill(1)))
        .position(dip(30.0));
    let placed = split_layout(split).compute(Rect::new(0, 0, 100, 40), 96);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].rect, Rect::new(0, 0, 100, 40));
}

#[test]
fn auto_slots_do_not_ratchet_under_overflow() {
    // An over-subscribed column: an `Auto` widget (natural height 100) and two
    // fixed slots. Overflow shrinking resizes the widget; without a cached
    // natural size its shrunken height becomes the next pass's natural size,
    // which shrinks it again — a one-pixel creep on every relayout.
    let mut layout = Layout::column();
    layout
        .slots
        .push(widget(leaf(Rect::new(0, 0, 0, 100)), Sizing::Auto));
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fixed(dip(90.0))));
    layout
        .slots
        .push(widget(leaf(Rect::default()), Sizing::Fixed(dip(90.0))));

    let area = Rect::new(0, 0, 100, 100);
    let mut rect = None;
    for _ in 0..4 {
        let placed = layout.compute(area, 96);
        if let Some(previous) = rect {
            assert_eq!(
                previous, placed[0].rect,
                "an Auto slot must not shrink on every relayout"
            );
        }
        rect = Some(placed[0].rect);
        for placed in &placed {
            placed.handle.set_bounds(placed.rect);
        }
    }
}

#[test]
fn tabs_show_only_the_selected_page() {
    let on_page = leaf(Rect::default());
    let off_page = leaf(Rect::default());
    let on_visible = Rc::clone(&on_page.visible);
    let off_visible = Rc::clone(&off_page.visible);

    let tabs = Tabs::new()
        .page("One", widget(on_page, Sizing::Fill(1)))
        .page("Two", widget(off_page, Sizing::Fill(1)));
    let tree = Layout::row().item(tabs);

    let placed = tree.compute(Rect::new(0, 0, 200, 100), 96);
    assert_eq!(placed.len(), 1, "only the selected page is laid out");
    assert_eq!(
        placed[0].rect,
        Rect::new(0, 0, 200, 100),
        "page fills the node"
    );
    assert!(on_visible.get(), "the selected page is shown");
    assert!(!off_visible.get(), "the unselected page is hidden");
}

#[test]
fn tabs_honour_the_initial_selection() {
    let first = leaf(Rect::default());
    let second = leaf(Rect::default());
    let second_visible = Rc::clone(&second.visible);

    let tabs = Tabs::new()
        .page("One", widget(first, Sizing::Fill(1)))
        .page("Two", widget(second, Sizing::Fill(1)))
        .initial(1);
    let placed = Layout::row()
        .item(tabs)
        .compute(Rect::new(0, 0, 200, 100), 96);

    assert_eq!(placed.len(), 1);
    assert!(second_visible.get(), "the initially selected page is shown");
}

#[test]
fn tabs_macro_builds_pages_in_order() {
    let tabs = crate::tabs![
        ("One", widget(leaf(Rect::default()), Sizing::Fill(1))),
        ("Two", widget(leaf(Rect::default()), Sizing::Fill(1))),
    ];
    let node = (&tabs).into_layout_item();
    let Content::Tabs(node) = node.content else {
        panic!("expected a tabs node");
    };
    assert_eq!(node.page_count(), 2);
}

#[test]
fn split_row_macro_builds_a_two_pane_node() {
    let a = LayoutItem {
        content: Content::Nested(Box::new(Layout::row())),
        sizing: Sizing::Fill(1),
    };
    let b = LayoutItem {
        content: Content::Nested(Box::new(Layout::row())),
        sizing: Sizing::Fill(1),
    };
    let tree = crate::column![crate::split_row![a, b].position(dip(10.0))];
    assert_eq!(tree.slots.len(), 1);
    let placed = tree.compute(Rect::new(0, 0, 50, 20), 96);
    assert!(placed.is_empty(), "empty panes have no leaves");
}
