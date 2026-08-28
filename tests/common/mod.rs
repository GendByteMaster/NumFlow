use numflow_windows::{KeyState, PhysicalKeyEvent};

pub fn key_event(scan_code: u32, extended: bool, state: KeyState) -> PhysicalKeyEvent {
    PhysicalKeyEvent::new(0, scan_code, extended, state)
}

pub fn numpad_event(scan_code: u32, state: KeyState) -> PhysicalKeyEvent {
    key_event(scan_code, false, state)
}
