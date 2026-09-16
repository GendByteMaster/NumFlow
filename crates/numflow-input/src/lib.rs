//! Minimal Windows input helper for NumFlow with UIAccess support.
//!
//! This crate provides the `numflow-input.exe` helper process that runs with
//! `uiAccess=true` to bypass UIPI and inject input into elevated applications.
//! It contains ONLY input/accessibility functions - no shell, exec, or
//! general-purpose privileged APIs.
//!
//! # Architecture
//!
//! NumFlow.exe (medium IL, no UI access) → named pipe → numflow-input.exe (UIAccess)
//! → SendInput → Windows input subsystem
//!
//! The helper:
//! - Is a minimal binary with no UI, tray, audio, shell, or updater
//! - Authenticates the connection via process ID verification + executable pinning
//! - Accepts only whitelisted input commands via a typed protocol
//! - Uses SendInput to inject mouse/keyboard events
//! - Tracks held button state for fail-safe release
//! - Exits cleanly on shutdown message or owner death

pub mod command;
pub mod error;
pub mod input;
pub mod protocol;

pub use command::Command;
pub use error::HelperError;
pub use input::InputBackend;
pub use protocol::{ProtocolError, ProtocolMessage};