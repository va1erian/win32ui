#![forbid(unsafe_code)]

//! A minimal colour palette shared by the demo's controls. The real frontend
//! would build this from `emusic-ui`'s theme; here it only needs to prove the
//! plumbing.

use crate::Color;

/// Semantic colours used by the bundled controls.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Window/panel background.
    pub background: Color,
    /// Slightly raised surface (toolbar, headers).
    pub surface: Color,
    /// Primary text.
    pub text: Color,
    /// De-emphasised text.
    pub text_weak: Color,
    /// Selection background.
    pub selection: Color,
    /// Accent used for the playing row and active toolbar buttons.
    pub accent: Color,
    /// Widget border.
    pub border: Color,
}

impl Theme {
    /// The light palette.
    pub const fn light() -> Theme {
        Theme {
            background: Color::hex(0xf6_f6_f6),
            surface: Color::hex(0xea_ea_ea),
            text: Color::hex(0x1a_1a_1a),
            text_weak: Color::hex(0x70_70_70),
            selection: Color::hex(0xcf_e4_f7),
            accent: Color::hex(0x0a_66_c2),
            border: Color::hex(0xcc_cc_cc),
        }
    }

    /// The dark palette.
    pub const fn dark() -> Theme {
        Theme {
            background: Color::hex(0x1e_1e_1e),
            surface: Color::hex(0x2a_2a_2a),
            text: Color::hex(0xf0_f0_f0),
            text_weak: Color::hex(0x9a_9a_9a),
            selection: Color::hex(0x2f_4a_63),
            accent: Color::hex(0x5a_a9_ff),
            border: Color::hex(0x3a_3a_3a),
        }
    }
}
