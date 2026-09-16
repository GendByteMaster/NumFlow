use numflow_core::NumpadKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxKeyCode {
    NumLock,
    Kp0,
    Kp1,
    Kp2,
    Kp3,
    Kp4,
    Kp5,
    Kp6,
    Kp7,
    Kp8,
    Kp9,
    KpPlus,
    KpDecimal,
    KpDivide,
    KpMultiply,
    KpSubtract,
    Other(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxKeyState {
    Pressed,
    Released,
    Repeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxInputEvent {
    Numpad {
        key: NumpadKey,
        state: LinuxKeyState,
    },
    NumLockChanged {
        num_lock_on: bool,
    },
    InputUnavailable,
}

#[must_use]
pub const fn map_numpad_key(key: LinuxKeyCode) -> Option<NumpadKey> {
    match key {
        LinuxKeyCode::Kp0 => Some(NumpadKey::Num0),
        LinuxKeyCode::Kp1 => Some(NumpadKey::Num1),
        LinuxKeyCode::Kp2 => Some(NumpadKey::Num2),
        LinuxKeyCode::Kp3 => Some(NumpadKey::Num3),
        LinuxKeyCode::Kp4 => Some(NumpadKey::Num4),
        LinuxKeyCode::Kp5 => Some(NumpadKey::Num5),
        LinuxKeyCode::Kp6 => Some(NumpadKey::Num6),
        LinuxKeyCode::Kp7 => Some(NumpadKey::Num7),
        LinuxKeyCode::Kp8 => Some(NumpadKey::Num8),
        LinuxKeyCode::Kp9 => Some(NumpadKey::Num9),
        LinuxKeyCode::KpPlus => Some(NumpadKey::Add),
        LinuxKeyCode::KpDecimal => Some(NumpadKey::Decimal),
        LinuxKeyCode::KpDivide => Some(NumpadKey::Divide),
        LinuxKeyCode::KpMultiply => Some(NumpadKey::Multiply),
        LinuxKeyCode::KpSubtract => Some(NumpadKey::Subtract),
        LinuxKeyCode::NumLock | LinuxKeyCode::Other(_) => None,
    }
}

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
