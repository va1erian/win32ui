//! Interactive check for the material top bar: it reserves its band, exposes a
//! native slot, and the top-level surface paints the band instead of leaving it
//! black (which is what an uncomposited extended-frame band looks like).
//!
//! Opt-in (`#[ignore]`) because it shows a real window and runs the message
//! loop; run with `cargo test --test top_bar -- --ignored --nocapture`.

#![cfg(windows)]

mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use win32ui::prelude::*;

enum Msg {
    Capture,
    Quit,
}

struct App {
    shots: Rc<RefCell<Vec<RgbaImage>>>,
}

impl win32ui::App for App {
    type Msg = Msg;

    fn update(&mut self, msg: Msg, ui: &mut Ui<Msg>) {
        match msg {
            Msg::Capture => {
                if let Ok(image) = ui.capture() {
                    self.shots.borrow_mut().push(image);
                }
            }
            Msg::Quit => ui.quit(),
        }
    }
}

fn spec() -> WindowSpec {
    WindowSpec::new("top-bar.material")
        .theme(Theme::dark())
        .backdrop(Backdrop::Acrylic)
        .title_bar(TitleBar::Extended)
}

/// The mean brightness of the window centre (a plausible client pixel).
fn centre(image: &RgbaImage) -> u32 {
    let [r, g, b, _] = image
        .pixel(image.width / 2, image.height / 2)
        .unwrap_or([0; 4]);
    u32::from(r) + u32::from(g) + u32::from(b)
}

#[test]
#[ignore = "shows a real window and runs the message loop"]
fn the_top_bar_reserves_its_band_and_paints() {
    let shots = Rc::new(RefCell::new(Vec::new()));
    let shots_for_app = Rc::clone(&shots);
    let Some(run) = common::run_app_spec_with_watchdog(spec(), move |ui| {
        let bar = MaterialTopBar::new(ui).expect("material top bar");
        bar.set_items(vec![
            TopBarItem::icon_button(1u32, Fluent::PLAY).tooltip("Play"),
            TopBarItem::flexible_spacer(),
            TopBarItem::slider(2u32, 0.5, 0.0..=1.0),
            TopBarItem::label(3u32, "0:42"),
            TopBarItem::native(4u32, dip(100.0)),
        ]);
        // The band is reserved below the strip, so `title_bar_height` grew.
        assert!(
            ui.title_bar_height().value() > ui.strip_height().value() as f32,
            "title_bar_height must include the top bar band"
        );
        assert!(
            ui.material_top_bar_height().value() >= 1.0,
            "the band has height"
        );
        assert!(
            ui.material_top_bar_slot(4u32).is_some(),
            "the native slot is reported"
        );

        let proxy = ui.proxy();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(700));
            let _ = proxy.send(Msg::Capture);
            std::thread::sleep(Duration::from_millis(200));
            let _ = proxy.send(Msg::Quit);
        });
        App {
            shots: shots_for_app,
        }
    }) else {
        eprintln!("skipping: this session cannot create windows");
        return;
    };
    assert!(!run.timed_out, "the app hung");

    let shots = shots.borrow();
    assert_eq!(shots.len(), 1, "expected one capture");
    let brightness = centre(&shots[0]);
    assert!(
        brightness > 30,
        "the client is near-black ({brightness}) — the material surface did not paint"
    );
}
