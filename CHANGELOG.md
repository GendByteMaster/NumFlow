# Changelog

All notable user-facing changes to NumFlow are tracked here.

## [Unreleased]

### Added

- Compact Apple-inspired Slint settings interface with material/glass styling and accessible motion.
- Separate editable Bindings panel instead of a permanent NumPad visualization in the main window.
- HUD material styling and semantic UI SFX feedback.
- Persistent 0–100% interface-sound volume control.
- `--background` launch mode for silent startup into the system tray/runtime.
- WiX Toolset 4 Windows x64 MSI packaging.
- Portable Windows x64 ZIP packaging and SHA-256 release checksums.
- GitHub Actions Windows distribution pipeline for release pull requests and version tags.
- Event-driven Windows suspend/resume recovery documentation covering hook re-arming, Raw Input reconciliation, Num Lock resynchronization, diagnostics, and manual lifecycle validation.

### Changed

- `Start with Windows` now registers NumFlow with the explicit `--background` launch mode.
- Windows release packaging uses the embedded NumFlow executable icon generated from `assets/numflow-icon.svg`.
- Input lifecycle now has explicit `Running → Suspended → Recovering → Running` state with one
  event-driven `resync_input_state(reason)` path for power, session, focus, hook, device, and
  startup recovery.
- Added an explicit `--elevated` UAC profile for controlling high-integrity Windows applications;
  the default tray/background profile remains non-elevated.

### Fixed

- Foreground NumPad interception while the NumFlow window has focus.
- Focus-independent NumPad delivery by removing winit's keyboard-only Raw Input registration once
  after Slint initialization, without claiming Raw Input from the hook recovery thread.
- HUD taskbar/Alt+Tab behavior.
- Num Lock and UI enabled-state synchronization.
- Glass material contrast and HUD background artifacts.
- UI sound attenuation and persistent runtime volume control.
- NumPad responsiveness after Windows Sleep/Hibernate by restoring `WH_KEYBOARD_LL` from Windows power events, reconciling Raw Input, clearing stale movement/hold state, and resynchronizing Num Lock without timer-based recovery delays.
- Resume callback ordering race observed on real Windows hardware: power callbacks are serialized and resume stages are monotonic, so a delayed `PBT_APMRESUMEAUTOMATIC` callback cannot regress an already-processed `PBT_APMRESUMESUSPEND` recovery.
- Resume/session Num Lock desynchronization by freezing the tracked NumFlow mode until the final interactive session recovery phase instead of accepting transient lock-screen Num Lock semantics.
- Premature pointer injection during the lock-screen-to-desktop transition: automatic/user power phases now re-arm the keyboard hook while pointer activation remains quiesced until `WTS_SESSION_UNLOCK`/desktop-ready.
- A failed relative mouse-movement injection no longer permanently disables NumFlow; movement is stopped safely while the enabled mode remains available for the next physical movement press.
- Physical Num Lock is now observed and passed through to Windows, removing the deferred replay
  failure window; tagged replay remains limited to explicit UI/lifecycle synchronization.
- Foreground executable/elevation, hook callback health, and Raw Input keyboard-device registration
  are logged so Task Manager/UIPI failures are distinguishable from lost keyboard hooks.
