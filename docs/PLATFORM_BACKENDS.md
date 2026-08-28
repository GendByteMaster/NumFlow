# Global input platform backends

NumFlow keeps bindings, controller state, motion, fail-safe effects, and pointer abstractions in
`numflow-core`. Global keyboard capture and pointer injection are operating-system capabilities and
must be implemented separately.

## Backend contract

Every production backend must provide:

- one process-wide global input listener with explicit singleton ownership;
- physical NumPad normalization into the shared `NumpadKey` model;
- focus-independent capture where the operating system permits it;
- explicit permission/integrity diagnostics;
- pointer movement/button injection with checked failures;
- suspend, session, device, and permission-loss recovery;
- fail-safe release of movement, pressed keys, and injected mouse buttons;
- no listener recreation for ordinary foreground changes.

The UI calls only `platform_input::prepare_after_ui`; platform-specific setup does not leak into
Slint callbacks.

## Windows

Implemented in `numflow-windows` using `WH_KEYBOARD_LL`, `SendInput`, WTS/power notifications,
WinEvent foreground diagnostics, and device-interface notifications. Slint/winit's keyboard Raw
Input registration is removed once after event-loop initialization because it makes low-level hook
delivery unreliable while NumFlow itself is foreground. Raw Input is not recreated during recovery.

Elevated windows are protected by UIPI. `--elevated` starts an explicit UAC-approved NumFlow
profile; the default tray/background profile remains non-elevated. A no-prompt production
accessibility deployment would instead require a signed binary, secure installation location, and
an approved `uiAccess` manifest. The normal instance must be closed before starting the elevated
profile because singleton ownership deliberately forbids two global listeners.

## Linux

The backend boundary exists in `src/platform_input/linux.rs`, but global capture is not yet
implemented. A production implementation must choose and test separate permission paths for
evdev/uinput, X11, and Wayland compositor/portal environments. NumFlow fails explicitly instead of
claiming that the current no-op runtime provides global capture.

## macOS

The backend boundary exists in `src/platform_input/macos.rs`, but global capture is not yet
implemented. A production implementation requires Accessibility consent, a `CGEventTap` listener,
event-source tagging to prevent recursion, and sleep/device permission recovery. NumFlow fails
explicitly instead of silently claiming global input support.
