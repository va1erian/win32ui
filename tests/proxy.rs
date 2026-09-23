//! Proxy tests: worker threads feed the app through `Ui::proxy`, and sending
//! after the window is gone hands the message back instead of panicking.
//!
//! The wake-coalescing count itself (a burst posts one wake) is covered by the
//! in-crate unit tests in `src/app/proxy.rs`, which observe the post decision
//! directly; these integration tests prove every sent message still arrives
//! exactly once, in per-thread order.

#![cfg(windows)]

mod common;

use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;

use common::run_app_with_watchdog;
use win32ui::prelude::*;

const THREADS: usize = 8;
const PER_THREAD: usize = 250;

#[derive(Debug)]
enum Msg {
    Start,
    Work { thread: usize, seq: usize },
}

struct ProxyApp {
    received: Arc<Mutex<Vec<(usize, usize)>>>,
    expected: usize,
}

impl App for ProxyApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Start => {
                let proxy = ui.proxy();
                for thread in 0..THREADS {
                    let proxy = proxy.clone();
                    thread::spawn(move || {
                        for seq in 0..PER_THREAD {
                            // The window stays alive until every message has
                            // arrived, so these sends succeed; ignore a late
                            // failure rather than panic the worker.
                            let _ = proxy.send(Msg::Work { thread, seq });
                        }
                    });
                }
            }
            Msg::Work { thread, seq } => {
                let mut received = self.received.lock().unwrap();
                received.push((thread, seq));
                if received.len() == self.expected {
                    ui.quit();
                }
            }
        }
    }
}

/// Many threads x many sends: every message arrives exactly once, and each
/// thread's messages arrive in the order it sent them.
#[test]
fn proxy_delivers_from_many_threads() {
    let expected = THREADS * PER_THREAD;
    let received: Arc<Mutex<Vec<(usize, usize)>>> = Arc::new(Mutex::new(Vec::new()));
    let received_for_make = Arc::clone(&received);
    let Some(run) = run_app_with_watchdog("win32ui.proxy.many", move |ui| {
        ui.emit(Msg::Start);
        ProxyApp {
            received: received_for_make,
            expected,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let received = received.lock().unwrap();
    assert_eq!(
        received.len(),
        expected,
        "messages were lost: got {} of {expected}",
        received.len()
    );
    let mut seen = HashSet::new();
    for item in received.iter() {
        assert!(seen.insert(*item), "a message arrived twice: {item:?}");
    }
    for thread in 0..THREADS {
        let seqs: Vec<usize> = received
            .iter()
            .filter(|(t, _)| *t == thread)
            .map(|(_, s)| *s)
            .collect();
        let ordered: Vec<usize> = (0..PER_THREAD).collect();
        assert_eq!(seqs, ordered, "thread {thread} was reordered");
    }
}

/// Sending after the window is gone hands the message back and never panics.
#[test]
fn send_after_close_returns_message() {
    #[derive(Debug)]
    enum CloseMsg {
        Start,
        Late { thread: usize, seq: usize },
    }

    struct CloseApp;

    impl App for CloseApp {
        type Msg = CloseMsg;

        fn update(&mut self, msg: CloseMsg, ui: &mut Ui<CloseMsg>) {
            if matches!(msg, CloseMsg::Start) {
                ui.close();
            }
        }
    }

    let slot: Arc<Mutex<Option<Proxy<CloseMsg>>>> = Arc::new(Mutex::new(None));
    let slot_for_make = Arc::clone(&slot);
    let Some(run) = run_app_with_watchdog("win32ui.proxy.closed", move |ui| {
        *slot_for_make.lock().unwrap() = Some(ui.proxy());
        ui.emit(CloseMsg::Start);
        CloseApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    let proxy = slot
        .lock()
        .unwrap()
        .take()
        .expect("the app stored its proxy");
    match proxy.send(CloseMsg::Late { thread: 3, seq: 7 }) {
        Err(CloseMsg::Late { thread: 3, seq: 7 }) => {}
        Err(other) => panic!("the wrong message came back: {other:?}"),
        Ok(()) => panic!("sending after close must fail"),
    }
    // A clone sees the same closed window, without panicking either.
    assert!(
        proxy.clone().send(CloseMsg::Start).is_err(),
        "sending after close must fail"
    );
}
