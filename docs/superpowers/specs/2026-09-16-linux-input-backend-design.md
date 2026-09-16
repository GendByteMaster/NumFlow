# NumFlow Linux Input Backend Design

Status: Draft — architecture approved; awaiting written spec review
Branch: `feat/linux-input-backend`
Base: `dev/master`
Risk: High

## Goal

Implement a Linux global input backend that preserves NumFlow's existing NumPad-driven pointer semantics on both Wayland and X11 by using Linux input subsystem primitives rather than compositor-specific injection APIs.

The backend must:

- capture the physical NumPad independently of application focus;
- preserve normal keyboard input when Num Lock is On;
- consume NumPad pointer bindings when Num Lock is Off;
- inject pointer movement and mouse-button events through a virtual input device;
- fail safe if permissions, devices, virtual-device creation, or runtime processing fail;
- avoid running the NumFlow UI process as root;
- keep shared motion, bindings, and controller semantics in `numflow-core`.

## Non-goals

This change does not implement:

- X11/XTest-specific injection;
- XDG GlobalShortcuts as the primary capture path;
- XDG RemoteDesktop / libei / EIS injection;
- a kernel driver;
- root execution of the NumFlow UI;
- secure/login-screen control;
- arbitrary keyboard remapping outside the events that must be replayed after an exclusive keyboard grab.

Portal/libei support may be added later as an optional backend without changing the shared core contract.

## Rationale

Wayland does not provide a universal application-level global key interception API with the exact semantics NumFlow needs. Global shortcut portals register actions but do not provide a general raw physical-key interception and suppression path. NumFlow must be able to consume NumPad input while Num Lock is Off and restore ordinary numeric input while Num Lock is On.

Linux `evdev` exposes physical input event devices under `/dev/input`, and `uinput` allows a userspace process to create virtual keyboard and mouse devices. This gives NumFlow one compositor-independent path below both Wayland and X11.

The implementation should use the Rust `evdev` crate rather than custom unsafe ioctl bindings. The repository root denies unsafe code, and the Linux platform crate should also deny unsafe code.

References:

- Linux kernel uinput documentation: <https://docs.kernel.org/input/uinput.html>
- Linux input event codes: <https://docs.kernel.org/input/event-codes.html>
- Rust evdev crate: <https://docs.rs/evdev/>

## Architecture

```text
Physical keyboard(s)
        |
        v
    evdev reader
        |
        +---------------- Num Lock On ----------------+
        |                                              |
        |                                       replay keyboard
        |                                              |
        |                                              v
        |                                      uinput keyboard
        |
        +---------------- Num Lock Off ---------------+
                         |
                         v
                  NumPad normalization
                         |
                         v
                    numflow-core
                         |
                         v
                   pointer effects
                         |
                         v
                    uinput mouse
                         |
                         v
                Wayland / X11 session
```

The Linux-specific implementation lives in a new `numflow-linux` crate. The root `src/platform_input/linux.rs` remains a thin application boundary.

Proposed layout:

```text
crates/numflow-linux/
├── Cargo.toml
└── src/
    ├── lib.rs
    ├── capture.rs
    ├── devices.rs
    ├── lifecycle.rs
    ├── numlock.rs
    ├── permissions.rs
    ├── pointer.rs
    └── replay.rs

src/platform_input/linux.rs
```

## Startup order and fail-closed behavior

Exclusive keyboard capture is the highest-risk runtime action. NumFlow must never grab a physical keyboard until normal input can be replayed.

Startup order:

1. Detect the Linux graphical session and collect diagnostics.
2. Verify access to candidate `/dev/input/event*` devices.
3. Verify access to `/dev/uinput`.
4. Create the virtual replay keyboard with the required key and LED capabilities.
5. Create the virtual mouse with relative X/Y and left/right/middle button capabilities.
6. Start the virtual-device output/event-feedback workers.
7. Select eligible physical keyboard devices.
8. Only after steps 1-7 succeed, perform `EVIOCGRAB` through `evdev::Device::grab()`.
9. Start event forwarding and NumFlow runtime processing.

If any prerequisite fails before step 8, NumFlow returns an actionable backend error and leaves the physical keyboard untouched.

If a failure occurs after a grab:

