//! `Shortcut` display text and parsing.

#![cfg(windows)]

use win32ui::prelude::*;

#[test]
fn display_renders_modifiers_in_order() {
    assert_eq!(Shortcut::ctrl(Key::N).to_string(), "Ctrl+N");
    assert_eq!(
        Shortcut::ctrl(Key::N).with_shift().to_string(),
        "Ctrl+Shift+N"
    );
    assert_eq!(Shortcut::alt(Key::F4).to_string(), "Alt+F4");
    assert_eq!(Shortcut::win(Key::D).to_string(), "Win+D");
    assert_eq!(Shortcut::new(Key::F5, Modifiers::NONE).to_string(), "F5");
    assert_eq!(
        Shortcut::new(Key::RETURN, Modifiers::NONE).to_string(),
        "Enter"
    );
    assert_eq!(Shortcut::new(Key::DIGIT1, Modifiers::NONE).to_string(), "1");
}

#[test]
fn parsing_round_trips_display_text() {
    for text in [
        "Ctrl+N",
        "Ctrl+Shift+S",
        "Alt+F4",
        "F5",
        "Ctrl+1",
        "Esc",
        "Enter",
        "PgDn",
    ] {
        let parsed: Shortcut = text.parse().expect(text);
        assert_eq!(parsed.to_string(), text, "round trip failed for {text}");
    }
}

#[test]
fn parsing_is_case_insensitive_and_accepts_aliases() {
    assert_eq!(
        "control+shift+n".parse::<Shortcut>().unwrap(),
        Shortcut::ctrl(Key::N).with_shift()
    );
    assert_eq!(
        "ESCAPE".parse::<Shortcut>().unwrap(),
        Shortcut::new(Key::ESCAPE, Modifiers::NONE)
    );
    assert_eq!(
        "page_up".parse::<Shortcut>().unwrap(),
        Shortcut::new(Key::PAGE_UP, Modifiers::NONE)
    );
}

#[test]
fn parsing_rejects_unknown_or_empty_parts() {
    assert!("Ctrl+".parse::<Shortcut>().is_err());
    assert!("Nope".parse::<Shortcut>().is_err());
    assert!("Ctrl+N+M".parse::<Shortcut>().is_err());
    assert!("".parse::<Shortcut>().is_err());
}

#[test]
fn unknown_keys_fall_back_to_their_code() {
    let unknown = Key::from_code(0x00FF);
    assert_eq!(Shortcut::new(unknown, Modifiers::NONE).to_string(), "VKFF");
}
