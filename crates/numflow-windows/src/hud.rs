use std::mem::size_of;

use windows::Win32::{
    Foundation::{POINT, RECT},
    Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    },
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
    unsafe { GetCursorPos(&mut cursor).ok()? };

    let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
    let mut monitor_info = MONITORINFO {
        cbSize: u32::try_from(size_of::<MONITORINFO>()).ok()?,
        ..MONITORINFO::default()
    };

    if !unsafe { GetMonitorInfoW(monitor, &mut monitor_info) }.as_bool() {
        return None;
    }

    Some(choose_hud_position(
        WorkArea::from(monitor_info.rcWork),
        cursor,
        width,
        height,
        margin,
    ))
}

fn choose_hud_position(
    work: WorkArea,
    cursor: POINT,
    width: i32,
    height: i32,
    margin: i32,
) -> HudPosition {
    let left = work.left.saturating_add(margin);
    let top = work.top.saturating_add(margin);
    let right = work
        .right
        .saturating_sub(margin)
        .saturating_sub(width)
        .max(left);
    let bottom = work
        .bottom
        .saturating_sub(margin)
        .saturating_sub(height)
        .max(top);

    let candidates = [
        HudPosition { x: left, y: top },
        HudPosition { x: right, y: top },
        HudPosition { x: left, y: bottom },
        HudPosition {
            x: right,
            y: bottom,
        },
    ];

    candidates
        .into_iter()
        .max_by_key(|candidate| distance_from_cursor(*candidate, cursor, width, height))
        .expect("HUD always has four placement candidates")
}

fn distance_from_cursor(
    position: HudPosition,
    cursor: POINT,
    width: i32,
    height: i32,
) -> i64 {
    let center_x = i64::from(position.x) + i64::from(width) / 2;
    let center_y = i64::from(position.y) + i64::from(height) / 2;
    let dx = center_x - i64::from(cursor.x);
    let dy = center_y - i64::from(cursor.y);
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

#[cfg(test)]
mod tests {
    use windows::Win32::Foundation::POINT;

    use super::{HudPosition, WorkArea, choose_hud_position};

    const WORK: WorkArea = WorkArea {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1040,
    };

    #[test]
    fn cursor_near_top_left_places_hud_bottom_right() {
        let position = choose_hud_position(WORK, POINT { x: 80, y: 80 }, 272, 78, 24);

        assert_eq!(
            position,
            HudPosition {
                x: 1624,
                y: 938
            }
        );
    }

    #[test]
    fn cursor_near_bottom_right_places_hud_top_left() {
        let position = choose_hud_position(WORK, POINT { x: 1840, y: 980 }, 272, 78, 24);

        assert_eq!(position, HudPosition { x: 24, y: 24 });
    }

    #[test]
    fn negative_monitor_coordinates_are_supported() {
        let work = WorkArea {
            left: -1920,
            top: 0,
            right: 0,
            bottom: 1040,
        };
        let position = choose_hud_position(work, POINT { x: -1800, y: 100 }, 272, 78, 24);

        assert_eq!(position.x, -296);
        assert_eq!(position.y, 938);
    }
}
