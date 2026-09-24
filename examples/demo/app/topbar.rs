//! The demo's material top bar: a transport row on the backdrop band, with
//! icon buttons, a repeat toggle, a seek slider, elapsed/volume labels, a
//! volume slider and a native `Edit` search slot.
//!
//! Enabled with `WIN32UI_DEMO_TOP_BAR=1`; only takes effect with
//! `WIN32UI_DEMO_TITLEBAR=extended` (and shows the material with
//! `WIN32UI_DEMO_BACKDROP=acrylic`). Every event maps to
//! [`Msg::TopBar`](super::Msg::TopBar), and a tick advances the seek slider to
//! exercise the cheap `set_value`/`set_text` updates.

use win32ui::prelude::*;

use super::Msg;

/// The seek slider, elapsed label, volume slider and native search slot.
const SEEK: TopBarId = TopBarId::new(5);
const ELAPSED: TopBarId = TopBarId::new(6);
const SEARCH: TopBarId = TopBarId::new(8);

/// The demo transport bar and its app-owned search box.
pub(super) struct TopBar {
    bar: MaterialTopBar<Msg>,
    /// Kept alive here: the bar positions it but the app owns it.
    _search: Edit<Msg>,
    repeat: bool,
    elapsed: f64,
    duration: f64,
}

/// Builds the bar, or `None` when the window is not extended (or DirectWrite is
/// unavailable), so the demo can skip it.
pub(super) fn build(ui: &mut Ui<Msg>) -> Option<TopBar> {
    let bar = match MaterialTopBar::new(ui) {
        Ok(bar) => bar,
        Err(error) => {
            eprintln!("demo: top bar unavailable ({error}); needs TitleBar::Extended");
            return None;
        }
    };
    let duration = 215.0;
    let search = Edit::single_line(ui)
        .ok()?
        .cue("Search")
        .on_change(|text| Some(Msg::Search(text.to_owned())));
    bar.set_items(vec![
        TopBarItem::icon_button(1u32, Fluent::PREVIOUS).tooltip("Previous"),
        TopBarItem::icon_button(2u32, Fluent::PLAY).tooltip("Play"),
        TopBarItem::icon_button(3u32, Fluent::STOP).tooltip("Stop"),
        TopBarItem::toggle(4u32, Fluent::REPEAT).tooltip("Repeat"),
        // The seek bar expands to take the space between the transport buttons
        // and the right-hand controls (emusic's top bar layout).
        TopBarItem::slider(SEEK, 42.0, 0.0..=duration).expand(true),
        TopBarItem::label(ELAPSED, "0:42"),
        TopBarItem::slider(7u32, 0.7, 0.0..=1.0).width(dip(90.0)),
        // The bar keeps the edit in its slot across resizes, at the edit's
        // natural single-line height.
        TopBarItem::native(SEARCH, dip(200.0))
            .height(dip(20.0))
            .child(&search)
            .tooltip("Search"),
    ]);

    let bar = bar.on_event(|event| Some(Msg::TopBar(event)));
    Some(TopBar {
        bar,
        _search: search,
        repeat: false,
        elapsed: 42.0,
        duration,
    })
}

impl TopBar {
    /// Called on a timer: advances the seek slider through the cheap
    /// per-frame updates.
    pub(super) fn tick(&self) {
        let elapsed = (self.elapsed + 0.5).min(self.duration);
        self.bar.set_value(SEEK, elapsed);
        self.bar.set_text(ELAPSED, &format_clock(elapsed));
    }

    /// Applies one top bar event, returning a status line for the demo.
    pub(super) fn event(&mut self, event: TopBarEvent) -> String {
        match event {
            TopBarEvent::Click(id) => format!("Top bar clicked {}", id.value()),
            TopBarEvent::Toggle { checked, .. } => {
                self.repeat = checked;
                format!("Repeat {}", if checked { "on" } else { "off" })
            }
            TopBarEvent::SliderChange { value, .. } => {
                format!("Seek preview {:.0}s", value)
            }
            TopBarEvent::SliderCommit { value, .. } => {
                self.elapsed = value;
                format!("Seek {:.0}s", value)
            }
        }
    }
}

/// Formats seconds as `m:ss`.
fn format_clock(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}
