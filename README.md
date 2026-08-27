<div align="center">
  <img src="assets/numflow-icon.svg" alt="NumFlow icon" width="180" />

# NumFlow

**Precise pointer control from your keyboard.**

A lightweight Windows-first accessibility utility for controlling the mouse pointer with the NumPad, built with Rust and Slint.
</div>

## Status

NumFlow v0.1 is in active development on `dev/master`.

The main application path is already wired end to end: global NumPad capture, background pointer runtime, time-based motion, mouse-button selection/click/drag behaviour, Slint settings UI, HUD feedback, profiles, custom bindings, persistent configuration, tray lifecycle, startup registration, single-instance protection, and fail-safe pointer release.

Recent reliability work also removed high-frequency idle polling from the background runtime. The worker now sleeps until input or a command arrives while motion is idle, and the Slint bridge uses event-driven wakeups instead of a fixed UI polling timer.

> **Branch policy:** all v0.1 development happens only in `dev/master`. `master` must remain untouched until a release/merge is explicitly approved.

## Current capabilities

- Global Windows NumPad input through `WH_KEYBOARD_LL`.
- Pointer movement and mouse-button injection through `SendInput`.
- Smooth time-based movement with acceleration and diagonal normalization.
- Configurable speed, acceleration, precision mode, and per-profile bindings.
- Left, right, and middle mouse-button selection.
- Single click, double click, hold/drag lock, and release.
- Main Slint settings window with live status and NumPad visualization.
- Status icons for NumFlow state, selected mouse button, and precision mode.
- Built-in `Normal`, `Precision`, and `Fast` profiles.
- Editable NumPad bindings that apply at runtime.
- HUD feedback and persistent drag-state feedback.
- Persistent TOML configuration with schema validation and atomic writes.
- Safe fallback to defaults when configuration is invalid/corrupted.
- Start-minimized and start-with-Windows configuration.
- Single-instance protection.
- Fail-safe release of a held mouse button during disable/shutdown/error paths.
- Event-driven UI runtime notifications.
- Idle background runtime that blocks instead of waking on every motion tick.

## Default controls

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
| `+` | Double click selected mouse button |
| `0` | Hold selected mouse button / start drag lock |
| `.` | Release held mouse button |
| `/` | Select left mouse button |
| `*` | Select right mouse button |
| `-` | Select middle mouse button |

Bindings are configurable; the core logic does not depend on fixed virtual-key codes.

## Running from source

### Requirements

- Windows development environment.
- Rust `1.98`.
- Cargo with the committed `Cargo.lock`.

Clone the repository, switch to the development branch, and run:

```powershell
git switch dev/master
cargo run --locked
```

Useful quality commands:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release --all-features
```

See [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) for the development and architecture notes.

## Configuration

On Windows, NumFlow stores its user configuration at:

```text
%APPDATA%\NumFlow\config.toml
```

The configuration is versioned (`schema_version = 1`) and currently contains:

- active profile;
- HUD state;
- start-minimized state;
- start-with-Windows state;
- profile speed / maximum speed / acceleration;
- precision and boost multipliers;
- selected mouse button;
- custom NumPad bindings.

Writes are atomic. Invalid or unsupported configuration is recovered to safe defaults rather than being applied partially.

## Architecture

```text
NumFlow application
├── Slint UI / tray / HUD
├── Application layer
│   ├── config lifecycle
│   ├── UI ↔ runtime bridge
│   └── background runtime orchestration
├── numflow-core
│   ├── bindings
│   ├── state machine
│   ├── motion engine
│   └── pointer effects
└── numflow-windows
    ├── WH_KEYBOARD_LL hook
    ├── key normalization
    ├── SendInput pointer backend
    ├── HUD placement helpers
    ├── single-instance guard
    └── Windows startup registration
```

`numflow-core` remains platform-independent. Win32-specific code stays in `numflow-windows`, and the UI does not call Win32 APIs directly.

## Runtime and safety

NumFlow treats input interception and drag state as safety-critical:

- the keyboard hook callback does not perform blocking application work;
- the physical keyboard queue is bounded and uses non-blocking delivery from the hook callback;
- the runtime command queue is bounded;
- interception starts disabled until the application runtime is ready;
- pointer motion ticks run only while motion is active;
- while idle, the worker blocks on real input/commands instead of a high-frequency sleep loop;
- UI wakeups are event-driven;
- disable/shutdown paths stop movement and release any held mouse button.

One Phase 11 reliability item is still intentionally open: the `RuntimeEvent → UI` event path needs bounded/coalescing semantics that preserve faults and state transitions without ever blocking the pointer worker.

## CI

GitHub Actions runs the Windows quality gate on `dev/master` using Rust `1.98`:

1. `cargo fmt --all -- --check`
2. `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
3. `cargo test --locked --workspace --all-features`
4. `cargo build --locked --workspace --release --all-features`

The Phase 11 idle-runtime change passed the full gate with 78 tests (30 application, 30 core, 18 Windows) before delivery to `dev/master`.

## v0.1 work still open

The remaining work is primarily validation and release readiness rather than major product functionality:

- design and validate bounded/coalesced `RuntimeEvent → UI` delivery;
- long-running resource/soak validation;
- manual Windows 10/11 validation;
- DPI scaling validation from 100% through 200%;
- Num Lock on/off validation;
- foreground/background application validation;
- multi-monitor and sleep/resume validation;
- final accessibility and keyboard-navigation pass;
- Windows executable icon/metadata and packaging strategy;
- release artifact checksums, changelog, usage notes, and known limitations.

See [`docs/RELEASE_CHECKLIST.md`](docs/RELEASE_CHECKLIST.md) and the [v0.1 roadmap](https://github.com/GendByteMaster/NumFlow/issues/1).

## Windows input limitation

NumFlow uses the Win32 `SendInput` API for simulated pointer movement and mouse-button events. Windows User Interface Privilege Isolation (UIPI) permits input injection only into applications running at an equal or lower integrity level. A normally launched NumFlow process therefore might not control an elevated/admin application.

`SendInput` does not reliably report that UIPI was the specific reason an injection was blocked, so this limitation must not be treated as a random pointer-backend failure. NumFlow should run without elevation by default; elevation is not a general workaround and is outside the v0.1 default design.

## Platform direction

v0.1 is Windows-first. The core is intentionally kept platform-independent so Linux backends can be added later without rewriting the state machine, bindings, or motion engine.

## License

MIT. See [`LICENSE`](LICENSE).
