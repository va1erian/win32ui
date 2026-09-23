#![forbid(unsafe_code)]

//! Worker-thread access to the widget layer: [`Proxy`].
//!
//! Background work (network, database, rendering) feeds the UI through a
//! [`Proxy`]: [`Ui::proxy`](super::Ui::proxy) returns one, worker threads
//! share it (`Clone`, and `Send + Sync` when the message type is), and
//! [`send`](Proxy::send) pushes onto a thread-safe inbox. The UI thread moves
//! that inbox into the window's own message queue ahead of the drain, so the
//! messages are delivered by the same drain and
//! [`App::update`](super::App::update) is still never re-entered.

use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use crate::hwnd::Hwnd;
use crate::sys;

use super::core::Core;

/// A thread-safe handle for sending an app's messages from worker threads.
///
/// Returned by [`Ui::proxy`](super::Ui::proxy). Cheap to clone: every clone
/// feeds the same window. `Proxy<M>` is `Send + Sync` whenever `M: Send`, so
/// worker threads can hold it across thread boundaries.
///
/// Sending is coalesced: no matter how many sends happen before the UI thread
/// collects the inbox, only one wake is posted.
pub struct Proxy<M> {
    shared: Arc<Shared<M>>,
    hooked: bool,
}

/// The state shared by every clone of one [`Proxy`].
struct Shared<M> {
    /// Messages waiting for the UI thread to collect.
    inbox: Mutex<VecDeque<M>>,
    /// Whether a wake is already in flight: set by `send`, cleared when the
    /// UI thread collects the inbox. Guards the drain post so a burst of sends
    /// before the next drain posts exactly one wake.
    woken: AtomicBool,
    /// The window whose queue the messages join.
    hwnd: Hwnd,
    /// The window's private drain message.
    drain: u32,
}

impl<M: 'static> Proxy<M> {
    /// Builds a proxy for `core`'s window and hooks the window to collect it.
    pub(crate) fn new(core: &Rc<Core<M>>) -> Proxy<M> {
        let shared = Arc::new(Shared {
            inbox: Mutex::new(VecDeque::new()),
            woken: AtomicBool::new(false),
            hwnd: core.hwnd(),
            drain: sys::message::drain_message(),
        });
        let hooked = {
            let core = Rc::clone(core);
            let shared = Arc::clone(&shared);
            sys::proxy::install(
                shared.hwnd,
                shared.drain,
                Box::new(move || {
                    // Runs on the UI thread, ahead of the window's own drain
                    // handling: move every waiting message into the window's
                    // queue. `enqueue` posts its own drain on the
                    // empty-to-non-empty edge, a harmless backup wake.
                    if shared.woken.swap(false, Ordering::SeqCst) {
                        let waiting: Vec<M> = shared
                            .inbox
                            .lock()
                            .unwrap_or_else(|poison| poison.into_inner())
                            .drain(..)
                            .collect();
                        for msg in waiting {
                            core.enqueue(msg);
                        }
                    }
                }),
            )
        };
        Proxy { shared, hooked }
    }

    /// Sends `msg` to the app for delivery to
    /// [`App::update`](super::App::update), from any thread.
    ///
    /// A burst of sends before the UI thread collects posts a single wake no
    /// matter how many messages it holds. If the window is already gone (or
    /// the hook could not be installed) the message is handed back as
    /// `Err(msg)`; this never panics.
    pub fn send(&self, msg: M) -> Result<(), M> {
        if !self.hooked || !self.shared.hwnd.is_alive() {
            return Err(msg);
        }
        if self.shared.push(msg) {
            let _ = sys::window::post_message(self.shared.hwnd, self.shared.drain, 0, 0);
        }
        Ok(())
    }
}

impl<M> Shared<M> {
    /// Pushes `msg`, reporting whether the caller must post a wake. Only the
    /// first push since the last drain reports `true`, so a burst of pushes
    /// costs a single wake.
    fn push(&self, msg: M) -> bool {
        self.inbox
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .push_back(msg);
        !self.woken.swap(true, Ordering::SeqCst)
    }
}

impl<M> Clone for Proxy<M> {
    fn clone(&self) -> Proxy<M> {
        Proxy {
            shared: Arc::clone(&self.shared),
            hooked: self.hooked,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn shared() -> Shared<String> {
        Shared {
            inbox: Mutex::new(VecDeque::new()),
            woken: AtomicBool::new(false),
            hwnd: Hwnd::NULL,
            drain: 0,
        }
    }

    /// A burst of pushes before the drain posts exactly one wake.
    #[test]
    fn burst_posts_once() {
        let shared = shared();
        let mut posts = 0;
        for i in 0..1000 {
            if shared.push(format!("{i}")) {
                posts += 1;
            }
        }
        assert_eq!(posts, 1, "a burst of 1000 pushes must post one wake");
        assert_eq!(shared.inbox.lock().unwrap().len(), 1000);
    }

    /// After the UI thread collects (clears the flag), the next push posts
    /// again.
    #[test]
    fn posts_again_after_drain() {
        let shared = shared();
        assert!(shared.push("a".to_string()));
        assert!(!shared.push("b".to_string()));
        // What the UI-side hook does on a drain: clear the flag, take all.
        assert!(shared.woken.swap(false, Ordering::SeqCst));
        let waiting: Vec<String> = shared.inbox.lock().unwrap().drain(..).collect();
        assert_eq!(waiting, vec!["a".to_string(), "b".to_string()]);
        assert!(shared.push("c".to_string()));
    }

    /// Concurrent bursts still post exactly one wake and lose nothing.
    #[test]
    fn concurrent_burst_posts_once() {
        let shared = Arc::new(shared());
        let posts = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..8 {
            let shared = Arc::clone(&shared);
            let posts = Arc::clone(&posts);
            handles.push(std::thread::spawn(move || {
                for i in 0..500 {
                    if shared.push(format!("{i}")) {
                        posts.fetch_add(1, Ordering::SeqCst);
                    }
                }
            }));
        }
        for handle in handles {
            handle.join().expect("worker panicked");
        }
        assert_eq!(
            posts.load(Ordering::SeqCst),
            1,
            "8 threads x 500 pushes must post one wake"
        );
        assert_eq!(shared.inbox.lock().unwrap().len(), 4000);
    }

    /// `Proxy` is `Clone`, and `Send + Sync` whenever the message is.
    #[test]
    fn proxy_is_clone_send_sync() {
        fn assert_clone_send_sync<T: Clone + Send + Sync>() {}
        assert_clone_send_sync::<Proxy<String>>();
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Proxy<Vec<u8>>>();
    }
}
