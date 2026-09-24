use super::*;

fn pack(x: i32, y: i32) -> isize {
    (x as u16 as isize) | ((y as u16 as isize) << 16)
}

#[test]
fn key_down_carries_key_repeat_and_modifiers() {
    let modifiers = Modifiers {
        ctrl: true,
        ..Modifiers::NONE
    };
    let message = decode_input(WM_KEYDOWN, Key::F5.code() as usize, 3, modifiers).unwrap();
    assert!(matches!(
        message,
        Message::KeyDown { key, modifiers: m, repeat: 3, system: false }
            if key == Key::F5 && m.ctrl
    ));
}

#[test]
fn system_key_is_flagged() {
    let message = decode_input(WM_SYSKEYDOWN, Key::MENU.code() as usize, 1, Modifiers::NONE);
    assert!(matches!(
        message,
        Some(Message::KeyDown { system: true, .. })
    ));
    let up = decode_input(WM_SYSKEYUP, Key::MENU.code() as usize, 1, Modifiers::NONE);
    assert!(matches!(up, Some(Message::KeyUp { system: true, .. })));
}

#[test]
fn key_up_decodes() {
    let message = decode_input(WM_KEYUP, Key::ESCAPE.code() as usize, 1, Modifiers::NONE);
    assert!(matches!(message, Some(Message::KeyUp { key, .. }) if key == Key::ESCAPE));
}

#[test]
fn every_button_decodes_on_down_and_up() {
    let cases = [
        (WM_LBUTTONDOWN, WM_LBUTTONUP, MouseButton::Left),
        (WM_RBUTTONDOWN, WM_RBUTTONUP, MouseButton::Right),
        (WM_MBUTTONDOWN, WM_MBUTTONUP, MouseButton::Middle),
    ];
    for (down, up, button) in cases {
        assert!(matches!(
            decode_input(down, 0, pack(4, 5), Modifiers::NONE),
            Some(Message::MouseDown { x: 4, y: 5, button: b, .. }) if b == button
        ));
        assert!(matches!(
            decode_input(up, 0, pack(4, 5), Modifiers::NONE),
            Some(Message::MouseUp { button: b, .. }) if b == button
        ));
    }
}

#[test]
fn mouse_down_carries_ctrl_and_shift_from_wparam() {
    let wparam = MK_CONTROL.0 as usize | MK_SHIFT.0 as usize;
    let message = decode_input(WM_LBUTTONDOWN, wparam, pack(4, 5), Modifiers::NONE).unwrap();
    assert!(matches!(
        message,
        Message::MouseDown { modifiers: m, .. } if m.ctrl && m.shift
    ));
}

#[test]
fn mouse_move_keeps_alt_and_win_from_key_state_but_ctrl_shift_from_wparam() {
    let keys = Modifiers {
        alt: true,
        win: true,
        ..Modifiers::NONE
    };
    let message = decode_input(WM_MOUSEMOVE, MK_CONTROL.0 as usize, pack(1, 2), keys).unwrap();
    assert!(matches!(
        message,
        Message::MouseMove {
            modifiers: Modifiers {
                ctrl: true,
                shift: false,
                alt: true,
                win: true,
            },
            ..
        }
    ));
}

#[test]
fn mouse_up_without_modifier_flags_decodes_no_modifiers() {
    let message = decode_input(WM_LBUTTONUP, 0, pack(4, 5), Modifiers::NONE).unwrap();
    assert!(matches!(
        message,
        Message::MouseUp {
            modifiers: Modifiers::NONE,
            ..
        }
    ));
}

#[test]
fn extra_buttons_decode_from_hiword() {
    let x1 = (XBUTTON1 as usize) << 16;
    let x2 = (XBUTTON2 as usize) << 16;
    assert!(matches!(
        decode_input(WM_XBUTTONDOWN, x1, 0, Modifiers::NONE),
        Some(Message::MouseDown {
            button: MouseButton::X1,
            ..
        })
    ));
    assert!(matches!(
        decode_input(WM_XBUTTONUP, x2, 0, Modifiers::NONE),
        Some(Message::MouseUp {
            button: MouseButton::X2,
            ..
        })
    ));
    assert!(decode_input(WM_XBUTTONDOWN, 0, 0, Modifiers::NONE).is_none());
}

#[test]
fn double_click_decodes() {
    assert!(matches!(
        decode_input(WM_LBUTTONDBLCLK, 0, pack(7, 8), Modifiers::NONE),
        Some(Message::MouseDoubleClick {
            x: 7,
            y: 8,
            button: MouseButton::Left,
            ..
        })
    ));
}

