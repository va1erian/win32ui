//! A tiny input example: every decoded keyboard/mouse/focus message is echoed
//! in the window title. Move the mouse, click, scroll, type and press Escape
//! (or close the window) to quit.
//!
//! Run with:
//!
//! ```text
//! cargo run -p win32ui --example input
//! ```
//!
//! `WIN32UI_DEMO_AUTOCLOSE_MS` makes it quit itself, for headless runs.

#[cfg(windows)]
mod input {
    use std::cell::Cell;

    use win32ui::prelude::*;

    pub(crate) fn main() {
        win32ui::init();

        let theme = Theme::dark();
        let Ok(class) = WindowClass::register("win32ui.input", theme.background) else {
            return;
        };
        let Ok(window) = Window::create(
            class,
            None,
            WindowStyle::overlapped(),
            WindowExStyle::new(),
            Rect::new(80, 80, 720, 420),
            "win32ui input — move, click, scroll, type; Esc quits",
            App::default(),
        ) else {
            return;
        };

        window.show();
        win32ui::run();
        window.destroy();
    }

    #[derive(Default)]
    struct App {
        auto_close: Cell<Option<TimerId>>,
    }

    impl WindowHandler for App {
        fn message(&self, window: &Window, message: Message) -> Option<LResult> {
            let description = match message {
                Message::Create => {
                    if let Ok(millis) = std::env::var("WIN32UI_DEMO_AUTOCLOSE_MS") {
                        self.auto_close
                            .set(window.set_timer(millis.parse().unwrap_or(1500)).ok());
                    }
                    return Some(0);
                }
                Message::Close => {
                    window.destroy();
                    win32ui::quit(0);
                    return Some(0);
                }
                Message::Timer { id } if self.auto_close.get() == Some(id) => {
                    window.destroy();
                    win32ui::quit(0);
                    return Some(0);
                }
                Message::KeyDown {
                    key,
                    modifiers,
                    repeat,
                    system,
                } => {
                    if key == Key::ESCAPE && !system {
                        window.destroy();
                        win32ui::quit(0);
                        return Some(0);
                    }
                    format!("KeyDown {key:?} mods={modifiers:?} repeat={repeat} system={system}")
                }
                Message::KeyUp { key, modifiers, .. } => {
                    format!("KeyUp {key:?} mods={modifiers:?}")
                }
                Message::Char(character) => format!("Char {character:?}"),
                Message::MouseMove { x, y } => {
                    // Re-arm every move: `WM_MOUSELEAVE` is one-shot.
                    let _ = window.track_mouse_leave();
                    format!("MouseMove ({x}, {y})")
                }
                Message::MouseLeave => "MouseLeave".to_string(),
                Message::MouseDown { button, x, y } => {
                    format!("MouseDown {button:?} ({x}, {y})")
                }
                Message::MouseUp { button, x, y } => format!("MouseUp {button:?} ({x}, {y})"),
                Message::MouseDoubleClick { button, x, y } => {
                    format!("MouseDoubleClick {button:?} ({x}, {y})")
                }
                Message::MouseWheel {
                    delta,
                    horizontal,
                    x,
                    y,
                    modifiers,
                } => format!(
                    "MouseWheel delta={delta} horizontal={horizontal} ({x}, {y}) mods={modifiers:?}"
                ),
                Message::SetFocus => "SetFocus".to_string(),
                Message::KillFocus => "KillFocus".to_string(),
                Message::Activate { active, minimized } => {
                    format!("Activate active={active} minimized={minimized}")
                }
                Message::CaptureChanged => "CaptureChanged".to_string(),
                Message::SetCursor { hit_test } => format!("SetCursor {hit_test:?}"),
                Message::ContextMenu { position } => format!("ContextMenu {position:?}"),
                Message::SettingChange { section } => {
                    format!("SettingChange section={section:?}")
                }
                _ => return None,
            };

            let _ = window.set_title(&description);
            Some(0)
        }
    }
}

#[cfg(windows)]
fn main() {
    input::main();
}

#[cfg(not(windows))]
fn main() {}
