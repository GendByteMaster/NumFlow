use std::{io, os::windows::ffi::OsStrExt, path::Path};

use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR},
        System::Registry::{
            HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW,
            RegSetKeyValueW,
        },
    },
    core::w,
};

#[derive(Debug, thiserror::Error)]
pub enum StartupError {
    #[error("failed to resolve the current NumFlow executable: {0}")]
    CurrentExecutable(#[source] io::Error),
    #[error("startup command is too large for the Windows registry")]
    CommandTooLarge,
    #[error("Windows registry operation failed with error code {0:?}")]
    Registry(WIN32_ERROR),
}

pub struct StartupRegistration;

impl StartupRegistration {
    /// Enables or disables launch at Windows sign-in for the current user.
    ///
    /// # Errors
    ///
    /// Returns [`StartupError`] when the executable path cannot be resolved or the per-user Run
    /// registry value cannot be updated.
    pub fn set_enabled(enabled: bool) -> Result<(), StartupError> {
        if enabled {
            let executable = std::env::current_exe().map_err(StartupError::CurrentExecutable)?;
            Self::enable_for_path(&executable)
        } else {
            Self::disable()
        }
    }

    /// Checks whether `NumFlow` currently has a per-user Windows startup registration.
    ///
    /// # Errors
    ///
    /// Returns [`StartupError`] for registry failures other than a missing Run value.
    pub fn is_enabled() -> Result<bool, StartupError> {
        let mut size = 0_u32;
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                w!("NumFlow"),
                RRF_RT_REG_SZ,
                None,
                None,
                Some(&raw mut size),
            )
        };

        if status.is_ok() {
            return Ok(true);
        }
        if is_missing(status) {
            return Ok(false);
        }

        Err(StartupError::Registry(status))
    }

    fn enable_for_path(executable: &Path) -> Result<(), StartupError> {
        let data = startup_command_bytes(executable);
        let data_len = u32::try_from(data.len()).map_err(|_| StartupError::CommandTooLarge)?;
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                w!("NumFlow"),
                REG_SZ.0,
                Some(data.as_ptr().cast()),
                data_len,
            )
        };

        if status.is_ok() {
            Ok(())
        } else {
            Err(StartupError::Registry(status))
        }
    }

    fn disable() -> Result<(), StartupError> {
        let status = unsafe {
            RegDeleteKeyValueW(
                HKEY_CURRENT_USER,
                w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
                w!("NumFlow"),
            )
        };

        if status.is_ok() || is_missing(status) {
            Ok(())
        } else {
            Err(StartupError::Registry(status))
        }
    }
}

fn is_missing(status: WIN32_ERROR) -> bool {
    status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND
}

fn startup_command_bytes(executable: &Path) -> Vec<u8> {
    let mut command = Vec::new();
    command.push(u16::from(b'"'));
    command.extend(executable.as_os_str().encode_wide());
    command.push(u16::from(b'"'));
    command.push(0);

    command.into_iter().flat_map(u16::to_le_bytes).collect()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::startup_command_bytes;

    #[test]
    fn startup_command_quotes_executable_and_is_nul_terminated() {
        let bytes = startup_command_bytes(Path::new(r"C:\Program Files\NumFlow\numflow.exe"));
        let (chunks, remainder) = bytes.as_chunks::<2>();
        assert!(remainder.is_empty());
        let wide = chunks
            .iter()
            .map(|chunk| u16::from_le_bytes(*chunk))
            .collect::<Vec<_>>();
        let decoded = String::from_utf16(&wide[..wide.len() - 1]).expect("valid UTF-16");

        assert_eq!(decoded, r#""C:\Program Files\NumFlow\numflow.exe""#);
        assert_eq!(wide.last(), Some(&0));
    }
}
