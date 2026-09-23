//! The slider state machine, exercised without a window.

use super::*;

/// A 224 dip wide slider: 200 dip of track between the paddings.
fn axis() -> Axis {
    Axis::new(224.0, false)
}

fn seek() -> SliderState {
    SliderState::new(0.0, 36_000.0)
}

#[test]
fn the_axis_maps_both_ends_and_the_midpoint() {
    let axis = axis();
    assert_eq!(axis.fraction_at(END_PADDING), 0.0);
    assert_eq!(axis.fraction_at(END_PADDING + 100.0), 0.5);
    assert_eq!(axis.fraction_at(END_PADDING + 200.0), 1.0);
    assert_eq!(axis.pos_of(0.5), END_PADDING + 100.0);
}

#[test]
fn a_pointer_far_outside_clamps_to_the_ends() {
    let axis = axis();
    assert_eq!(axis.fraction_at(-5000.0), 0.0);
    assert_eq!(axis.fraction_at(90_000.0), 1.0);
}

#[test]
fn a_reversed_axis_puts_the_minimum_at_the_far_end() {
    let axis = Axis::new(224.0, true);
    assert_eq!(axis.fraction_at(END_PADDING), 1.0);
    assert_eq!(axis.fraction_at(END_PADDING + 200.0), 0.0);
    assert_eq!(axis.pos_of(0.0), END_PADDING + 200.0);
}

#[test]
fn a_degenerate_axis_maps_to_the_minimum() {
    assert_eq!(Axis::new(10.0, false).fraction_at(50.0), 0.0);
}

#[test]
fn a_ten_hour_track_resolves_a_single_pixel() {
    let mut state = seek();
    let axis = axis();
    state.press(&axis, END_PADDING + 100.0);
    let before = state.value();
    state.pointer_moved(&axis, END_PADDING + 100.25);
    let step = state.value() - before;
    assert!(step > 0.0 && step < 36_000.0 / 200.0, "sub-pixel: {step}");
}

#[test]
fn pressing_the_track_jumps_and_starts_a_drag() {
    let mut state = seek();
    assert!(state.press(&axis(), END_PADDING + 50.0));
    assert_eq!(state.value(), 9_000.0);
    assert!(state.is_dragging());
    assert_eq!(state.take_pending_change(), Some(9_000.0));
}

#[test]
fn grabbing_the_thumb_does_not_jump_it() {
    let mut state = seek();
    state.set_value(18_000.0);
    let axis = axis();
    let thumb = axis.pos_of(0.5);
    assert!(!state.press(&axis, thumb + 6.0));
    assert_eq!(state.value(), 18_000.0);
    state.pointer_moved(&axis, thumb + 6.0 + 20.0);
    assert_eq!(state.value(), 18_000.0 + 3_600.0);
}

#[test]
fn dragging_far_outside_tracks_the_ends() {
    let mut state = seek();
    let axis = axis();
    state.press(&axis, END_PADDING + 100.0);
    state.pointer_moved(&axis, 1.0e9);
    assert_eq!(state.value(), 36_000.0);
    state.pointer_moved(&axis, -1.0e9);
    assert_eq!(state.value(), 0.0);
}

#[test]
fn moves_between_flushes_coalesce_to_the_last_value() {
    let mut state = seek();
    let axis = axis();
    state.press(&axis, END_PADDING);
    for offset in 1..=50 {
        state.pointer_moved(&axis, END_PADDING + f64::from(offset));
    }
    assert_eq!(state.take_pending_change(), Some(0.25 * 36_000.0));
    assert_eq!(state.take_pending_change(), None);
}

#[test]
fn set_value_is_ignored_while_dragging_and_applied_after() {
    let mut state = seek();
    let axis = axis();
    state.press(&axis, END_PADDING + 100.0);
    assert!(!state.set_value(1.0));
    assert_eq!(state.value(), 18_000.0);
    assert_eq!(state.release(), Some(18_000.0));
    assert!(state.set_value(1.0));
    assert_eq!(state.value(), 1.0);
}

#[test]
fn set_value_clamps_and_never_queues_an_event() {
    let mut state = seek();
    assert!(state.set_value(1.0e9));
    assert_eq!(state.value(), 36_000.0);
    assert!(!state.set_value(f64::NAN));
    assert_eq!(state.take_pending_change(), None);
}

#[test]
fn release_reports_the_commit_value_once() {
    let mut state = seek();
    state.press(&axis(), END_PADDING + 100.0);
    assert_eq!(state.release(), Some(18_000.0));
    assert_eq!(state.release(), None);
}

#[test]
fn steps_move_in_value_units_and_clamp() {
    let mut state = SliderState::new(0.0, 1.0);
    assert!(state.step(Step::Small(0.01), 1.0));
    assert!(state.step(Step::Large(0.1), 1.0));
    assert!((state.value() - 0.11).abs() < 1e-12);
    assert!(state.step(Step::Max, 1.0));
    assert!(!state.step(Step::Small(0.01), 1.0));
    assert!(state.step(Step::Min, 1.0));
    assert_eq!(state.value(), 0.0);
    assert!(!state.step(Step::Wheel(0.05), -1.0));
}

#[test]
fn hover_records_the_value_under_the_pointer() {
    let mut state = seek();
    state.pointer_moved(&axis(), END_PADDING + 100.0);
    assert_eq!(state.take_pending_hover(), Some(18_000.0));
    assert_eq!(state.value(), 0.0);
    state.pointer_left();
    assert!(!state.targets().hover);
}

#[test]
fn a_dragging_slider_targets_hover_and_press() {
    let mut state = seek();
    state.press(&axis(), 40.0);
    let targets = state.targets();
    assert!(targets.hover && targets.press && !targets.focus);
}

#[test]
fn the_focus_ring_shows_only_for_keyboard_focus() {
    let mut state = seek();
    state.set_focus(true, false);
    assert!(!state.targets().focus);
    state.show_focus_ring();
    assert!(state.targets().focus);
    state.set_focus(false, true);
    assert!(!state.targets().focus);
}

#[test]
fn a_disabled_slider_targets_nothing() {
    let mut state = seek();
    state.press(&axis(), 40.0);
    state.disable();
    assert!(!state.is_dragging());
    assert_eq!(state.targets(), Targets::default());
}

#[test]
fn a_reversed_range_is_swapped() {
    let mut state = SliderState::new(10.0, 0.0);
    assert!(state.set_value(20.0));
    assert_eq!(state.value(), 10.0);
}
