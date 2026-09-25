//! Secondary windows: a non-modal child is destroyed with its owner, a modal
//! disables and re-enables its owner, and both run their own `App` on the
//! shared message loop.

#![cfg(windows)]

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_app_with_watchdog;
use win32ui::column;
use win32ui::gdi::Canvas;
use win32ui::glow;
use win32ui::prelude::*;

/// A child app that records delivery of `Ping` and closes itself.
#[derive(Debug)]
enum ChildMsg {
    Ping,
}

struct ChildApp {
    got: Rc<Cell<bool>>,
}

impl App for ChildApp {
    type Msg = ChildMsg;

    fn update(&mut self, msg: ChildMsg, ui: &mut Ui<ChildMsg>) {
        match msg {
            ChildMsg::Ping => {
                self.got.set(true);
                ui.close();
            }
        }
    }
}

/// A child app that does nothing (used by the owner-destroy test).
struct NoopApp;

impl App for NoopApp {
    type Msg = ();

    fn update(&mut self, _msg: (), _ui: &mut Ui<()>) {}
}

enum ParentMsg {
    Start,
    Send,
    Check,
}

struct ParentApp {
    child: Option<WindowHandle<ChildMsg>>,
    child_alive_after_close: Rc<Cell<bool>>,
    got: Rc<Cell<bool>>,
}

impl App for ParentApp {
    type Msg = ParentMsg;

    fn update(&mut self, msg: ParentMsg, ui: &mut Ui<ParentMsg>) {
        match msg {
            ParentMsg::Start => {
                let got = Rc::clone(&self.got);
                let handle = ui
                    .open_window(
                        WindowSpec::new("Child").size(dip(260.0), dip(120.0)),
                        move |_ui| ChildApp { got },
                    )
                    .expect("child window");
                self.child = Some(handle);
                let timer = ui.set_timer(100).ok();
                if let Some(timer) = timer {
                    ui.on_timer(move |id| (id == timer).then_some(ParentMsg::Check));
                }
                ui.emit(ParentMsg::Send);
            }
            ParentMsg::Send => {
                self.child
                    .as_ref()
                    .expect("child")
                    .send(ChildMsg::Ping)
                    .expect("child alive");
            }
            ParentMsg::Check => {
                self.child_alive_after_close
                    .set(self.child.as_ref().is_some_and(|child| child.is_alive()));
                ui.quit();
            }
        }
    }
}