1. stop NumFlow movement;
2. release every virtual mouse button that NumFlow believes is held;
3. ungrab all physical keyboard devices;
4. destroy/drop virtual devices;
5. surface the failure to application/runtime diagnostics.

No retry path may leave a keyboard grabbed while the replay device is unavailable.

## Physical keyboard discovery

The backend enumerates `/dev/input/event*` and identifies physical keyboard candidates from advertised capabilities rather than device-name matching alone.

A candidate must advertise `EV_KEY`, `KEY_NUMLOCK`, and the NumPad keys required by NumFlow. The replay keyboard is built from the union of supported keyboard capabilities required to preserve events from all captured devices.

The backend must exclude NumFlow-created virtual devices using stable NumFlow virtual-device identifiers and sysfs/uinput metadata so replayed events cannot recursively re-enter capture.

If multiple eligible physical keyboards are present, the backend may capture all eligible physical keyboards and multiplex them into the same replay/runtime path. Ownership and pressed-key state are tracked per physical device so disconnecting one device does not corrupt another device's state.

The first implementation does not require permanent high-frequency enumeration. Device-loss recovery triggers a bounded rescan with backoff. Successful steady-state operation remains event-driven.

## Keyboard replay

Because `EVIOCGRAB` is device-wide, NumFlow must replay ordinary keyboard events that it does not consume.

The virtual replay keyboard exposes the capability union required by captured physical keyboards. Events are processed in original packet order and preserve synchronization boundaries where relevant.

Rules:

- non-NumPad keys are replayed unchanged;
- when Num Lock is On, NumPad events are replayed unchanged;
- when Num Lock is Off, configured NumFlow NumPad bindings are consumed instead of replayed;
- unrelated events from the same physical keyboard must not be silently discarded;
- generated events from NumFlow virtual devices must never be recaptured;
- key-up events must always be reconciled so a transition cannot leave a virtual key logically held.

The replay layer is not a general remapper. Its only purpose is to preserve events hidden from the desktop by the exclusive grab.

## Privacy boundary

The backend necessarily observes events from captured keyboard devices in memory so that it can replay non-NumPad input after `EVIOCGRAB`.

NumFlow must therefore enforce these privacy constraints:

- never persist ordinary key events;
- never send ordinary key events over the network;
- never include ordinary key contents in telemetry, diagnostics, crash reports, or normal logs;
- do not expose a general-purpose key history API;
- retain only the minimum transient pressed-state needed for correct replay and recovery;
- diagnostics may identify the device and failure type, but not the text or key sequence typed by the user.

## Num Lock semantics

Num Lock remains the authoritative mode switch, matching the Windows product behavior:

- Num Lock On: ordinary NumPad behavior is preserved and NumFlow pointer interception is disabled.
- Num Lock Off: configured NumPad keys drive NumFlow pointer actions.

The physical Num Lock key itself is replayed so the desktop/kernel retains normal lock-state ownership.

The backend tracks Num Lock state from Linux input state and lock events rather than inventing an independent toggle state. On startup and after recovery it re-reads current LED/key state from the physical device where available.

The virtual replay keyboard exposes Num Lock LED capability. LED feedback received for the virtual keyboard is mirrored to captured physical keyboards that support `LED_NUML` by sending output LED events through evdev. Failure to mirror an LED is diagnostic and must not cause a stuck exclusive grab; logical Num Lock operation takes precedence over LED cosmetics.

Auto-repeat of `KEY_NUMLOCK` must not cause multiple logical transitions from one physical press.

## NumPad mapping

The Linux backend maps Linux key codes into the existing shared `NumpadKey` / binding model instead of duplicating pointer semantics.

Required default behavior remains:

- `8` up;
- `2` down;
- `4` left;
- `6` right;
- `7` up-left;
- `9` up-right;
- `1` down-left;
- `3` down-right;
- `5` left click;
- `+` double click;
- `0` hold selected button;
- `.` release held button;
- `/` select left;
- `*` select right;
- `-` select middle.

Movement acceleration, precision, selected-button state, held-button state, click semantics, and fail-safe release remain owned by `numflow-core` and the existing application runtime.

## Pointer injection