#[test]
fn wheel_carries_delta_and_direction() {
    let wparam = (0xffff_u16 as usize) << 16; // -1 rotation
    let message = decode_input(WM_MOUSEWHEEL, wparam, pack(11, 12), Modifiers::NONE).unwrap();
    assert!(matches!(
        message,
        Message::MouseWheel {
            delta: -1,
            horizontal: false,
            x: 11,
            y: 12,
            ..
        }
    ));
    let horizontal = decode_input(WM_MOUSEHWHEEL, 120 << 16, pack(1, 2), Modifiers::NONE);
    assert!(matches!(
        horizontal,
        Some(Message::MouseWheel {
            delta: 120,
            horizontal: true,
            ..
        })
    ));
}

#[test]
fn focus_and_activation_decode() {
    assert!(matches!(
        decode_input(WM_SETFOCUS, 0, 0, Modifiers::NONE),
        Some(Message::SetFocus)
    ));
    assert!(matches!(
        decode_input(WM_KILLFOCUS, 0, 0, Modifiers::NONE),
        Some(Message::KillFocus)
    ));
    let active = decode_input(WM_ACTIVATE, 1, 0, Modifiers::NONE);
    assert!(matches!(
        active,
        Some(Message::Activate {
            active: true,
            minimized: false
        })
    ));
    let minimized = decode_input(
        WM_ACTIVATE,
        (1 << 16) | WA_INACTIVE as usize,
        0,
        Modifiers::NONE,
    );
    assert!(matches!(
        minimized,
        Some(Message::Activate {
            active: false,
            minimized: true
        })
    ));
}

#[test]
fn cursor_hit_test_decodes() {
    let message = decode_input(WM_SETCURSOR, 0, 1, Modifiers::NONE);
    assert!(matches!(
        message,
        Some(Message::SetCursor {
            hit_test: HitTest::Client
        })
    ));
    let other = decode_input(WM_SETCURSOR, 0, 0xffff, Modifiers::NONE);
    assert!(matches!(
        other,
        Some(Message::SetCursor {
            hit_test: HitTest::Other(0xffff)
        })
    ));
}

#[test]
fn context_menu_distinguishes_keyboard_invocation() {
    let mouse = decode_input(WM_CONTEXTMENU, 0, pack(30, 40), Modifiers::NONE);
    assert!(matches!(
        mouse,
        Some(Message::ContextMenu {
            position: Some(Point { x: 30, y: 40 })
        })
    ));
    let keyboard = decode_input(WM_CONTEXTMENU, 0, pack(-1, -1), Modifiers::NONE);
    assert!(matches!(
        keyboard,
        Some(Message::ContextMenu { position: None })
    ));
}

#[test]
fn session_and_capture_decode() {
    assert!(matches!(
        decode_input(WM_QUERYENDSESSION, 0, 0, Modifiers::NONE),
        Some(Message::QueryEndSession)
    ));
    assert!(matches!(
        decode_input(WM_ENDSESSION, 1, 0, Modifiers::NONE),
        Some(Message::EndSession { ending: true })
    ));
    assert!(matches!(
        decode_input(WM_CAPTURECHANGED, 0, 0, Modifiers::NONE),
        Some(Message::CaptureChanged)
    ));
    assert!(matches!(
        decode_input(WM_MOUSELEAVE, 0, 0, Modifiers::NONE),
        Some(Message::MouseLeave)
    ));
}

#[test]
fn non_input_messages_are_not_claimed() {
    assert!(decode_input(0x1234, 0, 0, Modifiers::NONE).is_none());
}

#[test]
fn char_decodes_bmp() {
    let mut pending = None;
    assert_eq!(decode_char(0x0041, &mut pending), Some('A'));
    assert_eq!(pending, None);
}

#[test]
fn char_combines_surrogate_pairs() {
    let mut pending = None;
    assert_eq!(decode_char(0xd83d, &mut pending), None);
    assert_eq!(pending, Some(0xd83d));
    assert_eq!(decode_char(0xde00, &mut pending), Some('\u{1f600}'));
    assert_eq!(pending, None);
}

#[test]
fn char_replaces_lone_surrogates() {
    let mut pending = None;
    assert_eq!(
        decode_char(0xdc00, &mut pending),
        Some(char::REPLACEMENT_CHARACTER)
    );
    assert_eq!(decode_char(0xd83d, &mut pending), None);
    assert_eq!(decode_char(0x0041, &mut pending), Some('A'));
}
