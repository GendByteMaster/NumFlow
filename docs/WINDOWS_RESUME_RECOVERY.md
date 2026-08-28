# Windows suspend/resume recovery

This document describes how NumFlow restores its Windows input runtime after Sleep, Hibernate, session unlock, and return to the interactive desktop.

The implementation is Windows-specific and lives in `crates/numflow-windows`. It does not change NumPad bindings, UI behavior, tray/HUD lifetime, startup registration, or the `--background` launch mode.

## Problem

NumFlow uses a global `WH_KEYBOARD_LL` hook on a dedicated Win32 message-loop thread. Real Windows hardware testing exposed several lifecycle failure modes:

1. a low-level hook cannot be assumed to remain usable after Sleep/Hibernate, and Windows provides no reliable liveness query for an existing `HHOOK`;
2. `PBT_APMRESUMEAUTOMATIC` may arrive before the interactive desktop is fully usable;
3. callback-mode power notifications can be delivered out of the expected automatic/user order;
4. Slint/winit Raw Input keyboard registration must be reconciled again after resume;
5. power resume alone is earlier than session unlock / desktop readiness on some systems;
6. `GetKeyState(VK_NUMLOCK)` is message-queue based. Reading it from NumFlow's background hook thread after resume can lag behind the foreground application and overwrite a correct tracked NumFlow mode with a stale value.

The last case was reproduced on real hardware: the tracked state was already `Num Lock Off` / NumFlow enabled, while the background-thread `GetKeyState` snapshot still reported `Num Lock On`. Recovery then incorrectly produced `numflow_enabled=false, interception=false`, especially when another application owned foreground focus.

## Design goals

Lifecycle recovery must remain:

- event-driven;
- independent of arbitrary multi-second sleeps;
- safe against duplicate hooks;
- safe against out-of-order power callbacks;
- independent of which application owns foreground focus;
- safe for active movement and drag/hold state;
- compatible with tray/HUD/background operation;
- compatible with `Start with Windows` and `--background`;
- independent from settings-window visibility.

The settings window is never activated or opened as part of recovery.

## Lifecycle sources

NumFlow uses two complementary Win32 lifecycle sources on the existing keyboard-hook thread.

### Power notifications

`RegisterSuspendResumeNotification` runs in callback mode. The callback does minimal work and posts private `WM_APP` messages to the hook thread.

Handled events:

| Windows event | NumFlow action |
| --- | --- |
| `PBT_APMSUSPEND` | enter fail-safe suspend state and reset lifecycle ordering |
| `PBT_APMRESUMEAUTOMATIC` | provisional early re-arm |
| `PBT_APMRESUMECRITICAL` | provisional early re-arm |
| `PBT_APMRESUMESUSPEND` | late/user-visible power re-arm |

Power stages are monotonic:

```text
idle → automatic → user
```

Once user recovery has been queued, a delayed automatic/critical callback from the same cycle is ignored instead of regressing the final recovery state.

### Session notifications

The hook thread also owns a hidden Win32 **message-only window** (`HWND_MESSAGE`) registered with `WTSRegisterSessionNotification` for the current session.

`WM_WTSSESSION_CHANGE` is translated into private hook-thread lifecycle messages. NumFlow handles:

- `WTS_SESSION_LOCK` — reset the session-recovery cycle;
- `WTS_SESSION_UNLOCK` — perform a fresh session-level re-arm;
- desktop-ready reason `0x0F` when delivered — perform another final desktop-level re-arm.

This keeps the runtime global and event-driven without introducing Tokio, polling, or a visible helper window.

If WTS registration is unavailable, NumFlow logs the failure and continues with the power-notification recovery path instead of failing application startup.

## Suspend path

When suspend is detected NumFlow:

1. disables NumPad interception;
2. clears the tracked Num Lock key-down edge state;
3. sends a fail-safe lifecycle event through the existing runtime path;
4. resets transient input state;
5. stops pointer motion;
6. releases a NumFlow-owned held mouse button;
7. records `suspend detected` in diagnostics.

The authoritative tracked `NUM_LOCK_ON` toggle value itself is preserved across the suspend transition.

## Resume recovery

Every recovery phase uses the same deterministic sequence:

```text
resume/session signal
        ↓
interception disabled
        ↓
retire current WH_KEYBOARD_LL
        ↓
install fresh WH_KEYBOARD_LL
        ↓
reconcile process Raw Input keyboard registration
        ↓
clear stale runtime input work
        ↓
restore tracked Num Lock / NumFlow mode
        ↓
restore interception only after successful recovery
```

The phases are:

```text
automatic
user
session-unlock
[desktop-ready, when delivered]
```

A valid stored `HHOOK` is not treated as proof that the hook still works. Each phase retires the current handle before installing its replacement. `ERROR_INVALID_HOOK_HANDLE` is treated as already retired.

This ordering intentionally prevents NumFlow from creating two live global keyboard hooks.

## Foreground-independent Num Lock / NumFlow state

The mode relation remains:

- Num Lock On → NumFlow pointer interception Off;
- Num Lock Off → NumFlow pointer interception On.

Resume recovery **does not use `GetKeyState(VK_NUMLOCK)` as its mode authority**.

