#![deny(unsafe_code)]

mod events;
mod routing;

pub use events::{LinuxInputEvent, LinuxKeyCode, LinuxKeyState, map_numpad_key};
pub use routing::{NumLockRouter, RoutingDecision};
