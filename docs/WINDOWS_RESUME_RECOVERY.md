# Windows suspend/resume recovery

This document describes how NumFlow restores its Windows input runtime after Sleep, Hibernate, and Resume.

The implementation is Windows-specific and lives in `crates/numflow-windows`. It does not change NumPad bindings, UI behavior, tray/HUD lifetime, startup registration, or the `--background` launch mode.

## Problem

Before resume recovery was added, the global `WH_KEYBOARD_LL` hook ran on its own Win32 message-loop thread, but NumFlow did not subscribe that thread to Windows power lifecycle notifications.

That created three related failure modes after Sleep/Hibernate:

1. NumFlow had no event-driven signal telling the input runtime that Windows had resumed.
2. A low-level keyboard hook could no longer be assumed to be usable after the power transition, but there is no reliable Win32 API that answers whether a `WH_KEYBOARD_LL` hook is still operational.
3. The Raw Input keyboard registration removed after Slint/winit initialization was not reconciled again after resume.

As a result, ordinary Windows keyboard input could already be responsive while NumFlow remained temporarily unable to consume NumPad input. Stale movement, key-normalizer, or drag/hold state also had no explicit lifecycle reset path.

## Design goals

Resume recovery must remain:

- event-driven;
- fast and independent of arbitrary sleeps such as `sleep(5s)`;
- safe against duplicate hooks;
- safe for active drag/hold state;
- compatible with tray/HUD/background operation;
- compatible with `Start with Windows` and `--background`;
- independent from the visibility of the settings window.

The settings window is never opened as part of recovery.

## Windows lifecycle notifications

The keyboard-hook thread registers for suspend/resume notifications with `RegisterSuspendResumeNotification` in callback mode.

The callback itself stays minimal. It maps the Windows power event to an internal `WM_APP` message and posts that message to the existing keyboard-hook thread with `PostThreadMessageW`.

Handled events:

| Windows event | NumFlow action |
| --- | --- |
| `PBT_APMSUSPEND` | enter suspend fail-safe state |
| `PBT_APMRESUMEAUTOMATIC` | perform immediate resume recovery |
| `PBT_APMRESUMECRITICAL` | perform immediate resume recovery |
| `PBT_APMRESUMESUSPEND` | reconcile interactive-session state and retry recovery if required |

No polling loop is introduced.

## Suspend path

When suspend is detected NumFlow:

1. disables NumPad interception;
2. clears the tracked Num Lock key-down edge state;
3. sends a fail-safe lifecycle event into the existing runtime path;
4. resets transient runtime input state through the normal Num Lock/state-machine handling;
5. records `suspend detected` in diagnostics.

The lifecycle event uses the same runtime path that already knows how to stop pointer motion and release a NumFlow-owned held mouse button.

## Resume recovery

The primary recovery path runs on `PBT_APMRESUMEAUTOMATIC`/`PBT_APMRESUMECRITICAL`:

```text
Windows resume event
        ↓
interception disabled
        ↓
retire previous WH_KEYBOARD_LL handle
        ↓
install replacement WH_KEYBOARD_LL
        ↓
reconcile process Raw Input keyboard registration
        ↓
clear transient runtime input state
        ↓
restore authoritative Num Lock / NumFlow mode
        ↓
restore interception if NumFlow mode is active
```

### Hook recovery

There is no reliable `WH_KEYBOARD_LL` liveness query. NumFlow therefore uses deterministic re-arming instead of pretending that a stored `HHOOK` value proves that the hook still works.

Recovery first calls `UnhookWindowsHookEx` for the previous handle. `ERROR_INVALID_HOOK_HANDLE` is treated as "already retired". A replacement hook is installed only after the old hook is confirmed retired.

This ordering is a safety invariant: NumFlow must never intentionally leave two active global keyboard hooks installed at the same time.

If re-arming fails, interception remains disabled rather than running in an uncertain partial state.

