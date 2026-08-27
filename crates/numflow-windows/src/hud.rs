use std::{ffi::c_void, mem::size_of};

use windows::Win32::{
    Foundation::{HWND, POINT, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::WindowsAndMessaging::{
        GWL_EXSTYLE, GetCursorPos, GetWindowLongW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOSIZE, SWP_NOZORDER, SetWindowLongW, SetWindowPos, WINDOW_EX_STYLE, WS_EX_APPWINDOW,
        WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    },
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudPosition {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkArea {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl From<RECT> for WorkArea {
    fn from(value: RECT) -> Self {
        Self {
            left: value.left,
            top: value.top,
            right: value.right,
            bottom: value.bottom,
        }
    }
}

#[must_use]
pub fn recommended_hud_position(width: u32, height: u32, margin: u32) -> Option<HudPosition> {
    let width = i32::try_from(width).ok()?;
    let height = i32::try_from(height).ok()?;
    let margin = i32::try_from(margin).ok()?;

    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&raw mut cursor).ok()? };

    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).ok()?,
        ..MONITORINFO::default()
    };

    if !unsafe { GetMonitorInfoW(monitor, &raw mut monitor_info) }.as_bool() {
        return None;
    }

    Some(bottom_right_hud_position(
        WorkArea::from(monitor_info.rcWork),
        width,
        height,
        margin,
    ))
}

/// Apply the native Windows styles expected from a transient HUD/overlay window.
///
/// `WS_EX_TOOLWINDOW` keeps the HUD out of the Alt+Tab switcher, `WS_EX_NOACTIVATE`
/// prevents it from taking keyboard focus when shown, and `WS_EX_APPWINDOW` is removed
/// so the shell does not promote it to a normal application window/taskbar entry.
///
/// # Errors
///
/// Returns the Win32 error from `SetWindowPos` if Windows cannot refresh the window after the
/// extended style is updated.
pub fn configure_hud_native_window(hwnd: isize) -> windows::core::Result<()> {
    let hwnd = HWND(hwnd as *mut c_void);
    let current_style = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) };
    let updated_style = hud_extended_style(current_style);

    unsafe {
        SetWindowLongW(hwnd, GWL_EXSTYLE, updated_style);
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        )?;
    }

    Ok(())
}

fn style_flag(style: WINDOW_EX_STYLE) -> i32 {
    i32::try_from(style.0).expect("NumFlow HUD extended-window style flag must fit in i32")
}

fn hud_extended_style(current_style: i32) -> i32 {
    (current_style | style_flag(WS_EX_TOOLWINDOW) | style_flag(WS_EX_NOACTIVATE))
        & !style_flag(WS_EX_APPWINDOW)
}

fn bottom_right_hud_position(work: WorkArea, width: i32, height: i32, margin: i32) -> HudPosition {
    let left = work.left.saturating_add(margin);
    let top = work.top.saturating_add(margin);

    HudPosition {
        x: work
            .right
            .saturating_sub(margin)
            .saturating_sub(width)
            .max(left),
        y: work
            .bottom
            .saturating_sub(margin)
            .saturating_sub(height)
            .max(top),
    }
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_APPWINDOW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };

    use super::{HudPosition, WorkArea, bottom_right_hud_position, hud_extended_style, style_flag};

    const WORK: WorkArea = WorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    #[test]
    fn hud_is_always_placed_bottom_right() {
        let position = bottom_right_hud_position(WORK, 272, 78, 24);

        assert_eq!(position, HudPosition { x: 1624, y: 938 });
    }

    #[test]
    fn negative_monitor_coordinates_are_supported() {
        let work = WorkArea {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        let position = bottom_right_hud_position(work, 272, 78, 24);

        assert_eq!(position, HudPosition { x: -296, y: 938 });
    }

    #[test]
    fn small_work_areas_clamp_to_the_available_top_left_margin() {
        let work = WorkArea {
            left: 0,
            top: 0,
            right: 240,
            bottom: 100,
        };
        let position = bottom_right_hud_position(work, 272, 78, 24);

        assert_eq!(position, HudPosition { x: 24, y: 24 });
    }

    #[test]
    fn hud_native_style_is_tool_window_and_non_activating() {
        let app_window = style_flag(WS_EX_APPWINDOW);
        let tool_window = style_flag(WS_EX_TOOLWINDOW);
        let no_activate = style_flag(WS_EX_NOACTIVATE);
        let styled = hud_extended_style(app_window);

        assert_eq!(styled & app_window, 0);
        assert_ne!(styled & tool_window, 0);
        assert_ne!(styled & no_activate, 0);
    }
}
