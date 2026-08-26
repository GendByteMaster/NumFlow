mod keymap;
mod normalize;

pub use keymap::{KeyState, PhysicalKeyEvent, map_numpad_key};
pub use normalize::{KeyboardEventNormalizer, NormalizedKeyEvent};

#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod hud;
#[cfg(windows)]
mod instance;
#[cfg(windows)]
mod pointer;
#[cfg(windows)]
mod startup;
#[cfg(windows)]
mod window;

#[cfg(windows)]
pub use hook::{HookError, KeyboardHook};
#[cfg(windows)]
pub use hud::{HudPosition, recommended_hud_position};
#[cfg(windows)]
pub use instance::{SingleInstanceError, SingleInstanceGuard};
#[cfg(windows)]
pub use pointer::{PointerError, WindowsPointer};
#[cfg(windows)]
pub use startup::{StartupError, StartupRegistration};
#[cfg(windows)]
pub use window::{WindowActivationError, show_numflow_window};
