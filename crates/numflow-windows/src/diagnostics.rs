use std::mem::size_of;

use windows::{
    Win32::{
        Foundation::{CloseHandle, HANDLE, HWND},
        Security::{
            GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TOKEN_ELEVATION,
            TOKEN_MANDATORY_LABEL, TOKEN_QUERY, TokenElevation, TokenIntegrityLevel, TokenUIAccess,
        },
        System::Threading::{
            GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_NAME_WIN32,
            PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
    core::PWSTR,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundProcessInfo {
    pub window: HWND,
    pub process_id: u32,
    pub process_name: String,
    pub elevated: Option<bool>,
    pub integrity: Option<&'static str>,
}

#[repr(C)]
struct TokenLabelBuffer {
    label: TOKEN_MANDATORY_LABEL,
    _sid_storage: [u8; 128],
}

#[must_use]
pub fn foreground_process_info() -> Option<ForegroundProcessInfo> {
    let window = unsafe { GetForegroundWindow() };
    foreground_process_info_for_window(window)
}

#[must_use]
pub fn foreground_process_info_for_window(window: HWND) -> Option<ForegroundProcessInfo> {
    if window.0.is_null() {
        return None;
    }

    let mut process_id = 0;
    let thread_id = unsafe { GetWindowThreadProcessId(window, Some(&raw mut process_id)) };
    if thread_id == 0 || process_id == 0 {
        return None;
    }

    let process =
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, process_id) }.ok()?;
    let process_name = process_name(process).unwrap_or_else(|| "<unknown>".to_owned());
    let (elevated, integrity) = process_security(process);
    let _ = unsafe { CloseHandle(process) };

    Some(ForegroundProcessInfo {
        window,
        process_id,
        process_name,
        elevated,
        integrity,
    })
}

#[must_use]
pub fn current_process_elevated() -> Option<bool> {
    process_security(unsafe { GetCurrentProcess() }).0
}

#[must_use]
pub fn current_process_integrity() -> Option<&'static str> {
    process_security(unsafe { GetCurrentProcess() }).1
}

#[must_use]
pub fn current_process_ui_access() -> Option<bool> {
    let mut token = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) }.ok()?;

    let mut ui_access = 0_u32;
    let mut return_length = 0;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenUIAccess,
            Some((&raw mut ui_access).cast()),
            u32::try_from(size_of::<u32>()).ok()?,
            &raw mut return_length,
        )
    }
    .ok()
    .map(|()| ui_access != 0);
    let _ = unsafe { CloseHandle(token) };
    result
}

fn process_name(process: HANDLE) -> Option<String> {
    let mut buffer = [0u16; 512];
    let mut length = u32::try_from(buffer.len()).ok()?;
    unsafe {
        QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(buffer.as_mut_ptr()),
            &raw mut length,
        )
    }
    .ok()?;

    let path = String::from_utf16_lossy(&buffer[..usize::try_from(length).ok()?]);
    std::path::Path::new(&path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
}

fn process_security(process: HANDLE) -> (Option<bool>, Option<&'static str>) {
    let mut token = HANDLE::default();
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &raw mut token) }.is_err() {
        return (None, None);
    }

    let mut elevation = TOKEN_ELEVATION::default();
    let mut return_length = 0;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            Some((&raw mut elevation).cast()),
            u32::try_from(size_of::<TOKEN_ELEVATION>()).unwrap_or(0),
            &raw mut return_length,
        )
    };
    let elevated = result.ok().map(|()| elevation.TokenIsElevated != 0);

    let mut label_buffer = TokenLabelBuffer {
        label: TOKEN_MANDATORY_LABEL::default(),
        _sid_storage: [0; 128],
    };
    let mut label_return_length = 0;
    let integrity = unsafe {
        GetTokenInformation(
            token,
            TokenIntegrityLevel,
            Some((&raw mut label_buffer.label).cast()),
            u32::try_from(size_of::<TokenLabelBuffer>()).unwrap_or(0),
            &raw mut label_return_length,
        )
    }
    .ok()
    .and_then(|()| {
        let label = &label_buffer.label;
        if label.Label.Sid.0.is_null() {
            return None;
        }
        let count = unsafe { *GetSidSubAuthorityCount(label.Label.Sid) };
        if count == 0 {
            return None;
        }
        let rid = unsafe { *GetSidSubAuthority(label.Label.Sid, u32::from(count - 1)) };
        Some(integrity_label(rid))
    });
    let _ = unsafe { CloseHandle(token) };

    (elevated, integrity)
}

const fn integrity_label(rid: u32) -> &'static str {
    match rid {
        0x1000 => "low",
        0x2000 => "medium",
        0x2100 => "medium-plus",
        0x3000 => "high",
        0x4000 => "system",
        0x5000 => "protected",
        _ => "unknown",
    }
}