The virtual mouse is created through uinput and exposes only the capabilities NumFlow needs:

- `EV_REL` / `REL_X`;
- `EV_REL` / `REL_Y`;
- `EV_KEY` / `BTN_LEFT`;
- `EV_KEY` / `BTN_RIGHT`;
- `EV_KEY` / `BTN_MIDDLE`.

Pointer effects produced by the shared runtime are translated to bounded batches of uinput events and emitted with synchronization.

The pointer backend reports every failed emit operation instead of pretending that movement or a button transition succeeded.

Held-button state follows the same safety invariant as Windows: shutdown, disable, backend failure, or device-session teardown releases the actually held button.

## Lifecycle and recovery

The Linux backend has one process-wide supervisor.

Responsibilities:

- own physical-device handles and grabs;
- own virtual keyboard and mouse devices;
- start and stop event workers;
- detect device EOF/removal and fatal I/O errors;
- coordinate fail-safe release before teardown;
- rebuild the backend after device loss with bounded retry/backoff;
- reject duplicate backend ownership in the same process.

Steady-state operation is event-driven. Recovery polling is allowed only while the backend is degraded or waiting for a required device/permission to return.

Suspend/resume validation is required. If an input descriptor becomes invalid after resume, the backend tears down safely and performs discovery again instead of retaining stale device ownership.

## Permissions

Risk is High because this backend requires permission to read and exclusively grab physical keyboard event devices.

NumFlow must not require the GUI application to run as root and must not modify live permissions by itself.

The production permission model should grant the logged-in interactive user narrowly scoped access to:

- keyboard event devices NumFlow needs to read/grab;
- `/dev/uinput` for virtual-device creation.

The preferred first implementation is a documented installation-time udev/uaccess policy, or an equivalent distribution-supported mechanism. It must not use world-writable (`0666`) permissions for input devices and must not make all `/dev/input/event*` nodes broadly writable.

Installing or activating a udev permission policy is a consequential High-risk system change and requires an explicit user/admin action. Repository implementation may prepare the rule and documentation, but NumFlow must not silently install it at runtime.

Permission diagnostics must distinguish:

- cannot read physical input device;
- cannot grab physical keyboard;
- cannot open `/dev/uinput`;
- virtual-device creation rejected.

Running NumFlow with `sudo` is not the normal solution and should not be presented as the production configuration.

A later hardening phase may replace direct device-node access with logind/libseat-provided file descriptors without changing the `numflow-linux` public contract.

## Concurrency

Physical input reading must not block the UI thread.

The backend should use dedicated blocking workers or an equivalent event-driven executor, with bounded channels into the existing runtime. Input capture, replay, pointer output, and lifecycle control must not form an unbounded feedback loop.

Critical release/ungrab operations must remain available even when the UI is stalled.

## Errors and diagnostics

Linux backend initialization returns structured errors that can be rendered as concise user diagnostics.

At minimum diagnostics include:

- session type (`wayland`, `x11`, or unknown);
- selected physical device path/name;
- whether the device was successfully grabbed;
- `/dev/uinput` availability;
- virtual keyboard/mouse creation status;
- current Num Lock state when known;
- last device/replay/pointer I/O failure;
- recovery state and retry reason.

Ordinary key contents must never be logged.

## Security and safety

Risk level: High.

Why High: implementation requires permission changes around protected input device nodes and observes system-wide keyboard events for replay while a device is exclusively grabbed.

Blast radius:

- local interactive input session;
- captured physical keyboards;
- local privacy if ordinary key events were accidentally logged or persisted;
- desktop pointer behavior through the NumFlow virtual mouse.

Mandatory controls:

- create replay devices before grabbing physical input;
- never keep a grab if replay or supervisor startup fails;
- keep ownership in RAII types whose drop path attempts ungrab/release;
- release virtual mouse buttons before backend teardown;
- exclude NumFlow virtual devices from discovery;
- never persist or transmit ordinary keyboard events;
- do not execute shell commands or expose privileged IPC;
- no setuid NumFlow executable;
- no world-writable input-device permissions;
- keep Linux platform code isolated from `numflow-core`.

Recovery strategy: on any fatal backend error, fail open for the user's physical keyboard by ungrabbing it, even if that disables NumFlow pointer control.

