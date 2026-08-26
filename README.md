<div align="center">
  <img src="assets/numflow-icon.svg" alt="NumFlow icon" width="180" />

# NumFlow

**Precise pointer control from your keyboard.**

A lightweight accessibility utility for controlling the mouse pointer with the NumPad, built with Rust and Slint.
</div>

## Status

Early development / architecture phase.

> Development branch: `dev/master`. The `master` branch is intentionally left untouched until a release or merge is explicitly approved.

## Core controls

| Key | Action |
| --- | --- |
| `8` | Move up |
| `2` | Move down |
| `4` | Move left |
| `6` | Move right |
| `7` | Move up-left |
| `9` | Move up-right |
| `1` | Move down-left |
| `3` | Move down-right |
| `5` | Click selected mouse button |
| `+` | Double click |
| `0` | Hold selected mouse button |
| `.` | Release selected mouse button |
| `/` | Select left mouse button |
| `*` | Select right mouse button |
| `-` | Select middle mouse button |

## Direction

NumFlow is Windows-first, but the core is intended to remain platform-independent so Linux backends can be added later.

Planned capabilities include configurable pointer speed and acceleration, precision mode, custom bindings, visual HUD feedback, profiles, tray integration, safe drag-lock handling, and accessibility-focused keyboard control.

## Planned stack

- Rust
- Slint
- `windows` crate / Win32 APIs on Windows
- `WH_KEYBOARD_LL` for global keyboard input
- `SendInput` for pointer movement and mouse button events
- Serde-based persistent configuration

## Windows input limitation

NumFlow uses the Win32 `SendInput` API for simulated pointer movement and mouse-button events. Windows User Interface Privilege Isolation (UIPI) permits input injection only into applications running at an equal or lower integrity level. A normally launched NumFlow process therefore might not control an elevated/admin application.

`SendInput` does not reliably report that UIPI was the specific reason an injection was blocked, so this limitation must not be treated as a random pointer-backend failure. NumFlow should run without elevation by default; elevation is not a general workaround and is outside the v0.1 default design.

See the project roadmap issue for the full development plan.
