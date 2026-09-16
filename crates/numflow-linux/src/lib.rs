#![deny(unsafe_code)]

mod devices;
mod events;
mod pointer;
mod replay;
mod routing;

pub use devices::{
    DeviceIdentity, KeyboardCandidate, POINTER_DEVICE_NAME, REPLAY_DEVICE_NAME,
    VIRTUAL_DEVICE_PREFIX, discover_keyboards, union_supported_keys,
};
pub use events::{LinuxInputEvent, LinuxKeyCode, LinuxKeyState, map_numpad_key};
pub use routing::{NumLockRouter, RoutingDecision};
