//! Operating-system boundary for global input capture setup.
//!
//! Shared bindings, motion, and pointer state stay in `numflow-core`. Each desktop platform owns
//! its global listener, permission model, device lifecycle, and injection implementation here or in
//! a dedicated platform crate.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
pub(crate) use linux::prepare_after_ui;
#[cfg(target_os = "macos")]
pub(crate) use macos::prepare_after_ui;
#[cfg(windows)]
pub(crate) use windows::prepare_after_ui;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(crate) fn prepare_after_ui() -> Result<(), String> {
    Err("global input capture is not implemented for this operating system".to_owned())
}