`GetKeyState` reflects the calling thread's keyboard/message-queue state. NumFlow's hook thread is a background worker, so after Sleep/Resume its toggle bit can lag behind the application that currently owns foreground focus.

Instead, recovery restores the mode already tracked from real Num Lock transitions:

```text
NUM_LOCK_ON
    ↓
tracked resume authority
    ↓
NumLockChanged cleanup + restore
    ↓
INTERCEPTION_ENABLED = !NUM_LOCK_ON
```

This means returning from sleep while Warp, a browser, an editor, a game, or another application owns focus does not itself change NumFlow's mode.

### Physical reconciliation remains authoritative for new input

NumFlow already inspects the physical NumPad event semantics reported by Windows:

- digit VK + NumPad scan code → Num Lock On;
- navigation VK + the same physical NumPad scan code → Num Lock Off.

Therefore the first physical NumPad event after resume can repair the tracked mode if Windows genuinely changed the toggle outside NumFlow's observable desktop/session.

A physical or injected Num Lock transition observed by the global hook also updates the tracked state normally.

This gives NumFlow a focus-independent resume policy without forcing the Windows LED/toggle to an assumed value.

## Raw Input reconciliation

NumFlow removes winit's process-wide raw-keyboard device-event registration because that registration can interfere with the low-level hook in the NumFlow process.

Removal is idempotent and repeated during each recovery phase. Raw mouse registration is left untouched.

## Diagnostics

A typical recovery with NumFlow enabled (`Num Lock Off`) should include lines similar to:

```text
NumFlow: session lifecycle window registered
NumFlow: suspend detected
NumFlow: resume user detected
NumFlow: hook restored (phase=user, generation=...)
NumFlow: NumLock resume state (tracked=false, source=physical-history)
NumFlow: NumLock resynced (phase=user, num_lock_on=false, numflow_enabled=true, interception=true)
NumFlow: resume session-unlock detected
NumFlow: hook restored (phase=session-unlock, generation=...)
NumFlow: NumLock resume state (tracked=false, source=physical-history)
NumFlow: NumLock resynced (phase=session-unlock, num_lock_on=false, numflow_enabled=true, interception=true)
```

If desktop-ready is delivered, another `phase=desktop-ready` re-arm may follow.

An out-of-order automatic callback after interactive recovery is suppressed:

```text
NumFlow: stale automatic resume callback ignored after user resume
```

The important regression signal is that resume diagnostics must no longer contain a foreground-derived `cached=..., windows=..., effective=...` decision that disables NumFlow because the hook thread observed a stale toggle bit.

## Safety invariants

Changes to this subsystem must preserve the following rules:

- never create a replacement hook before the previous hook is retired;
- never enable interception while hook recovery is incomplete;
- never allow delayed automatic power callbacks to regress interactive recovery;
- use WTS/session signals as additional recovery points, not as an application-startup dependency;
- never overwrite tracked NumFlow mode from background-thread `GetKeyState` during resume;
- allow real physical NumPad semantics to reconcile external Num Lock changes;
- never keep stale pointer movement active across suspend/resume;
- never keep a NumFlow-owned mouse hold latched across lifecycle cleanup;
- do not rebuild or close tray/HUD/background runtime during recovery;
- do not activate the main settings window during recovery;
- do not change NumPad bindings as part of lifecycle recovery;
- do not use arbitrary multi-second sleeps as the primary recovery mechanism;
- do not add an async runtime only to coordinate Win32 lifecycle messages.

## Automated testing

A GitHub-hosted Windows runner cannot reproduce a physical Sleep/Hibernate cycle, so automated coverage focuses on deterministic lifecycle logic and build correctness.

Current coverage includes:

- Windows power-event mapping;
- monotonic automatic/user callback ordering;
- WTS session unlock / desktop-ready ordering and coalescing;
- session-lock recovery-cycle reset;
- safe already-retired-hook handling;
- regression guard preventing a `GetKeyState` call inside the resume handler;
- Raw Input removal descriptor behavior;
- physical NumPad semantic inference for Num Lock On/Off;
- Num Lock transition/replay behavior;
- runtime cleanup behavior for movement and held-button release.

The Windows quality gate is:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release --all-features
```

## Manual release validation

Real Windows lifecycle validation remains required before release approval.

At minimum verify:

1. Put NumFlow in pointer mode (`Num Lock Off`).
2. Focus **another application**; do not leave the NumFlow settings window foreground.
3. Enter Sleep, then resume.
4. Without clicking or focusing NumFlow, immediately test NumPad pointer movement in the other application.
5. Confirm diagnostics keep `tracked=false` and produce `numflow_enabled=true, interception=true` after user/session recovery.
6. Repeat with NumFlow Off / Num Lock On and confirm ordinary number entry remains ordinary number entry.
7. Repeat while a NumFlow drag/hold is active and confirm no mouse button remains stuck.
8. Repeat multiple Sleep/Resume and Hibernate/Resume cycles.
9. Test with the settings window closed/minimized and with `--background` startup.
10. Confirm tray and HUD survive each cycle without opening the main window.
11. Verify physical Num Lock toggling after resume still switches NumFlow immediately and keeps normal numeric mode usable.
12. Verify multi-monitor and DPI scenarios separately before release approval.
