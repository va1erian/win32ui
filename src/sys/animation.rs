//! The user's "animation effects" preference.

use core::ffi::c_void;

use windows::Win32::UI::WindowsAndMessaging::{
    SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
};
use windows::core::BOOL;

/// Whether client-area animations are enabled (`SPI_GETCLIENTAREAANIMATION`,
/// the "Animation effects" switch in Settings). A failed query counts as on.
pub(crate) fn client_area_animation() -> bool {
    let mut enabled = BOOL(1);
    // SAFETY: `enabled` is a valid `BOOL` out-pointer, which is what
    // `SPI_GETCLIENTAREAANIMATION` writes through `pvParam`.
    let queried = unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(&mut enabled as *mut BOOL as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    queried.is_err() || enabled.as_bool()
}
