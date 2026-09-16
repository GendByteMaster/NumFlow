#[cfg(test)]
mod tests {
    use numflow_core::NumpadKey;

    use super::{LinuxKeyCode, map_numpad_key};

    #[test]
    fn maps_linux_keypad_keys_to_shared_numpad_keys() {
        assert_eq!(map_numpad_key(LinuxKeyCode::Kp8), Some(NumpadKey::Num8));
        assert_eq!(map_numpad_key(LinuxKeyCode::KpPlus), Some(NumpadKey::Add));
        assert_eq!(
            map_numpad_key(LinuxKeyCode::KpDecimal),
            Some(NumpadKey::Decimal)
        );
        assert_eq!(map_numpad_key(LinuxKeyCode::Other(30)), None);
    }
}
