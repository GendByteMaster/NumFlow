use std::mem::size_of;

use windows::Win32::{
    Foundation::{POINT, RECT},
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::WindowsAndMessaging::GetCursorPos,
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
    use super::{HudPosition, WorkArea, bottom_right_hud_position};

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
}