`PBT_APMRESUMESUSPEND` provides a second event-driven recovery opportunity when the interactive user session becomes active. It is used as a retry point if the earlier automatic-resume re-arm failed; it is not a timer-based retry.

### Raw Input reconciliation

NumFlow removes winit's process-wide raw-keyboard device-event registration because that registration can interfere with `WH_KEYBOARD_LL` delivery while a NumFlow window owns foreground focus.

The removal is intentionally idempotent and is repeated after resume. Raw mouse registration is not modified.

### Num Lock and NumFlow state

The tracked Num Lock state remains the authoritative mode relation:

- Num Lock On → NumFlow pointer interception Off;
- Num Lock Off → NumFlow pointer interception On.

Resume recovery first dispatches a fail-safe cleanup state and then re-dispatches the authoritative tracked Num Lock state. This reuses the normal runtime/state-machine path instead of introducing a second resume-only state machine.

The cleanup/restore sequence resets the keyboard normalizer, stops movement, releases NumFlow-owned held-button state, and then restores the correct NumFlow On/Off mode.

Interception is only re-enabled when both the replacement hook is installed and the lifecycle state events were delivered successfully.

## Diagnostics

The Windows recovery path emits concise lifecycle diagnostics:

```text
NumFlow: suspend detected
NumFlow: resume detected
NumFlow: hook restored
NumFlow: NumLock resynced (num_lock_on=..., numflow_enabled=...)
```

Hook-recovery and Raw Input reconciliation failures are also logged. These diagnostics are intended for troubleshooting real hardware Sleep/Hibernate issues without adding timing delays to the input path.

## Safety invariants

Changes to this subsystem must preserve the following rules:

- never create a replacement hook before the previous hook is retired;
- never enable interception while hook recovery is incomplete;
- never keep stale pointer movement active across suspend/resume;
- never keep a NumFlow-owned mouse hold latched across lifecycle cleanup;
- do not rebuild or close the tray/HUD/background runtime during recovery;
- do not show the main settings window during recovery;
- do not change NumPad bindings as part of lifecycle recovery;
- do not use arbitrary multi-second sleeps as the primary recovery mechanism;
- keep the power callback minimal and move recovery work onto the existing hook message-loop thread.

## Automated testing

CI cannot put a GitHub-hosted Windows runner through a real physical Sleep/Hibernate cycle, so automated coverage focuses on deterministic lifecycle logic.

Current focused coverage includes:

- mapping Windows suspend/resume notification constants to internal hook-thread messages;
- treating `ERROR_INVALID_HOOK_HANDLE` as an already-retired hook during safe re-arm;
- existing Raw Input removal descriptor behavior;
- existing Num Lock transition/replay behavior;
- existing runtime cleanup tests for movement and held-button release.

The standard Windows quality gate remains:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release --all-features
```

The implementation commit passed this complete gate on Windows before the documentation update.

## Manual release validation

Real Windows lifecycle validation remains required before release approval because CI does not reproduce firmware, USB keyboard, HID driver, session-lock, and hardware Num Lock LED behavior.

At minimum verify on real Windows hardware:

1. Enable NumFlow and start continuous NumPad movement.
2. Enter Sleep, resume, and confirm movement is no longer stale.
3. Confirm NumPad control is available immediately after Windows input becomes responsive.
4. Repeat while a drag/hold is latched and confirm no mouse button remains stuck after resume.
5. Repeat with NumFlow Off / Num Lock On and confirm ordinary number entry remains ordinary number entry.
6. Verify Num Lock LED and NumFlow mode stay synchronized.
7. Repeat from Hibernate where supported.
8. Repeat while NumFlow was started with `--background` / Start with Windows.
9. Confirm tray and HUD survive the lifecycle transition without opening the main window.
10. Repeat several suspend/resume cycles to catch duplicate-hook or stale-state regressions.

Manual results should be recorded in `docs/RELEASE_CHECKLIST.md` before the v0.1 release is approved.
