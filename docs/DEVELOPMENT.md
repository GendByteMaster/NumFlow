# NumFlow development guide

This document describes the current v0.1 development model. The authoritative product plan is tracked in [Roadmap #1](https://github.com/GendByteMaster/NumFlow/issues/1).

## Branch policy

All v0.1 development is performed only in:

```text
dev/master
```

Do not commit directly to `master`. `master` is reserved for an explicitly approved release merge.

Before starting work:

```powershell
git fetch origin
git switch dev/master
git pull --ff-only origin dev/master
```

## Toolchain

The workspace currently targets:

- Rust 1.98;
- Rust edition 2024;
- Slint 1.17.1;
- Windows APIs through the `windows` crate;
- Cargo lockfile reproducibility through `--locked`.

The root crate denies unsafe Rust. Win32 integration is isolated in the Windows backend crate.

## Workspace layout

```text
NumFlow/
├── Cargo.toml
├── Cargo.lock
├── build.rs
├── assets/
├── ui/
├── src/
│   ├── app.rs
│   ├── bindings_ui.rs
│   ├── config.rs
│   ├── error.rs
│   ├── hud.rs
│   ├── main.rs
│   └── runtime.rs
├── crates/
│   ├── numflow-core/
│   └── numflow-windows/
└── docs/
```

### Root application

The root application owns the Slint-facing application layer, persisted settings, background runtime orchestration, HUD state, tray state, and synchronization between UI state and the core/runtime.

### `numflow-core`

`numflow-core` is platform-independent. It contains the domain model and deterministic logic:

- NumPad key abstractions and bindings;
- input actions;
- controller state;
- mouse-button and drag state;
- time-based pointer motion;
- acceleration and precision calculations;
- pointer effects.

It must not depend on Win32 or Slint APIs.

### `numflow-windows`

`numflow-windows` owns platform integration:

- `WH_KEYBOARD_LL` global keyboard hook;
- physical Num Lock observation and tagged replay for explicit synchronization;
- NumPad key mapping and normalization;
- asynchronous mode-switch audio feedback;
- `SendInput` pointer injection;
- HUD placement helpers;
- single-instance protection;
- per-user Windows startup registration.

The hook callback must stay short and non-blocking. Blocking work such as audio playback must stay outside the hook thread.

## Running locally

Development run:

```powershell
cargo run --locked
```

Release build:

```powershell
cargo build --locked --workspace --release --all-features
```

The release executable is produced under the Cargo release target directory. Packaged Windows distributions are built from that executable as a WiX MSI and a portable ZIP; see `RELEASING.md`.

## Required quality gate

Before considering a change ready:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release --all-features
```

GitHub Actions runs the same Windows-oriented quality gate on `dev/master`.

Do not bypass `--locked` in CI or release validation. Dependency changes must update `Cargo.lock` deliberately.

## Configuration lifecycle

Windows user configuration is stored at:

```text
%APPDATA%\NumFlow\config.toml
```

Current schema version:

```text
1
```

The config model owns profile selection, HUD state, start-minimized/start-with-Windows state, interface-sound enable/volume state, motion values, precision/boost multipliers, selected mouse button, and bindings.

Configuration rules:

1. Deserialize into the typed model.
2. Validate schema/profile/binding invariants.
3. Recover invalid configuration to safe defaults.
4. Persist with atomic replacement.
5. Apply UI changes to the active runtime immediately where supported.

Do not introduce ad-hoc settings outside the typed configuration model.

The Windows audio service is controlled through the typed configuration model: Advanced settings persist both the interface-sound enable state and the 0–100% volume, and runtime updates apply without changing the Windows system mixer.

## Runtime model

The Windows application has three important event domains:

```text
Global keyboard hook
        ↓
bounded physical-key channel
        ↓
background runtime
        ↓
core state / pointer effects
        ↓
Windows pointer backend + UI/HUD state
```

### Num Lock mode ownership

Num Lock is the authoritative global mode switch while NumFlow is running:

```text
physical Num Lock press
        ↓
WH_KEYBOARD_LL observes the edge and updates NumFlow mode immediately
        ↓
Windows processes the same physical event and owns Num Lock state + LED
        ↓
NumLockChanged event → runtime/UI/audio
```

Required behaviour:

- Num Lock On → NumFlow pointer interception is disabled and NumPad keys are available for ordinary numeric input;
- Num Lock Off → NumFlow pointer interception is enabled and the NumPad controls the system cursor;
- the physical Num Lock key is passed through; Windows remains the owner of its actual toggle state;
- NumFlow's own injected replay is identified by a private `dwExtraInfo` tag and must pass through without re-entering the mode state machine;
- injected Num Lock input from other software is not consumed, but NumFlow mirrors that state change;
- explicit UI/lifecycle synchronization reports a failed `SendInput` instead of pretending that
  Windows accepted the requested toggle;
- key autorepeat must not toggle the mode more than once per physical press;
- switching to Num Lock On must immediately stop interception and then let the runtime safely release any held mouse-button state;
- the mode switch must work while the settings window is unfocused/minimized.

Audio feedback is dispatched by the runtime to a separate bounded worker. The keyboard hook must never call blocking audio playback directly.

### Idle behaviour

The runtime must not use a permanent high-frequency busy/poll loop.

Current behaviour:

- while no movement is active, the worker blocks until keyboard input or a runtime command arrives;
- while movement is active, the motion ticker runs at the configured 8 ms cadence;
- the Slint bridge wakes the UI event loop only when runtime events are ready;
- the command queue is bounded;
- the keyboard-hook event queue is bounded and the hook callback uses non-blocking delivery.

### Runtime event backpressure

`RuntimeEvent → UI` delivery uses a bounded, non-blocking queue. The runtime is the single producer; when the UI queue is full it evicts one stale UI event before retrying delivery, while the latest runtime event carries an authoritative state snapshot used to resynchronize UI/tray/HUD state.

Manual release validation must still exercise stalled/minimized UI, fault delivery, and long-running memory behaviour. Do not replace the non-blocking design with a blocking send from the pointer worker.

## Safety invariants

NumFlow is an accessibility utility, so input safety takes priority over convenience.

The following invariants should remain true after every change:

- disabled NumFlow does not intentionally move the pointer;
- Num Lock On means NumFlow does not intercept ordinary NumPad number entry;
- physical Num Lock handling never causes a recursive/double toggle;
- Windows Num Lock state/LED remains synchronized with NumFlow mode in the normal replay path;
- physical toggles are always passed through; explicit replay failure is reported and leaves the
  runtime in a safe state instead of silently claiming synchronization;
- interception is not enabled before the runtime is ready;
- hook callback work remains non-blocking apart from the minimal synchronous `SendInput` replay needed to preserve immediate Num Lock semantics;
- audio playback never blocks the hook;
- selected button and physically held button remain separate state;
- release always targets the actually held button;
- disable/shutdown/error handling releases a held button;
- shutdown is safe to call repeatedly;
- changing UI settings must not leave core/runtime state desynchronized;
- a stalled UI must not block critical pointer/input processing.

## UI and accessibility direction

The UI uses a compact hierarchy inspired by Apple Human Interface Guidelines while remaining a Windows application rather than imitating macOS chrome.

Keep these constraints:

- one compact primary settings window;
- clear On/Off status;
- Num Lock mode must be understandable from status text/icon without relying only on the keyboard LED;
- selected left/right/middle mouse mode must be visible with text/icon, not color alone;
- visible keyboard focus;
- controls should have accessible labels where Slint/platform support allows;
- avoid unnecessary animation;
- do not trap keyboard navigation;
- layouts must remain usable under Windows DPI scaling from 100% to 200%;
- drag-lock state must remain understandable when the main window is not focused.

Manual accessibility validation is still required before v0.1 release.

## Windows-specific notes

### Num Lock

NumFlow observes the physical Num Lock key with `WH_KEYBOARD_LL`, toggles runtime mode on the first
key-down edge, and then passes the original event to Windows. This makes Windows the owner of the
actual Num Lock toggle and LED, with no deferred replay window. A tagged synthetic Num Lock down/up
pair is reserved for explicit UI requests and lifecycle repair; NumFlow recognizes that replay and
does not feed it back into the mode state machine.

The backend still maps the physical NumPad path rather than relying only on logical key names. Automated transition/replay tests exist, but real Num Lock On/Off behaviour, LED synchronization, background operation, rapid toggling, external injected Num Lock events, and explicit UI/lifecycle replay failure handling remain part of the manual release matrix.

### UIPI / elevated applications

`SendInput` is subject to Windows User Interface Privilege Isolation. A normal NumFlow process may be unable to inject input into an elevated application. This is an OS security boundary, not a reason to run NumFlow elevated by default.

Task Manager is therefore a diagnostic target, not a reason to reinstall hooks. Foreground changes
log the target executable/integrity/elevation and pointer injection failures log the same target
context. `numflow.exe --elevated` starts the explicit UAC-approved elevated profile. The default
tray/background profile remains non-elevated. A no-prompt deployment must ship a signed binary from
a secure location with an appropriate `uiAccess` manifest. Neither profile can control the secure
desktop.

See `docs/PLATFORM_BACKENDS.md` for the shared backend contract and the separate Windows, Linux,
and macOS permission/input architecture.

### Startup registration

Start-with-Windows uses the current user's Run registry key. This is a per-user registration and does not require a machine-wide installer.

## Adding or changing bindings

Bindings belong to the typed profile configuration and core binding resolver. UI code should translate editor choices into `InputAction` values; it should not hard-code pointer behaviour itself.

Num Lock is reserved as the application mode switch and must not be exposed as a remappable profile binding in v0.1.

Any new action should be considered across:

1. core action/state semantics;
2. binding serialization;
3. runtime dispatch;
4. UI editor representation;
5. HUD/tray feedback if relevant;
6. tests and migration/versioning impact.

## Testing principles

Prefer deterministic tests for core behaviour. Real pointer movement is not required to validate acceleration/state-machine math.

Black-box behavior tests live in the repository-level `tests/` directory. The portable core
contract is covered by `tests/core_behavior.rs`; Windows keyboard mapping and normalization are
covered by `tests/windows_keyboard.rs`; and explicitly ignored interactive hook smoke tests live
in `tests/windows_system.rs`. Private runtime, Win32 message-ordering, pointer-structure, and UI
helper tests remain beside their implementation so production visibility is not widened for tests.
See [`TESTING.md`](TESTING.md) for the environment classes and manual regression matrix.

High-value automated areas include:

- state transitions and illegal-state prevention;
- Num Lock edge/repeat handling and tagged replay construction;
- diagonal normalization;
- acceleration and speed clamps;
- frame-rate independence;
- immediate movement stop on release;
- config round-trip and corruption recovery;
- key normalization;
- queue/backpressure behaviour;
- shutdown/fail-safe behaviour.

Windows interaction that cannot be faithfully reproduced in unit tests belongs in the manual release checklist rather than being marked complete from CI alone.

## Documentation updates

When behaviour changes, update documentation in the same `dev/master` change when practical:

- `README.md` for user/developer-visible behaviour;
- this document for architecture or development workflow;
- `docs/RELEASE_CHECKLIST.md` for release evidence and pending manual verification;
- Roadmap #1 for product-phase tracking/evidence.


## Windows distribution development

Distribution changes must preserve the normal Rust quality gate and prove that the MSI and portable archive can be built from the same release executable. WiX authoring lives in `installer/NumFlow.wxs`; release automation lives in `.github/workflows/release.yml`.

The MSI is a per-machine x64 package for `Program Files`, while `Start with Windows` remains a per-user runtime preference implemented by `numflow-windows`. Autostart invokes the installed or portable executable with `--background`; do not move this responsibility into the installer or force startup on users.

For local packaging commands and tag/version rules, see [`RELEASING.md`](RELEASING.md). For end-user behaviour, see [`INSTALLATION.md`](INSTALLATION.md).
