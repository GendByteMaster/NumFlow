use std::ffi::c_void;

use windows::{
    Win32::UI::WindowsAndMessaging::{
        SPI_GETCLIENTAREAANIMATION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    },
    core::{BOOL, Result},
};

/// Returns whether Windows client-area animations are enabled for the current user.
///
/// # Errors
///
/// Returns the underlying Win32 error if the accessibility preference cannot be queried.
pub fn client_area_animations_enabled() -> Result<bool> {
    let mut enabled = BOOL::from(true);
    // SAFETY: `enabled` is a live BOOL for the duration of the call and the selected SPI action
    // requires `pvParam` to point to a writable BOOL. No pointer escapes this function.
    unsafe {
        SystemParametersInfoW(
            SPI_GETCLIENTAREAANIMATION,
            0,
            Some(std::ptr::addr_of_mut!(enabled).cast::<c_void>()),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )?;
    }
    Ok(enabled.as_bool())
}
