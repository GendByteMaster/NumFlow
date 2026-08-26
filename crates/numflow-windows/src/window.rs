use windows::{
    Win32::UI::WindowsAndMessaging::{FindWindowW, SW_RESTORE, SetForegroundWindow, ShowWindow},
    core::{Error as WindowsError, w},
};

#[derive(Debug, thiserror::Error)]
pub enum WindowActivationError {
    #[error("failed to find the NumFlow settings window: {0}")]
    Find(#[source] WindowsError),
}

/// Shows and activates the existing `NumFlow` settings window.
///
/// # Errors
///
/// Returns [`WindowActivationError`] when the settings window cannot be found.
pub fn show_numflow_window() -> Result<(), WindowActivationError> {
    let window =
        unsafe { FindWindowW(None, w!("NumFlow")) }.map_err(WindowActivationError::Find)?;

    let _ = unsafe { ShowWindow(window, SW_RESTORE) };
    let _ = unsafe { SetForegroundWindow(window) };

    Ok(())
}
