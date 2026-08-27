mod keymap;
mod normalize;

pub use keymap::{KeyState, PhysicalKeyEvent, map_numpad_key};
pub use normalize::{KeyboardEventNormalizer, NormalizedKeyEvent};

#[cfg(windows)]
mod audio;
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
pub use audio::{AudioCue, AudioFeedbackError, AudioFeedbackService};
#[cfg(windows)]
pub use hook::{
    HookError, KeyboardHook, KeyboardHookEvent, remove_raw_keyboard_device_event_registration,
};
#[cfg(windows)]
pub use hud::{HudPosition, configure_hud_native_window, recommended_hud_position};
#[cfg(windows)]
pub use instance::{SingleInstanceError, SingleInstanceGuard};
#[cfg(windows)]
pub use pointer::{PointerError, WindowsPointer};
#[cfg(windows)]
pub use startup::{StartupError, StartupRegistration};
