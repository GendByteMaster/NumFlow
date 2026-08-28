use std::{env, iter, path::Path};

use thiserror::Error;
use windows::{
    Win32::{
        Foundation::HWND,
        UI::{
            Shell::ShellExecuteW,
            WindowsAndMessaging::{SHOW_WINDOW_CMD, SW_SHOWNORMAL},
        },
    },
    core::PCWSTR,
};

#[derive(Debug, Error)]
pub enum ElevationError {
    #[error("failed to locate the NumFlow executable: {0}")]
    CurrentExecutable(#[source] std::io::Error),
    #[error("the NumFlow executable path is not valid Unicode")]
    InvalidExecutablePath,
    #[error("Windows rejected the elevated NumFlow launch (ShellExecute code {0})")]
    ShellExecute(isize),
}

/// Starts a new `NumFlow` process through the standard Windows UAC `runas` flow.
///
/// The caller must exit after a successful launch. Elevation is explicit because silently raising
/// a background accessibility utility would be a security and startup-policy violation.
///
/// # Errors
///
/// Returns an error when the executable cannot be resolved or Windows rejects/cancels elevation.
pub fn relaunch_elevated(background: bool) -> Result<(), ElevationError> {
    let executable = env::current_exe().map_err(ElevationError::CurrentExecutable)?;
    relaunch_elevated_executable(&executable, background)
}

fn relaunch_elevated_executable(executable: &Path, background: bool) -> Result<(), ElevationError> {
    let executable = executable
        .to_str()
        .ok_or(ElevationError::InvalidExecutablePath)?;
    let operation = wide("runas");
    let executable = wide(executable);
    let parameters = wide(elevated_parameters(background));

    let result = unsafe {
        ShellExecuteW(
            Some(HWND::default()),
            PCWSTR(operation.as_ptr()),
            PCWSTR(executable.as_ptr()),
            PCWSTR(parameters.as_ptr()),
            PCWSTR::null(),
            SHOW_WINDOW_CMD(SW_SHOWNORMAL.0),
        )
    };
    let code = result.0 as isize;
    if code <= 32 {
        return Err(ElevationError::ShellExecute(code));
    }
    Ok(())
}

fn elevated_parameters(background: bool) -> &'static str {
    if background {
        "--elevated --background"
    } else {
        "--elevated"
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::elevated_parameters;

    #[test]
    fn elevated_profile_preserves_background_mode() {
        assert_eq!(elevated_parameters(false), "--elevated");
        assert_eq!(elevated_parameters(true), "--elevated --background");
    }
}
