//! Clipboard round-trips through a real window. If the session cannot open the
//! clipboard (a headless CI agent, for instance), the test skips rather than
//! fails.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::run_with_watchdog;
use win32ui::prelude::*;

/// Text across the UTF-16 boundary, plus a lone LF that must come back as CRLF.
const SAMPLES: &[&str] = &[
    "hello",
    "🎵 Émoji 🎶 𝄞",
    "日本語のアルバム",
    "e\u{301}\u{327} combining",
    "🅰🅱🆎 astral",
    "line one\nline two\nline three",
];

enum Outcome {
    Unavailable,
    Passed,
    Failed(String),
}

struct ClipboardHandler {
    outcome: Rc<RefCell<Option<Outcome>>>,
}

impl WindowHandler for ClipboardHandler {
    fn message(&self, window: &Window, message: Message) -> Option<LResult> {
        if let Message::Create = message {
            *self.outcome.borrow_mut() = Some(round_trip(window.hwnd()));
            win32ui::quit(0);
            return Some(0);
        }
        None
    }
}

fn round_trip(owner: Hwnd) -> Outcome {
    // `text` needs the clipboard open too, so a failure here means this session
    // has no usable clipboard: skip instead of failing.
    if win32ui::clipboard::text(owner).is_err() {
        return Outcome::Unavailable;
    }

    for sample in SAMPLES {
        if let Err(error) = win32ui::clipboard::set_text(owner, sample) {
            return Outcome::Failed(format!("set_text({sample:?}): {error}"));
        }
        let read = match win32ui::clipboard::text(owner) {
            Ok(Some(text)) => text,
            Ok(None) => return Outcome::Failed(format!("no text after writing {sample:?}")),
            Err(error) => return Outcome::Failed(format!("text(): {error}")),
        };
        // Assert against the clipboard convention, not the implementation.
        let expected = sample.replace('\n', "\r\n");
        if read != expected {
            return Outcome::Failed(format!(
                "round-trip mismatch: wrote {sample:?}, expected {expected:?}, read {read:?}"
            ));
        }
    }
    Outcome::Passed
}

#[test]
fn unicode_text_round_trips() {
    let outcome = Rc::new(RefCell::new(None));
    let outcome_for_handler = Rc::clone(&outcome);

    let Some(run) = run_with_watchdog("win32ui.clipboard", move || ClipboardHandler {
        outcome: outcome_for_handler,
    }) else {
        return;
    };

    assert!(
        !run.timed_out,
        "the watchdog fired before the clipboard ran"
    );
    match outcome.borrow_mut().take() {
        Some(Outcome::Passed) => {}
        Some(Outcome::Unavailable) => {
            eprintln!("skipping clipboard test: no clipboard in this session");
        }
        Some(Outcome::Failed(error)) => panic!("clipboard round-trip failed: {error}"),
        None => panic!("the clipboard handler never ran"),
    }
}
