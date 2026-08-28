# Windows suspend/resume recovery

This document describes how NumFlow restores its Windows input runtime after Sleep, Hibernate, session unlock, and return to the interactive desktop.

The implementation is Windows-specific and lives in `crates/numflow-windows`. It does not change NumPad bindings, UI behavior, tray/HUD lifetime, startup registration, or the `--background` launch mode.

## Problem

NumFlow uses a global `WH_KEYBOARD_LL` hook on a dedicated Win32 message-loop thread. Real Windows hardware testing exposed several independent lifecycle failure modes:

1. a low-level hook cannot be assumed to remain usable after Sleep/Hibernate, and Windows provides no reliable liveness query for an existing `HHOOK`;
2. `PBT_APMRESUMEAUTOMATIC` may arrive before the interactive desktop is fully usable;
3. callback-mode power notifications can be delivered out of the expected automatic/user order;
4. Slint/winit Raw Input keyboard registration must be reconciled again after resume;
5. power resume alone is earlier than session unlock / desktop readiness on some systems;
6. `GetKeyState(VK_NUMLOCK)` is message-queue based, so reading it from NumFlow's background hook thread after resume can lag behind the foreground application;
7. the keyboard/session transition can emit Num Lock or NumPad semantics before the interactive desktop is fully stable. Treating those transient events as user intent can change `NUM_LOCK_ON` between the user power phase and `WTS_SESSION_UNLOCK`.

The last two cases were reproduced on real hardware. In one failure, the tracked state was already `Num Lock Off` / NumFlow enabled while a background-thread Windows snapshot reported `Num Lock On`. In a later failure, user recovery correctly restored `tracked=false`, but a Num Lock transition arriving before session unlock changed the tracked state to `true`; session-unlock then faithfully restored the wrong mode and produced `numflow_enabled=false, interception=false`.

## Design goals

Lifecycle recovery must remain:

- event-driven;
- independent of arbitrary multi-second sleeps;
- safe against duplicate hooks;
- safe against out-of-order power callbacks;
- independent of which application owns foreground focus;
- safe against transient Num Lock / NumPad semantics during lock-screen-to-desktop transition;
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
| `PBT_APMSUSPEND` | enter fail-safe suspend state and begin the frozen-mode recovery transaction |
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

- `WTS_SESSION_LOCK` — mark the session as locked, reset the session-recovery cycle, and freeze the currently tracked NumFlow mode;
- `WTS_SESSION_UNLOCK` — perform a fresh session-level hook re-arm and finalize the frozen-mode transaction;
- desktop-ready reason `0x0F` when delivered — perform another final desktop-level re-arm.

This keeps the runtime global and event-driven without introducing Tokio, polling, or a visible helper window.

If WTS registration is unavailable, NumFlow logs the failure and continues with the power-notification recovery path instead of failing application startup.

## Suspend path

When suspend is detected NumFlow:

1. disables NumPad interception;
2. clears the tracked Num Lock key-down edge state;
3. enables the resume Num Lock lifecycle guard;
4. clears any pending Windows-toggle mismatch from an older recovery cycle;
5. sends a fail-safe lifecycle event through the existing runtime path;
6. resets transient input state;
7. stops pointer motion;
8. releases a NumFlow-owned held mouse button;
9. records `suspend detected` in diagnostics.

The authoritative tracked `NUM_LOCK_ON` toggle value itself is preserved across the suspend transition.

## Resume recovery

Every recovery phase uses the same deterministic hook sequence:

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
restore frozen/tracked Num Lock / NumFlow mode
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

## Lifecycle-frozen Num Lock / NumFlow mode

The mode relation remains:

- Num Lock On → NumFlow pointer interception Off;
- Num Lock Off → NumFlow pointer interception On.

Resume recovery **does not use `GetKeyState(VK_NUMLOCK)` as its mode authority**. It also no longer allows the first keyboard semantic observed during recovery to become the authority by itself.

The tracked `NUM_LOCK_ON` value at suspend/session-lock time is treated as the desired mode for the current recovery transaction:

```text
tracked NUM_LOCK_ON before/at suspend
        ↓
RESUME_NUM_LOCK_GUARD = true
        ↓
automatic power recovery
        ↓
user power recovery
        ↓
[transient Num Lock / NumPad events cannot replace tracked mode]
        ↓
WTS_SESSION_UNLOCK / desktop-ready
        ↓
restore tracked mode + optional tagged Windows resync
        ↓
clear lifecycle guard
```

If a `WTS_SESSION_LOCK` was observed, the guard remains active through the user power phase and is finalized only at `session-unlock` or `desktop-ready`.

If no session lock was observed, the user power phase is the fallback finalization point so the guard cannot remain active indefinitely on systems where WTS lifecycle events are unavailable.

### Num Lock transitions while recovery is frozen

While the lifecycle guard is active, a Num Lock transition is **not** allowed to mutate `NUM_LOCK_ON` or `INTERCEPTION_ENABLED`.

This is intentional. During lock-screen-to-desktop transition Windows, firmware, a keyboard driver, or another input source can produce a Num Lock transition before the interactive session is stable. Treating that event as definitive user intent caused the real-hardware `tracked=false → tracked=true` regression.

NumFlow therefore suppresses that transition as a mode authority while the lifecycle transaction is frozen. If an external injected toggle implies that Windows may now differ from the tracked mode, NumFlow records the mismatch and performs one tagged Num Lock replay when the lifecycle guard is finalized.

