//! The demo's Sliders tab: a seek bar with a buffered range and a hover
//! timestamp, a vertical volume slider, a tick-marked tile-size slider, a
//! keyboard-focused slider and a disabled one.

use win32ui::prelude::*;
use win32ui::{column, row};

use super::Msg;

/// The seek bar's length, in seconds (3 minutes 20).
const TRACK_SECONDS: f64 = 200.0;

/// What the sliders tell the app.
pub(super) enum SliderMsg {
    /// The seek bar is being dragged (a live preview).
    Preview(f64),
    /// The seek bar was released or stepped: seek the player here.
    Seek(f64),
    /// The pointer is over the seek bar at this time.
    Hover(f64),
    /// The volume changed.
    Volume(f64),
    /// The tile size changed.
    Tiles(f64),
}

/// The tab's widgets. The sliders paint and take input through their windows;
/// holding them here keeps those windows alive.
pub(super) struct Sliders {
    seek: Slider<Msg>,
    seek_caption: Label,
    seek_readout: Label,
    volume: Slider<Msg>,
    volume_caption: Label,
    tiles: Slider<Msg>,
    tiles_caption: Label,
    focused: Slider<Msg>,
    disabled: Slider<Msg>,
}

/// `seconds` as `m:ss`.
fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0) as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

impl Sliders {
    pub(super) fn build(ui: &mut Ui<Msg>) -> Sliders {
        let label = |ui: &mut Ui<Msg>, text: &str| Label::new(ui, Rect::default(), text);
        let seek = Slider::new(ui, 0.0..=TRACK_SECONDS)
            .expect("seek")
            .value(62.0)
            .on_change(|v| Some(Msg::Slider(SliderMsg::Preview(v))))
            .on_commit(|v| Some(Msg::Slider(SliderMsg::Seek(v))))
            .on_hover(|v| Some(Msg::Slider(SliderMsg::Hover(v))));
        seek.set_buffered(0.0..140.0);
        let volume = Slider::new(ui, 0.0..=1.0)
            .expect("volume")
            .value(0.7)
            .vertical()
            .on_change(|v| Some(Msg::Slider(SliderMsg::Volume(v))));
        let tiles = Slider::new(ui, 1.0..=6.0)
            .expect("tiles")
            .value(3.0)
            .tick_marks(5)
            .key_steps(1.0, 1.0)
            .wheel_step(1.0)
            .on_change(|v| Some(Msg::Slider(SliderMsg::Tiles(v))));
        let focused = Slider::new(ui, 0.0..=1.0).expect("focused").value(0.4);
        let disabled = Slider::new(ui, 0.0..=1.0).expect("disabled").value(0.55);
        disabled.set_enabled(false);
        focused.focus();

        Sliders {
            seek_caption: label(ui, "Seek: click to jump, drag to scrub, hover for the time")
                .expect("caption"),
            seek_readout: label(ui, "Position 1:02 of 3:20").expect("readout"),
            volume_caption: label(ui, "Volume 70%").expect("caption"),
            tiles_caption: label(ui, "Tile size 3 (arrows and wheel step by 1)").expect("caption"),
            seek,
            volume,
            tiles,
            focused,
            disabled,
        }
    }

    pub(super) fn page(&self) -> Layout {
        column![
            self.seek_caption.height(dip(20.0)),
            self.seek.height(dip(28.0)),
            self.seek_readout.height(dip(20.0)),
            row![
                column![
                    self.tiles_caption.height(dip(20.0)),
                    self.tiles.height(dip(40.0)),
                    self.focused.height(dip(28.0)),
                    self.disabled.height(dip(28.0)),
                ]
                .spacing(dip(6.0))
                .fill(1),
                column![self.volume_caption.height(dip(20.0)), self.volume.fill(1)]
                    .width(dip(120.0)),
            ]
            .fill(1),
        ]
        .spacing(dip(6.0))
    }

    /// Handles the tab's messages. Returns whether `msg` was one.
    pub(super) fn update(&mut self, msg: &Msg, status: &StatusBar<Msg>) -> bool {
        let Msg::Slider(msg) = msg else {
            return false;
        };
        match msg {
            SliderMsg::Preview(value) => {
                self.seek_readout
                    .set_text(&format!("Scrubbing: {}", clock(*value)));
            }
            SliderMsg::Seek(value) => {
                self.seek_readout.set_text(&format!(
                    "Position {} of {}",
                    clock(*value),
                    clock(TRACK_SECONDS)
                ));
                status.set_text(0, &format!("Seek to {}", clock(*value)));
            }
            SliderMsg::Hover(value) => status.set_text(0, &format!("Hover {}", clock(*value))),
            SliderMsg::Volume(value) => {
                self.volume_caption
                    .set_text(&format!("Volume {:.0}%", value * 100.0));
            }
            SliderMsg::Tiles(value) => self.tiles_caption.set_text(&format!(
                "Tile size {value:.1} (arrows and wheel step by 1)"
            )),
        }
        true
    }
}
