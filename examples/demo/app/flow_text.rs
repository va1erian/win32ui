//! The demo's Flow tab: emusic's now-playing line — "Artist · Album (Year) ·
//! Track 3, Disc 1 · Genre" — with the artist and album as separately
//! clickable links, shown once wide and once narrow (where it wraps). The
//! narrow line also carries CJK, an emoji and RTL text, which DirectWrite
//! falls back for.
//!
//! The widget is sized with [`FlowText::preferred_height`], so the height
//! follows the width the layout gives it.

use win32ui::column;
use win32ui::prelude::*;

use super::Msg;

/// The widths the line is shown at, in design units.
const WIDE_WIDTH: f32 = 620.0;
const NARROW_WIDTH: f32 = 240.0;

/// What a click on a link reports.
pub(super) enum FlowMsg {
    /// The artist link was clicked.
    Artist,
    /// The album link was clicked.
    Album,
}

/// The tab's widgets. Holding the two flow lines keeps their windows alive.
pub(super) struct Flow {
    wide: FlowText<Msg>,
    narrow: FlowText<Msg>,
    wide_height: Dip,
    narrow_height: Dip,
    wide_caption: Label,
    narrow_caption: Label,
    hint: Label,
}

/// Builds one now-playing line. `extra` appends a CJK/emoji/RTL tail the narrow
/// copy wraps onto more lines.
fn now_playing(ui: &mut Ui<Msg>, extra: bool) -> FlowText<Msg> {
    let flow = FlowText::new(ui)
        .expect("flow text")
        .run(Run::link("The Midnight Set").on_click(|| Some(Msg::Flow(FlowMsg::Artist))))
        .separator(" · ")
        .run(Run::link("Signal 1").on_click(|| Some(Msg::Flow(FlowMsg::Album))))
        .separator(" · ")
        .run(Run::weak("(2004)").size(12.0))
        .separator(" · ")
        .run(Run::normal("Track 3, Disc 1"))
        .separator(" · ")
        .run(Run::normal("Electronic"));
    if extra {
        flow.separator(" · ")
            .run(Run::weak("日本語のアルバム"))
            .separator(" · ")
            .run(Run::normal("👋"))
            .separator(" · ")
            .run(Run::weak("مرحبا"))
    } else {
        flow
    }
}

impl Flow {
    pub(super) fn build(ui: &mut Ui<Msg>) -> Flow {
        let wide = now_playing(ui, false);
        let wide_height = wide.preferred_height(dip(WIDE_WIDTH));
        let narrow = now_playing(ui, true);
        let narrow_height = narrow.preferred_height(dip(NARROW_WIDTH));
        Flow {
            wide,
            narrow,
            wide_height,
            narrow_height,
            wide_caption: Label::new(ui, Rect::default(), "Wide: the whole line, links hover")
                .expect("caption"),
            narrow_caption: Label::new(
                ui,
                Rect::default(),
                "Narrow: the same runs wrap; CJK, emoji and RTL fall back",
            )
            .expect("caption"),
            hint: Label::new(
                ui,
                Rect::default(),
                "Hover a link for the hand cursor and underline",
            )
            .expect("hint"),
        }
    }

    pub(super) fn page(&self) -> Layout {
        column![
            self.wide_caption.height(dip(20.0)),
            column![self.wide.height(self.wide_height)].width(dip(WIDE_WIDTH)),
            self.narrow_caption.height(dip(20.0)),
            column![self.narrow.height(self.narrow_height)].width(dip(NARROW_WIDTH)),
            self.hint.height(dip(20.0)),
        ]
        .spacing(dip(6.0))
    }

    /// Handles the tab's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg, status: &StatusBar<Msg>) -> bool {
        let Msg::Flow(msg) = msg else {
            return false;
        };
        status.set_text(
            0,
            match msg {
                FlowMsg::Artist => "Flow: go to artist The Midnight Set",
                FlowMsg::Album => "Flow: go to album Signal 1",
            },
        );
        true
    }
}
