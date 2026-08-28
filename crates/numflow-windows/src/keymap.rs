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
