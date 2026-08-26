use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
        System::Threading::CreateMutexW,
    },
    core::{Error as WindowsError, PCWSTR},
};

const INSTANCE_NAME: &str = "Local\\NumFlow.SingleInstance.v1";

#[derive(Debug, thiserror::Error)]
pub enum SingleInstanceError {
    #[error("another NumFlow instance is already running")]
    AlreadyRunning,
    #[error("failed to create the NumFlow single-instance mutex: {0}")]
    Create(#[source] WindowsError),
}

#[derive(Debug)]
pub struct SingleInstanceGuard {
    handle: HANDLE,
}

impl SingleInstanceGuard {
    /// Acquires the per-session NumFlow single-instance mutex.
    ///
    /// # Errors
    ///
    /// Returns [`SingleInstanceError::AlreadyRunning`] when another NumFlow process in the same
    /// Windows session already owns a handle to the mutex. Other Win32 failures are returned as
    /// [`SingleInstanceError::Create`].
    pub fn acquire() -> Result<Self, SingleInstanceError> {
        Self::acquire_named(INSTANCE_NAME)
    }

    fn acquire_named(name: &str) -> Result<Self, SingleInstanceError> {
        let wide_name = wide_null(name);
        let handle = unsafe { CreateMutexW(None, false, PCWSTR(wide_name.as_ptr())) }
            .map_err(SingleInstanceError::Create)?;
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        if already_exists {
            let _ = unsafe { CloseHandle(handle) };
            return Err(SingleInstanceError::AlreadyRunning);
        }

        Ok(Self { handle })
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseHandle(self.handle) };
    }
}

fn wide_null(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::{SingleInstanceError, SingleInstanceGuard};

    #[test]
    fn second_guard_with_same_name_is_rejected_until_first_is_dropped() {
        let name = format!("Local\\NumFlow.Test.{}.{}", std::process::id(), line!());
        let first = SingleInstanceGuard::acquire_named(&name).expect("first guard should acquire");

        assert!(matches!(
            SingleInstanceGuard::acquire_named(&name),
            Err(SingleInstanceError::AlreadyRunning)
        ));

        drop(first);
        let _replacement =
            SingleInstanceGuard::acquire_named(&name).expect("mutex should release with guard");
    }
}
