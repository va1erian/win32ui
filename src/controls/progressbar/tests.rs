use super::*;

fn state() -> ProgressBarState {
    ProgressBarState {
        theme: ProgressBarTheme::from_theme(&Theme::light()),
        min: 0,
        max: 100,
        value: 0,
        state: ProgressState::Normal,
        marquee: false,
        offset: 0.0,
        timer: None,
        bounds: Rect::new(0, 0, 200, 8),
    }
}

#[test]
fn range_is_normalised_and_clamps_value() {
    let mut state = state();
    state.set_value(50);
    let (high, low) = (100, 0);
    state.set_range(high..=low);
    assert_eq!((state.min, state.max), (0, 100));
    state.set_range(10..=20);
    assert_eq!(state.value, 20);
}

#[test]
fn value_is_clamped_to_range() {
    let mut state = state();
    state.set_range(0..=10);
    state.set_value(25);
    assert_eq!(state.value, 10);
    state.set_value(-5);
    assert_eq!(state.value, 0);
}

#[test]
fn value_fill_scales_with_the_fraction() {
    let mut state = state();
    state.set_range(0..=100);
    state.set_value(50);
    let fill = state.value_fill().expect("a mid value fills");
    assert_eq!(fill.width(), 100);
    state.set_value(0);
    assert!(state.value_fill().is_none());
}

#[test]
fn marquee_advances_and_wraps() {
    let mut state = state();
    state.marquee = true;
    state.advance_marquee();
    assert!(state.offset > 0.0);
    state.offset = 1.0;
    state.advance_marquee();
    assert_eq!(state.offset, 0.0);
}

#[test]
fn state_selects_the_fill_colour() {
    let theme = Theme::dark();
    let mut state = state();
    state.theme = ProgressBarTheme::from_theme(&theme);
    for (progress, expected) in [
        (ProgressState::Normal, theme.accent),
        (ProgressState::Paused, theme.warning),
        (ProgressState::Error, theme.danger),
    ] {
        state.state = progress;
        assert_eq!(state.fill_color(), expected);
    }
}
