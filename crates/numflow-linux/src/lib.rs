#![deny(unsafe_code)]

mod devices;
mod events;
mod permissions;
mod pointer;
mod replay;
mod routing;

pub use devices::{
    DeviceIdentity, KeyboardCandidate, POINTER_DEVICE_NAME, REPLAY_DEVICE_NAME,
    VIRTUAL_DEVICE_PREFIX, discover_keyboards, union_supported_keys,
};
pub use events::{LinuxInputEvent, LinuxKeyCode, LinuxKeyState, map_numpad_key};
pub use permissions::LinuxInputError;
pub use pointer::LinuxPointer;
pub use replay::ReplayKeyboard;
pub use routing::{NumLockRouter, ProcessedBatch, RoutingDecision, process_event_batch};
