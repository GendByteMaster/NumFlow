mod keymap;
mod normalize;

pub use keymap::{KeyState, PhysicalKeyEvent, map_numpad_key};
pub use normalize::{KeyboardEventNormalizer, NormalizedKeyEvent};

#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod hud;
#[cfg(windows)]
mod pointer;

#[cfg(windows)]
pub use hook::{HookError, KeyboardHook};
#[cfg(windows)]
pub use hud::{HudPosition, recommended_hud_position};
#[cfg(windows)]
pub use pointer::{PointerError, WindowsPointer};
