use numflow_core::NumpadKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalKeyEvent {
    pub vk_code: u32,
    pub scan_code: u32,
    pub extended: bool,
    pub state: KeyState,
}

impl PhysicalKeyEvent {
    #[must_use]
    pub const fn new(vk_code: u32, scan_code: u32, extended: bool, state: KeyState) -> Self {
        Self {
            vk_code,
            scan_code,
            extended,
            state,
        }
    }
}

#[must_use]
pub const fn map_numpad_key(event: PhysicalKeyEvent) -> Option<NumpadKey> {
    match (event.scan_code, event.extended) {
        (0x52, false) => Some(NumpadKey::Num0),
        (0x4F, false) => Some(NumpadKey::Num1),
        (0x50, false) => Some(NumpadKey::Num2),
        (0x51, false) => Some(NumpadKey::Num3),
        (0x4B, false) => Some(NumpadKey::Num4),
        (0x4C, false) => Some(NumpadKey::Num5),
        (0x4D, false) => Some(NumpadKey::Num6),
        (0x47, false) => Some(NumpadKey::Num7),
        (0x48, false) => Some(NumpadKey::Num8),
        (0x49, false) => Some(NumpadKey::Num9),
        (0x4E, false) => Some(NumpadKey::Add),
        (0x53, false) => Some(NumpadKey::Decimal),
        (0x35, true) => Some(NumpadKey::Divide),
        (0x37, false) => Some(NumpadKey::Multiply),
        (0x4A, false) => Some(NumpadKey::Subtract),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{KeyState, PhysicalKeyEvent, map_numpad_key};
    use numflow_core::NumpadKey;

    fn down(scan_code: u32, extended: bool) -> PhysicalKeyEvent {
        PhysicalKeyEvent::new(0, scan_code, extended, KeyState::Pressed)
    }

    #[test]
    fn maps_all_numflow_numpad_keys_by_scan_code() {
        let expected = [
            (0x52, false, NumpadKey::Num0),
            (0x4F, false, NumpadKey::Num1),
            (0x50, false, NumpadKey::Num2),
            (0x51, false, NumpadKey::Num3),
            (0x4B, false, NumpadKey::Num4),
            (0x4C, false, NumpadKey::Num5),
            (0x4D, false, NumpadKey::Num6),
            (0x47, false, NumpadKey::Num7),
            (0x48, false, NumpadKey::Num8),
            (0x49, false, NumpadKey::Num9),
            (0x4E, false, NumpadKey::Add),
            (0x53, false, NumpadKey::Decimal),
            (0x35, true, NumpadKey::Divide),
            (0x37, false, NumpadKey::Multiply),
            (0x4A, false, NumpadKey::Subtract),
        ];

        for (scan_code, extended, key) in expected {
            assert_eq!(map_numpad_key(down(scan_code, extended)), Some(key));
        }
    }

    #[test]
    fn extended_navigation_cluster_is_not_treated_as_numpad() {
        for scan_code in [0x47, 0x48, 0x49, 0x4B, 0x4D, 0x4F, 0x50, 0x51, 0x52, 0x53] {
            assert_eq!(map_numpad_key(down(scan_code, true)), None);
        }
    }

    #[test]
    fn regular_slash_is_not_treated_as_numpad_divide() {
        assert_eq!(map_numpad_key(down(0x35, false)), None);
        assert_eq!(map_numpad_key(down(0x35, true)), Some(NumpadKey::Divide));
    }

    #[test]
    fn mapping_is_independent_from_virtual_key_code() {
        let numpad_2_with_numlock = PhysicalKeyEvent::new(0x62, 0x50, false, KeyState::Pressed);
        let numpad_2_without_numlock = PhysicalKeyEvent::new(0x28, 0x50, false, KeyState::Pressed);

        assert_eq!(map_numpad_key(numpad_2_with_numlock), Some(NumpadKey::Num2));
        assert_eq!(
            map_numpad_key(numpad_2_without_numlock),
            Some(NumpadKey::Num2)
        );
    }
}
