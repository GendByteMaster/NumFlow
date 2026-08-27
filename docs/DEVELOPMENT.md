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
- NumPad key mapping and normalization;
- `SendInput` pointer injection;
- HUD placement helpers;
- single-instance protection;
- per-user Windows startup registration.

The hook callback must stay short and non-blocking.

## Running locally

Development run:

```powershell
cargo run --locked
```

Release build:

```powershell
cargo build --locked --workspace --release --all-features
```

The release executable is produced under the Cargo release target directory. Packaging is not finalized yet; do not treat a raw release binary as the final v0.1 distribution format.

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

The config model owns profile selection, HUD state, start-minimized/start-with-Windows state, motion values, precision/boost multipliers, selected mouse button, and bindings.

Configuration rules:

1. Deserialize into the typed model.
2. Validate schema/profile/binding invariants.
3. Recover invalid configuration to safe defaults.
4. Persist with atomic replacement.
5. Apply UI changes to the active runtime immediately where supported.

Do not introduce ad-hoc settings outside the typed configuration model.

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

### Idle behaviour

The runtime must not use a permanent high-frequency busy/poll loop.

Current behaviour:

- while no movement is active, the worker blocks until keyboard input or a runtime command arrives;
- while movement is active, the motion ticker runs at the configured 8 ms cadence;
- the Slint bridge wakes the UI event loop only when runtime events are ready;
- the command queue is bounded;
- the keyboard-hook event queue is bounded and the hook callback uses non-blocking delivery.

### Remaining Phase 11 concurrency work

`RuntimeEvent → UI` delivery is still an open reliability item. The final design must be bounded without blocking the pointer worker.

A correct solution should distinguish event importance:

- faults must not be silently lost;
- state snapshots can usually be coalesced to the latest value;
- UI lag must not stall input interception or pointer release;
- memory use must remain bounded during a stalled/minimized UI.

Do not solve this by replacing the event sender with a blocking bounded send from the pointer worker.

## Safety invariants

NumFlow is an accessibility utility, so input safety takes priority over convenience.

The following invariants should remain true after every change:

- disabled NumFlow does not intentionally move the pointer;
- interception is not enabled before the runtime is ready;
- hook callback work remains non-blocking;
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

The backend maps the physical NumPad path rather than relying only on logical key names. Automated normalization tests exist, but real Num Lock On/Off behaviour remains part of the manual release matrix.

### UIPI / elevated applications

`SendInput` is subject to Windows User Interface Privilege Isolation. A normal NumFlow process may be unable to inject input into an elevated application. This is an OS security boundary, not a reason to run NumFlow elevated by default.

### Startup registration

Start-with-Windows uses the current user's Run registry key. This is a per-user registration and does not require a machine-wide installer.

## Adding or changing bindings

Bindings belong to the typed profile configuration and core binding resolver. UI code should translate editor choices into `InputAction` values; it should not hard-code pointer behaviour itself.

Any new action should be considered across:

1. core action/state semantics;
2. binding serialization;
3. runtime dispatch;
4. UI editor representation;
5. HUD/tray feedback if relevant;
6. tests and migration/versioning impact.

## Testing principles

Prefer deterministic tests for core behaviour. Real pointer movement is not required to validate acceleration/state-machine math.

High-value automated areas include:

- state transitions and illegal-state prevention;
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
