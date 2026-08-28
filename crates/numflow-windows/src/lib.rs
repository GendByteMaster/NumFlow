mod keymap;
mod normalize;

pub use keymap::{KeyState, PhysicalKeyEvent, map_numpad_key};
pub use normalize::{KeyboardEventNormalizer, NormalizedKeyEvent};

#[cfg(windows)]
mod accessibility;
#[cfg(windows)]
mod assistive_technology;
#[cfg(windows)]
mod audio;
#[cfg(windows)]
mod desktop;
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
mod secure_runtime;
#[cfg(windows)]
mod startup;

#[cfg(windows)]
pub use accessibility::client_area_animations_enabled;
#[cfg(windows)]
pub use assistive_technology::{
    AT_KEY_NAME, AssistiveTechnologySession, AssistiveTechnologySessionError, SECURE_AT_KEY_NAME,
    SecureSettings, SecureSettingsError, assistive_technology_registered,
};
#[cfg(windows)]
pub use audio::{AudioCue, AudioFeedbackError, AudioFeedbackService};
#[cfg(windows)]
pub use desktop::{
    DesktopKind, RuntimeKind, current_desktop_kind, current_runtime_kind,
    current_thread_owns_input_desktop,
};
#[cfg(windows)]
pub use diagnostics::{
    ForegroundProcessInfo, current_process_elevated, current_process_integrity,
    current_process_ui_access, foreground_process_info, foreground_process_info_for_window,
};
#[cfg(windows)]
pub use elevation::{ElevationError, relaunch_elevated};
#[cfg(windows)]
pub use hook::{
    HookError, InputResyncReason, InputRuntimeState, KeyboardHook, KeyboardHookDiagnostics,
    KeyboardHookEvent, disable_winit_raw_keyboard_registration,
};
#[cfg(windows)]
pub use hud::{HudPosition, configure_hud_native_window, recommended_hud_position};
#[cfg(windows)]
pub use instance::{SingleInstanceError, SingleInstanceGuard};
#[cfg(windows)]
pub use pointer::{PointerError, WindowsPointer, mouse_hold_active};
#[cfg(windows)]
pub use secure_runtime::{SecureRuntimeError, run_secure_runtime};
#[cfg(windows)]
pub use startup::{StartupError, StartupRegistration};