/// `open_window` runs a child with its own `App`; `send` delivers to it and the
/// child can close itself.
#[test]
fn child_runs_its_own_app_and_receives_sends() {
    let got = Rc::new(Cell::new(false));
    let child_alive_after_close = Rc::new(Cell::new(true));
    let got_for_make = Rc::clone(&got);
    let alive_for_make = Rc::clone(&child_alive_after_close);
    let Some(run) = run_app_with_watchdog("win32ui.app.child", move |ui| {
        ui.emit(ParentMsg::Start);
        ParentApp {
            child: None,
            child_alive_after_close: alive_for_make,
            got: got_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(got.get(), "the child never received the sent message");
    assert!(
        !child_alive_after_close.get(),
        "the child was still alive after closing itself"
    );
}

enum OwnerMsg {
    Start,
}

struct OwnerApp {
    disabled: Rc<Cell<bool>>,
    enabled_after: Rc<Cell<bool>>,
}

impl App for OwnerApp {
    type Msg = OwnerMsg;

    fn update(&mut self, msg: OwnerMsg, ui: &mut Ui<OwnerMsg>) {
        match msg {
            OwnerMsg::Start => {
                let owner = ui.clone();
                let disabled = Rc::clone(&self.disabled);
                let result: Option<bool> = ui.open_modal(
                    WindowSpec::new("Confirm").size(dip(260.0), dip(120.0)),
                    move |ui| {
                        ui.emit(ConfirmMsg::Check);
                        ConfirmApp { owner, disabled }
                    },
                );
                assert_eq!(result, Some(true), "the modal returned its result");
                self.enabled_after.set(ui.is_enabled());
                ui.quit();
            }
        }
    }
}

enum ConfirmMsg {
    Check,
}

struct ConfirmApp {
    owner: Ui<OwnerMsg>,
    disabled: Rc<Cell<bool>>,
}

impl App for ConfirmApp {
    type Msg = ConfirmMsg;

    fn update(&mut self, msg: ConfirmMsg, ui: &mut Ui<ConfirmMsg>) {
        match msg {
            ConfirmMsg::Check => {
                self.disabled.set(!self.owner.is_enabled());
                ui.close_with_result(true);
            }
        }
    }
}

/// `open_modal` disables its owner while the child runs and re-enables it
/// before returning the child's result.
#[test]
fn modal_disables_then_reenables_its_owner() {
    let disabled = Rc::new(Cell::new(false));
    let enabled_after = Rc::new(Cell::new(false));
    let disabled_for_make = Rc::clone(&disabled);
    let enabled_for_make = Rc::clone(&enabled_after);
    let Some(run) = run_app_with_watchdog("win32ui.app.modal", move |ui| {
        ui.emit(OwnerMsg::Start);
        OwnerApp {
            disabled: disabled_for_make,
            enabled_after: enabled_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        disabled.get(),
        "the owner was not disabled while the modal ran"
    );
    assert!(
        enabled_after.get(),
        "the owner was not re-enabled after the modal closed"
    );
}

enum CloseMsg {
    Close,
}

struct ParentCloseApp;

impl App for ParentCloseApp {
    type Msg = CloseMsg;

    fn update(&mut self, msg: CloseMsg, ui: &mut Ui<CloseMsg>) {
        match msg {
            CloseMsg::Close => ui.close(),
        }
    }
}

#[derive(Debug)]
enum ChildLayoutMsg {
    SetText,
}

struct ChildLayoutApp {
    label: Label,
    label_text: Rc<Cell<String>>,
}

impl App for ChildLayoutApp {
    type Msg = ChildLayoutMsg;

    fn update(&mut self, msg: ChildLayoutMsg, _ui: &mut Ui<ChildLayoutMsg>) {
        match msg {
            ChildLayoutMsg::SetText => {
                self.label.set_text("Counter: 1");
                self.label_text.set(self.label.text());
            }
        }
    }
}

enum ParentLayoutMsg {
    Start,
    Check,
}

struct ParentLayoutApp {
    child: Option<WindowHandle<ChildLayoutMsg>>,
    child_client: Rc<Cell<Rect>>,
    label_bounds: Rc<Cell<Rect>>,
    child_alive: Rc<Cell<bool>>,
    label_text: Rc<Cell<String>>,
}

impl App for ParentLayoutApp {
    type Msg = ParentLayoutMsg;

    fn update(&mut self, msg: ParentLayoutMsg, ui: &mut Ui<ParentLayoutMsg>) {
        match msg {
            ParentLayoutMsg::Start => {
                let child_client = Rc::clone(&self.child_client);
                let label_bounds = Rc::clone(&self.label_bounds);
                let label_text = Rc::clone(&self.label_text);
                let handle = ui
                    .open_window(
                        WindowSpec::new("Child layout").size(dip(260.0), dip(120.0)),
                        move |ui| {
                            let label =
                                Label::new(ui, Rect::default(), "Counter: 0").expect("child label");
                            ui.set_layout(column![label.fill(1)]);
                            label_bounds.set(label.bounds());
                            child_client.set(ui.client_rect());
                            ChildLayoutApp { label, label_text }
                        },
                    )
                    .expect("child window");
                self.child = Some(handle.clone());
                handle.send(ChildLayoutMsg::SetText).expect("child alive");
                let timer = ui.set_timer(100).ok();
                if let Some(timer) = timer {
                    ui.on_timer(move |id| (id == timer).then_some(ParentLayoutMsg::Check));
                }
            }
            ParentLayoutMsg::Check => {
                self.child_alive
                    .set(self.child.as_ref().is_some_and(|child| child.is_alive()));
                ui.quit();
            }
        }
    }
}

/// A child lays out its own widgets at creation: a label in a column! layout is
/// sized and placed inside the child's client rect, a `send` delivers to its
/// app and changes the label, and it stays alive for its parent.
#[test]
fn child_lays_out_its_widgets_and_receives_sends() {
    let child_client = Rc::new(Cell::new(Rect::default()));
    let label_bounds = Rc::new(Cell::new(Rect::default()));
    let child_alive = Rc::new(Cell::new(false));
    let label_text = Rc::new(Cell::new(String::new()));

    let child_client_for_make = Rc::clone(&child_client);
    let label_bounds_for_make = Rc::clone(&label_bounds);
    let child_alive_for_make = Rc::clone(&child_alive);
    let label_text_for_make = Rc::clone(&label_text);

    let Some(run) = run_app_with_watchdog("win32ui.app.child.layout", move |ui| {
        ui.emit(ParentLayoutMsg::Start);
        ParentLayoutApp {
            child: None,
            child_client: child_client_for_make,
            label_bounds: label_bounds_for_make,
            child_alive: child_alive_for_make,
            label_text: label_text_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        child_alive.get(),
        "the child window was not alive after one loop turn"
    );
    assert_eq!(
        label_text.take(),
        "Counter: 1",
        "the sent message did not change the label text"
    );

    let client = child_client.get();
    let bounds = label_bounds.get();
    assert!(
        !bounds.is_empty(),
        "the label was not laid out (empty bounds)"
    );
    assert!(
        bounds.left >= client.left
            && bounds.top >= client.top
            && bounds.right <= client.right
            && bounds.bottom <= client.bottom,
        "the label ({bounds:?}) is outside the child client rect ({client:?})"
    );
}

/// Closing the owner destroys an owned child.
#[test]
fn closing_the_parent_destroys_the_child() {
    let child_hwnd = Rc::new(Cell::new(None));
    let child_hwnd_for_make = Rc::clone(&child_hwnd);
    let Some(run) = run_app_with_watchdog("win32ui.app.child.destroy", move |ui| {
        let handle = ui
            .open_window(
                WindowSpec::new("Child").size(dip(260.0), dip(120.0)),
                |_ui| NoopApp,
            )
            .expect("child window");
        child_hwnd_for_make.set(Some(handle.hwnd()));
        ui.emit(CloseMsg::Close);
        ParentCloseApp
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    if let Some(hwnd) = child_hwnd.get() {
        assert!(!hwnd.is_alive(), "the child survived the parent closing");
    }
}

/// A child app that hides itself when its close request is intercepted.
enum HideMsg {
    Hide,
}

struct HideApp;

impl App for HideApp {
    type Msg = HideMsg;

    fn update(&mut self, msg: HideMsg, ui: &mut Ui<HideMsg>) {
        match msg {
            HideMsg::Hide => ui.hide(),
        }
    }
}

enum HideParentMsg {
    Start,
    Closed,
    Shown,
}

struct HideParentApp {
    child: Option<WindowHandle<HideMsg>>,
    alive_after_close: Rc<Cell<bool>>,
    hidden_after_close: Rc<Cell<bool>>,
    visible_after_show: Rc<Cell<bool>>,
}

impl App for HideParentApp {
    type Msg = HideParentMsg;

    fn update(&mut self, msg: HideParentMsg, ui: &mut Ui<HideParentMsg>) {
        match msg {
            HideParentMsg::Start => {
                let handle = ui
                    .open_window(
                        WindowSpec::new("Visualizer").size(dip(260.0), dip(140.0)),
                        |ui| {
                            // Closing the window hides it instead of destroying it.
                            ui.on_close(|| Some(HideMsg::Hide));
                            HideApp
                        },
                    )
                    .expect("child window");
                // `on_close` intercepts `WM_CLOSE`, which is what the title
                // bar's X posts; `WindowHandle::close` would destroy directly.
                // SAFETY: `handle` wraps the live child; posting only enqueues.
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                        Some(windows::Win32::Foundation::HWND(
                            handle.hwnd().raw() as *mut core::ffi::c_void
                        )),
                        WM_CLOSE,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                    );
                }
                self.child = Some(handle);
                let timer = ui.set_timer(150).ok();
                ui.on_timer(move |id| (Some(id) == timer).then_some(HideParentMsg::Closed));
            }
            HideParentMsg::Closed => {
                let child = self.child.as_ref().expect("child");
                self.alive_after_close.set(child.is_alive());
                self.hidden_after_close
                    .set(child.is_alive() && !child.is_visible());
                child.show();
                let timer = ui.set_timer(100).ok();
                ui.on_timer(move |id| (Some(id) == timer).then_some(HideParentMsg::Shown));
            }
            HideParentMsg::Shown => {
                self.visible_after_show
                    .set(self.child.as_ref().is_some_and(|child| child.is_visible()));
                ui.quit();
            }
        }
    }
}

/// A secondary window's close request can be intercepted: the app hides it,
/// keeping its state, and the opener shows it again.
#[test]
fn child_close_is_intercepted_and_hides_instead_of_destroying() {
    let alive_after_close = Rc::new(Cell::new(false));
    let hidden_after_close = Rc::new(Cell::new(false));
    let visible_after_show = Rc::new(Cell::new(false));
    let alive_for_make = Rc::clone(&alive_after_close);
    let hidden_for_make = Rc::clone(&hidden_after_close);
    let visible_for_make = Rc::clone(&visible_after_show);

    let Some(run) = run_app_with_watchdog("win32ui.app.child.hide", move |ui| {
        ui.emit(HideParentMsg::Start);
        HideParentApp {
            child: None,
            alive_after_close: alive_for_make,
            hidden_after_close: hidden_for_make,
            visible_after_show: visible_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        alive_after_close.get(),
        "the intercepted close destroyed the child"
    );
    assert!(
        hidden_after_close.get(),
        "the child was not hidden after the intercepted close"
    );
    assert!(
        visible_after_show.get(),
        "the child did not become visible again after WindowHandle::show"
    );
}

enum PlaceMsg {
    Start,
}

struct PlaceApp {
    rect_after_set: Rc<Cell<Rect>>,
    rect_after_reset: Rc<Cell<Rect>>,
}

impl App for PlaceApp {
    type Msg = PlaceMsg;

    fn update(&mut self, msg: PlaceMsg, ui: &mut Ui<PlaceMsg>) {
        match msg {
            PlaceMsg::Start => {
                let handle = ui
                    .open_window(
                        WindowSpec::new("Child").size(dip(260.0), dip(120.0)),
                        |_ui| NoopApp,
                    )
                    .expect("child window");
                let target = Rect::new(300, 250, 700, 500);
                let mut placement = handle.placement();
                placement.normal = target;
                handle.set_placement(&placement).expect("set placement");
                self.rect_after_set.set(handle.window_rect());

                // Restore the original placement and read it back too.
                let mut moved = handle.placement();
                moved.normal = Rect::new(120, 90, 400, 260);
                handle.set_placement(&moved).expect("move again");
                self.rect_after_reset.set(handle.window_rect());
                ui.quit();
            }
        }
    }
}

/// A `WindowHandle` exposes the child's placement, so the app can save and
/// restore its position and size.
#[test]
fn child_placement_is_readable_and_settable() {
    let rect_after_set = Rc::new(Cell::new(Rect::default()));
    let rect_after_reset = Rc::new(Cell::new(Rect::default()));
    let set_for_make = Rc::clone(&rect_after_set);
    let reset_for_make = Rc::clone(&rect_after_reset);

    let Some(run) = run_app_with_watchdog("win32ui.app.child.placement", move |ui| {
        ui.emit(PlaceMsg::Start);
        PlaceApp {
            rect_after_set: set_for_make,
            rect_after_reset: reset_for_make,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert_eq!(
        rect_after_set.get(),
        Rect::new(300, 250, 700, 500),
        "the child did not move to the requested placement"
    );
    assert_eq!(
        rect_after_reset.get(),
        Rect::new(120, 90, 400, 260),
        "the child's second placement was not applied"
    );
}

/// A `Renderer::Gl` custom widget hosted in a secondary window.
struct GlWidget {
    paints: Rc<Cell<u32>>,
}

impl CustomWidget for GlWidget {
    type Event = ();

    fn paint(&self, _canvas: &Canvas, _bounds: Rect, _theme: &Theme) {}

    fn renderer(&self) -> Renderer {
        Renderer::Gl
    }

    fn paint_gl(&self, _gl: &glow::Context, _bounds: Rect, _theme: &Theme) {
        self.paints.set(self.paints.get() + 1);
    }
}

enum GlChildMsg {
    Done,
}

struct GlChildApp {
    #[allow(dead_code)]
    widget: Custom<GlWidget, GlChildMsg>,
}

impl App for GlChildApp {
    type Msg = GlChildMsg;

    fn update(&mut self, msg: GlChildMsg, ui: &mut Ui<GlChildMsg>) {
        match msg {
            GlChildMsg::Done => ui.close(),
        }
    }
}

enum GlParentMsg {
    Start,
    Check,
}

struct GlParentApp {
    paints: Rc<Cell<u32>>,
    #[allow(dead_code)]
    child: Option<WindowHandle<GlChildMsg>>,
}

impl App for GlParentApp {
    type Msg = GlParentMsg;

    fn update(&mut self, msg: GlParentMsg, ui: &mut Ui<GlParentMsg>) {
        match msg {
            GlParentMsg::Start => {
                let paints = Rc::clone(&self.paints);
                let handle = ui
                    .open_window(
                        WindowSpec::new("GL child").size(dip(300.0), dip(200.0)),
                        move |ui| {
                            let widget = Custom::new(ui, GlWidget { paints }).expect("gl widget");
                            ui.set_layout(column![widget.fill(1)]);
                            let timer = ui.set_timer(250).ok();
                            ui.on_timer(move |id| (Some(id) == timer).then_some(GlChildMsg::Done));
                            GlChildApp { widget }
                        },
                    )
                    .expect("child window");
                self.child = Some(handle);
                let timer = ui.set_timer(600).ok();
                ui.on_timer(move |id| (Some(id) == timer).then_some(GlParentMsg::Check));
            }
            GlParentMsg::Check => ui.quit(),
        }
    }
}

/// A secondary window can host a single `Renderer::Gl` custom widget, which is
/// painted through its own context. Skips when the session has no GL driver.
#[test]
fn child_hosts_a_renderer_gl_custom_widget() {
    if !gl_available() {
        return;
    }
    let paints = Rc::new(Cell::new(0u32));
    let paints_for_make = Rc::clone(&paints);
    let Some(run) = run_app_with_watchdog("win32ui.app.child.gl", move |ui| {
        ui.emit(GlParentMsg::Start);
        GlParentApp {
            paints: paints_for_make,
            child: None,
        }
    }) else {
        return;
    };

    assert!(!run.timed_out, "the watchdog fired before the app quit");
    assert!(
        paints.get() > 0,
        "the secondary window's GL widget never painted"
    );
}

/// Whether this session can create an OpenGL context (a hidden throwaway
/// window, so a GL pixel format is not set on another window's DC).
fn gl_available() -> bool {
    let Ok(class) = WindowClass::register("win32ui.secondary.gl.probe", Theme::light().background)
    else {
        return false;
    };
    let Ok(window) = Window::create(
        class,
        None,
        WindowStyle::overlapped(),
        WindowExStyle::new(),
        Rect::new(0, 0, 16, 16),
        "win32ui.secondary.gl.probe",
        common::NullHandler,
    ) else {
        return false;
    };
    let available = win32ui::gl::GlSurface::new(window.hwnd()).is_ok();
    window.destroy();
    available
}

/// `WM_CLOSE`, mirrored from `Winuser.h`.
const WM_CLOSE: u32 = 0x0010;