Rollback strategy:

- application rollback: disable/remove the Linux backend and run without physical grabs;
- runtime rollback: terminate NumFlow; kernel file-descriptor teardown releases `EVIOCGRAB` ownership;
- permission rollback: remove the NumFlow-specific udev/uaccess rule through an explicit admin action and reload rules/reconnect devices;
- no persistent keyboard remapping state is written by NumFlow.

## Testing

### Pure/unit tests

Add deterministic tests for:

- Linux key-code to shared NumPad mapping;
- Num Lock On/Off routing decisions;
- repeat suppression for Num Lock transitions;
- consumed versus replayed event classification;
- virtual-device recursion filtering;
- multi-device ownership/state bookkeeping;
- button fail-safe release decisions;
- reconnect/backoff state transitions;
- permission/error classification;
- privacy-safe diagnostic formatting.

Tests must not require `/dev/input` or `/dev/uinput` for pure routing logic.

### Linux integration tests

Where CI privileges permit, test virtual input using temporary uinput devices. Privileged-device tests that cannot run on hosted CI must be explicitly marked and documented rather than falsely passing as unit coverage.

### Manual validation matrix

On Omarchy/Hyprland Wayland, validate:

1. Num Lock On types digits normally.
2. Num Lock Off prevents NumPad digits from reaching focused applications.
3. 8/2/4/6 movement.
4. 7/9/1/3 diagonal movement.
5. 5 click.
6. + double-click.
7. 0 hold -> movement -> . release.
8. /, *, - selected-button changes.
9. background/unfocused operation.
10. terminal, browser, and native Wayland applications.
11. rapid Num Lock transitions and key autorepeat.
12. unplug/replug keyboard.
13. sleep/resume.
14. normal shutdown and forced termination leave the keyboard usable.
15. permission/startup failure does not grab the keyboard.
16. physical Num Lock LED remains synchronized when supported.
17. ordinary typing is not logged or persisted.

Repeat the core movement/click/Num Lock matrix on an X11 session when available.

## CI and repository integration

Add `crates/numflow-linux` to the workspace and make it a target-specific dependency of the root application.

Windows CI must remain green. Add a Linux CI job that at minimum runs:

```text
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release --all-features
```

The Linux job does not claim physical input integration success unless the runner actually exposes required devices and permissions.

## Documentation

The implementation PR must update:

- `README.md`;
- `docs/DEVELOPMENT.md`;
- `docs/PLATFORM_BACKENDS.md`;
- `docs/INSTALLATION.md`;
- `docs/RELEASE_CHECKLIST.md`;
- Linux permission/setup documentation added by the implementation.

Documentation must clearly distinguish implemented evdev/uinput behavior from future portal/libei work.

## Acceptance criteria

The implementation is ready for review when all of the following are true:

1. `numflow-linux` exists as a separate platform crate and the root Linux boundary delegates to it.
2. The virtual keyboard and mouse are created successfully before any physical keyboard grab.
3. Failure to create replay/output devices leaves all physical keyboards ungrabbed.
4. Num Lock On preserves ordinary keyboard/NumPad input.
5. Num Lock Off consumes configured NumPad controls and drives existing `numflow-core` pointer semantics.
6. Non-consumed keyboard events are replayed without recursive capture.
7. Ordinary key events are neither persisted nor included in diagnostics/telemetry.
8. Pointer failures, shutdown, disable, device loss, and backend teardown release held mouse buttons.
9. Fatal capture/replay failures ungrab physical keyboards.
10. Linux permission errors are explicit and do not require running the GUI as root.
11. Permission rules are prepared/documented but are not silently installed by the application.
12. Windows behavior and Windows quality gates remain unchanged/green.
13. Linux format, Clippy, tests, and release build are green.
14. The Omarchy/Hyprland manual matrix is completed before the Linux backend is described as production-ready.

## Deferred work

Deferred to separate changes:

- XDG GlobalShortcuts integration for optional user-configured shortcuts;
- XDG RemoteDesktop + libei/EIS when compositor support is sufficiently available;
- logind/libseat file-descriptor acquisition;
- packaging for non-Arch distributions;
- compositor-specific enhancements;
- secure/login-screen input.
