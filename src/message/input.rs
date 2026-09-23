#![forbid(unsafe_code)]

//! Input vocabulary shared by the message enum: virtual keys, modifier state,
//! mouse buttons and hit-test results.

use windows::Win32::UI::Input::KeyboardAndMouse as vk;

/// A virtual-key code (`VK_*`), the identifier carried by keyboard messages.
///
/// Named constants cover the common keys and [`Key::from_code`] accepts any raw
/// code. The type is small and `Hash`able so accelerators can use it as a table
/// key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Key(u16);

impl Key {
    /// Wraps a raw virtual-key code.
    pub const fn from_code(code: u16) -> Key {
        Key(code)
    }

    /// The raw virtual-key code.
    pub const fn code(self) -> u16 {
        self.0
    }
}

/// Defines [`Key`]'s named constants from the `windows` crate's `VK_*` values.
macro_rules! keys {
    ($($name:ident => $source:ident),+ $(,)?) => {
        impl Key {
            $(
                #[doc = concat!("The `", stringify!($source), "` key.")]
                pub const $name: Key = Key(vk::$source.0);
            )+
        }
    };
}

keys! {
    BACK => VK_BACK,
    TAB => VK_TAB,
    RETURN => VK_RETURN,
    SHIFT => VK_SHIFT,
    CONTROL => VK_CONTROL,
    MENU => VK_MENU,
    CAPITAL => VK_CAPITAL,
    ESCAPE => VK_ESCAPE,
    SPACE => VK_SPACE,
    PAGE_UP => VK_PRIOR,
    PAGE_DOWN => VK_NEXT,
    END => VK_END,
    HOME => VK_HOME,
    LEFT => VK_LEFT,
    UP => VK_UP,
    RIGHT => VK_RIGHT,
    DOWN => VK_DOWN,
    INSERT => VK_INSERT,
    DELETE => VK_DELETE,
    F1 => VK_F1,
    F2 => VK_F2,
    F3 => VK_F3,
    F4 => VK_F4,
    F5 => VK_F5,
    F6 => VK_F6,
    F7 => VK_F7,
    F8 => VK_F8,
    F9 => VK_F9,
    F10 => VK_F10,
    F11 => VK_F11,
    F12 => VK_F12,
    A => VK_A,
    B => VK_B,
    C => VK_C,
    D => VK_D,
    E => VK_E,
    F => VK_F,
    G => VK_G,
    H => VK_H,
    I => VK_I,
    J => VK_J,
    K => VK_K,
    L => VK_L,
    M => VK_M,
    N => VK_N,
    O => VK_O,
    P => VK_P,
    Q => VK_Q,
    R => VK_R,
    S => VK_S,
    T => VK_T,
    U => VK_U,
    V => VK_V,
    W => VK_W,
    X => VK_X,
    Y => VK_Y,
    Z => VK_Z,
    DIGIT0 => VK_0,
    DIGIT1 => VK_1,
    DIGIT2 => VK_2,
    DIGIT3 => VK_3,
    DIGIT4 => VK_4,
    DIGIT5 => VK_5,
    DIGIT6 => VK_6,
    DIGIT7 => VK_7,
    DIGIT8 => VK_8,
    DIGIT9 => VK_9,
}

/// Which modifier keys were held when an input message was produced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// Ctrl.
    pub ctrl: bool,
    /// Shift.
    pub shift: bool,
    /// Alt (`VK_MENU`).
    pub alt: bool,
    /// The Windows key (left or right).
    pub win: bool,
}

impl Modifiers {
    /// No modifiers held.
    pub const NONE: Modifiers = Modifiers {
        ctrl: false,
        shift: false,
        alt: false,
        win: false,
    };

    /// Whether no modifier is held.
    pub const fn is_empty(self) -> bool {
        !self.ctrl && !self.shift && !self.alt && !self.win
    }
}

/// A mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    /// Left button.
    Left,
    /// Right button.
    Right,
    /// Middle (wheel) button.
    Middle,
    /// First extra (thumb) button.
    X1,
    /// Second extra (thumb) button.
    X2,
}

/// The result of hit-testing the mouse against a window (`WM_SETCURSOR`'s
/// `LOWORD(lparam)`). Codes come from `winuser.h` (`HT*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HitTest {
    /// `HTNOWHERE`: not over any window.
    Nowhere,
    /// `HTCLIENT`: over the client area.
    Client,
    /// `HTCAPTION`: over the title bar.
    Caption,
    /// `HTSYSMENU`: over the system menu.
    SysMenu,
    /// `HTGROWBOX`: over the size box.
    GrowBox,
    /// `HTMENU`: over the menu.
    Menu,
    /// `HTHSCROLL`: over the horizontal scroll bar.
    HScroll,
    /// `HTVSCROLL`: over the vertical scroll bar.
    VScroll,
    /// `HTMINBUTTON`: over the minimise button.
    MinButton,
    /// `HTMAXBUTTON`: over the maximise button.
    MaxButton,
    /// `HTLEFT`: over the left border.
    Left,
    /// `HTRIGHT`: over the right border.
    Right,
    /// `HTTOP`: over the top border.
    Top,
    /// `HTTOPLEFT`: over the top-left corner.
    TopLeft,
    /// `HTTOPRIGHT`: over the top-right corner.
    TopRight,
    /// `HTBOTTOM`: over the bottom border.
    Bottom,
    /// `HTBOTTOMLEFT`: over the bottom-left corner.
    BottomLeft,
    /// `HTBOTTOMRIGHT`: over the bottom-right corner.
    BottomRight,
    /// `HTBORDER`: over an inactive border.
    Border,
    /// `HTOBJECT`: over an object.
    Object,
    /// `HTCLOSE`: over the close button.
    Close,
    /// `HTHELP`: over the help button.
    Help,
    /// Any other code, passed through unchanged (`HTERROR`, `HTTRANSPARENT`…).
    Other(u16),
}

impl HitTest {
    /// Maps a raw hit-test code.
    pub const fn from_code(code: u16) -> HitTest {
        match code {
            0 => HitTest::Nowhere,
            1 => HitTest::Client,
            2 => HitTest::Caption,
            3 => HitTest::SysMenu,
            4 => HitTest::GrowBox,
            5 => HitTest::Menu,
            6 => HitTest::HScroll,
            7 => HitTest::VScroll,
            8 => HitTest::MinButton,
            9 => HitTest::MaxButton,
            10 => HitTest::Left,
            11 => HitTest::Right,
            12 => HitTest::Top,
            13 => HitTest::TopLeft,
            14 => HitTest::TopRight,
            15 => HitTest::Bottom,
            16 => HitTest::BottomLeft,
            17 => HitTest::BottomRight,
            18 => HitTest::Border,
            19 => HitTest::Object,
            20 => HitTest::Close,
            21 => HitTest::Help,
            other => HitTest::Other(other),
        }
    }
}
