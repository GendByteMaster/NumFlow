mod keymap;
mod normalize;

pub use keymap::{KeyState, PhysicalKeyEvent, map_numpad_key};
pub use normalize::{KeyboardEventNormalizer, NormalizedKeyEvent};

#[cfg(windows)]
mod accessibility;
#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod diagnostics;
#[cfg(windows)]
mod elevation;
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
pub use accessibility::client_area_animations_enabled;
#[cfg(windows)]
pub use audio::{AudioCue, AudioFeedbackError, AudioFeedbackService};
#[cfg(windows)]
pub use diagnostics::{
    ForegroundProcessInfo, current_process_elevated, foreground_process_info,
    foreground_process_info_for_window,
};
#[cfg(windows)]
pub use elevation::{ElevationError, relaunch_elevated};
#[cfg(windows)]
pub use hook::{
    HookError, InputResyncReason, InputRuntimeState, KeyboardHook, KeyboardHookEvent,
    disable_winit_raw_keyboard_registration,
};
#[cfg(windows)]
pub use hud::{HudPosition, configure_hud_native_window, recommended_hud_position};
#[cfg(windows)]
pub use instance::{SingleInstanceError, SingleInstanceGuard};
#[cfg(windows)]
pub use pointer::{PointerError, WindowsPointer};
#[cfg(windows)]
pub use startup::{StartupError, StartupRegistration};