NumFlow's own tagged replay is still filtered from ordinary hook state tracking, so the repair cannot recursively toggle NumFlow's state.

### NumPad semantics while recovery is frozen

NumFlow inspects physical NumPad semantics reported by Windows:

- digit VK + NumPad scan code → observed Num Lock On semantics;
- navigation VK + the same physical NumPad scan code → observed Num Lock Off semantics.

Outside lifecycle recovery this remains a strong reconciliation signal and can repair stale startup/runtime state.

During lifecycle recovery the policy is different:

- observed semantics matching the tracked mode → preserve the tracked mode and keep the guard active;
- observed semantics conflicting with the tracked mode → preserve the tracked mode, keep interception aligned with it, record a Windows mismatch, and defer Windows resync until lifecycle finalization;
- the NumPad event can still be handled immediately as NumFlow input when pointer mode is frozen On.

Input therefore cannot prematurely clear the lifecycle guard.

After the guard is finalized, ordinary physical/injected Num Lock transitions and ordinary NumPad semantic reconciliation work normally again.

## Foreground independence

`GetKeyState` reflects the calling thread's keyboard/message-queue state. NumFlow's hook thread is a background worker, so after Sleep/Resume its toggle bit can lag behind the application that currently owns foreground focus.

NumFlow therefore restores its frozen tracked mode regardless of whether NumFlow, Warp, a browser, an editor, a game, or another application owns foreground focus.

No settings-window focus change is required for recovery.

## Raw Input reconciliation

NumFlow removes winit's process-wide raw-keyboard device-event registration because that registration can interfere with the low-level hook in the NumFlow process.

Removal is idempotent and repeated during each recovery phase. Raw mouse registration is left untouched.

## Diagnostics

A typical locked Sleep/Resume cycle with NumFlow enabled (`Num Lock Off`) should keep `tracked=false` through all phases:

```text
NumFlow: session lifecycle window registered
NumFlow: session lock detected
NumFlow: suspend detected
NumFlow: resume automatic detected
NumFlow: hook restored (phase=automatic, generation=...)
NumFlow: NumLock resume state (tracked=false, source=physical-history)
NumFlow: NumLock resynced (phase=automatic, num_lock_on=false, numflow_enabled=true, interception=true)
NumFlow: resume user detected
NumFlow: hook restored (phase=user, generation=...)
NumFlow: NumLock resume state (tracked=false, source=physical-history)
NumFlow: NumLock resynced (phase=user, num_lock_on=false, numflow_enabled=true, interception=true)
NumFlow: resume session-unlock detected
NumFlow: hook restored (phase=session-unlock, generation=...)
NumFlow: NumLock resume state (tracked=false, source=physical-history)
NumFlow: NumLock resynced (phase=session-unlock, num_lock_on=false, numflow_enabled=true, interception=true)
NumFlow: resume NumLock lifecycle guard cleared (phase=session-unlock, tracked=false)
```

If Windows emits transient input during the frozen period, diagnostics may additionally include:

```text
NumFlow: NumLock transition suppressed while resume lifecycle is frozen
```

or:

```text
NumFlow: resume NumLock semantic mismatch (tracked=false, observed=true); preserving tracked mode and deferring Windows resync until lifecycle finalization
```

If desktop-ready is delivered, another `phase=desktop-ready` re-arm may follow.

An out-of-order automatic callback after interactive recovery is suppressed:

```text
NumFlow: stale automatic resume callback ignored after user resume
```

The critical regression signal is that a locked resume must not change `tracked=false` into `tracked=true` before session-unlock merely because Windows emitted transient keyboard semantics.

## Safety invariants

Changes to this subsystem must preserve the following rules:

- never create a replacement hook before the previous hook is retired;
- never enable interception while hook recovery is incomplete;
- never allow delayed automatic power callbacks to regress interactive recovery;
- use WTS/session signals as additional recovery points, not as an application-startup dependency;
- never overwrite tracked NumFlow mode from background-thread `GetKeyState` during resume;
- when a session lock participates in the recovery cycle, do not let input clear the lifecycle mode guard before session-unlock/desktop-ready;
- do not let transient Num Lock or NumPad semantics mutate the frozen mode during recovery;
- outside recovery, keep ordinary Num Lock transitions and NumPad semantic reconciliation functional;
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
- lifecycle guard finalization policy for locked vs. power-only resumes;
- matching NumPad semantics preserving the guard during recovery;
- mismatching NumPad semantics preserving tracked mode and requesting deferred Windows repair;
- ordinary NumPad semantic reconciliation outside recovery;
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
5. Confirm diagnostics keep `tracked=false` through automatic, user, and session-unlock recovery and produce `numflow_enabled=true, interception=true` at each restored phase.
6. Confirm the lifecycle guard is cleared only after the appropriate final phase (`session-unlock`/`desktop-ready` for a locked cycle; user phase for power-only fallback).
7. Repeat with NumFlow Off / Num Lock On and confirm ordinary number entry remains ordinary number entry after recovery completes.
8. Immediately after recovery completes, toggle physical Num Lock and confirm normal mode switching still works.
9. Repeat while a NumFlow drag/hold is active and confirm no mouse button remains stuck.
10. Repeat multiple Sleep/Resume and Hibernate/Resume cycles.
11. Test with the settings window closed/minimized and with `--background` startup.
12. Confirm tray and HUD survive each cycle without opening the main window.
13. Verify multi-monitor and DPI scenarios separately before release approval.
