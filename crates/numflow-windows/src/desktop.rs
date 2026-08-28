use std::mem::{MaybeUninit, size_of};

use windows::{
    Win32::{
        Foundation::HANDLE,
        System::{
            RemoteDesktop::{
                ProcessIdToSessionId, WTS_SESSIONSTATE_LOCK, WTSFreeMemory, WTSINFOEX_LEVEL1_W,
                WTSINFOEXW, WTSQuerySessionInformationW, WTSSessionInfoEx,
            },
            StationsAndDesktops::{
                CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, GetThreadDesktop,
                GetUserObjectInformationW, OpenInputDesktop, UOI_NAME,
            },
            Threading::{GetCurrentProcessId, GetCurrentThreadId},
        },
    },
    core::PWSTR,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DesktopKind {
    Default,
    Secure,
    Locked,
    Logon,
    Unknown,
}

impl DesktopKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Secure => "secure",
            Self::Locked => "locked",
            Self::Logon => "logon",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Normal,
    Elevated,
    Secure,
}

impl RuntimeKind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Elevated => "elevated",
            Self::Secure => "secure",
        }
    }
}

#[must_use]
pub fn current_runtime_kind() -> RuntimeKind {
    let mut secure = false;
    let mut elevated = false;
    for argument in std::env::args_os().skip(1) {
        secure |= argument == "--secure-runtime";
        elevated |= argument == "--elevated";
    }

    if secure {
        RuntimeKind::Secure
    } else if elevated {
        RuntimeKind::Elevated
    } else {
        RuntimeKind::Normal
    }
}

#[must_use]
pub fn current_desktop_kind() -> DesktopKind {
    let Some(name) = current_thread_desktop_name() else {
        return DesktopKind::Unknown;
    };
    classify_desktop(&name, current_session_surface())
}

/// Returns whether the calling thread is attached to the desktop currently receiving input.
///
/// Failure to open or identify the input desktop is treated as inactive. This fail-closed behavior
/// is important while the secure desktop denies access to the normal medium-integrity process.
#[must_use]
pub fn current_thread_owns_input_desktop() -> bool {
    let Some(thread_name) = current_thread_desktop_name() else {
        return false;
    };
    let input = unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) };
    let Ok(input) = input else {
        return false;
    };
    let input_name = desktop_name(HANDLE(input.0));
    let _ = unsafe { CloseDesktop(input) };

    input_name.is_some_and(|name| name.eq_ignore_ascii_case(&thread_name))
}

fn current_thread_desktop_name() -> Option<String> {
    let desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) }.ok()?;
    desktop_name(HANDLE(desktop.0))
}

fn desktop_name(handle: HANDLE) -> Option<String> {
    let mut buffer = [0_u16; 256];
    let mut needed = 0;
    unsafe {
        GetUserObjectInformationW(
            handle,
            UOI_NAME,
            Some(buffer.as_mut_ptr().cast()),
            u32::try_from(size_of::<[u16; 256]>()).ok()?,
            Some(&raw mut needed),
        )
    }
    .ok()?;

    let length = buffer.iter().position(|unit| *unit == 0)?;
    String::from_utf16(&buffer[..length]).ok()
}

#[derive(Debug, Clone, Copy)]
struct SessionSurface {
    locked: bool,
    has_user: bool,
}

fn current_session_surface() -> Option<SessionSurface> {
    let mut session_id = 0;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &raw mut session_id) }.ok()?;

    let mut buffer = PWSTR::null();
    let mut byte_count = 0;
    unsafe {
        WTSQuerySessionInformationW(
            None,
            session_id,
            WTSSessionInfoEx,
            &raw mut buffer,
            &raw mut byte_count,
        )
    }
    .ok()?;

    let result = if usize::try_from(byte_count).ok()? >= size_of::<WTSINFOEXW>() {
        let mut aligned = MaybeUninit::<WTSINFOEXW>::uninit();
        unsafe {
            std::ptr::copy_nonoverlapping(
                buffer.0.cast::<u8>(),
                aligned.as_mut_ptr().cast::<u8>(),
                size_of::<WTSINFOEXW>(),
            );
        }
        let info = unsafe { aligned.assume_init() };
        if info.Level == 1 {
            let level: WTSINFOEX_LEVEL1_W = unsafe { info.Data.WTSInfoExLevel1 };
            Some(SessionSurface {
                locked: level.SessionFlags == WTS_SESSIONSTATE_LOCK.cast_signed(),
                has_user: level.UserName.first().copied().unwrap_or_default() != 0,
            })
        } else {
            None
        }
    } else {
        None
    };
    unsafe { WTSFreeMemory(buffer.0.cast()) };
    result
}

fn classify_desktop(name: &str, session: Option<SessionSurface>) -> DesktopKind {
    if name.eq_ignore_ascii_case("default") {
        return DesktopKind::Default;
    }
    if !name.eq_ignore_ascii_case("winlogon") {
        return DesktopKind::Unknown;
    }

    match session {
        Some(SessionSurface {
            has_user: false, ..
        }) => DesktopKind::Logon,
        Some(SessionSurface { locked: true, .. }) => DesktopKind::Locked,
        Some(_) | None => DesktopKind::Secure,
    }
}

#[cfg(test)]
mod tests {
    use super::{DesktopKind, SessionSurface, classify_desktop};

    #[test]
    fn desktop_names_and_session_state_are_classified_without_window_data() {
        assert_eq!(classify_desktop("Default", None), DesktopKind::Default);
        assert_eq!(classify_desktop("ConsentUx", None), DesktopKind::Unknown);
        assert_eq!(
            classify_desktop(
                "Winlogon",
                Some(SessionSurface {
                    locked: false,
                    has_user: false,
                })
            ),
            DesktopKind::Logon
        );
        assert_eq!(
            classify_desktop(
                "Winlogon",
                Some(SessionSurface {
                    locked: true,
                    has_user: true,
                })
            ),
            DesktopKind::Locked
        );
        assert_eq!(
            classify_desktop(
                "Winlogon",
                Some(SessionSurface {
                    locked: false,
                    has_user: true,
                })
            ),
            DesktopKind::Secure
        );
    }
}
