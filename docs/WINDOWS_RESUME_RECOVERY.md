# Windows suspend/resume recovery

This document describes how NumFlow restores its Windows input runtime after Sleep, Hibernate, session unlock, and return to the interactive desktop.

The implementation is Windows-specific and lives in `crates/numflow-windows`. It does not change NumPad bindings, UI behavior, tray/HUD lifetime, startup registration, or the `--background` launch mode.

## Problem

NumFlow uses a global `WH_KEYBOARD_LL` hook on a dedicated Win32 message-loop thread. Real Windows hardware testing exposed several independent lifecycle failure modes:

1. a low-level hook cannot be assumed to remain usable after Sleep/Hibernate, and Windows provides no reliable liveness query for an existing `HHOOK`;
2. `PBT_APMRESUMEAUTOMATIC` may arrive before the interactive desktop is fully usable;
3. callback-mode power notifications can be delivered out of the expected automatic/user order;
4. Slint/winit registers the process-wide Raw Input keyboard class while creating its event loop;
5. power resume alone is earlier than session unlock / desktop readiness on some systems;
6. `GetKeyState(VK_NUMLOCK)` is message-queue based, so reading it from NumFlow's background hook thread after resume can lag behind the foreground application;
7. the keyboard/session transition can emit Num Lock or NumPad semantics before the interactive desktop is fully stable. Treating those transient events as user intent can change `NUM_LOCK_ON` between the user power phase and `WTS_SESSION_UNLOCK`;
8. leaving winit's raw-keyboard registration active makes `WH_KEYBOARD_LL` delivery unreliable while NumFlow itself owns foreground focus, while moving that registration into hook recovery creates a self-triggering device-arrival loop that evicts queued NumPad events;
9. Task Manager commonly runs at elevated integrity. That does not make a low-integrity `SendInput` target controllable, and reinstalling `WH_KEYBOARD_LL` cannot bypass UIPI.

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

The input lifecycle is explicit:

```text
Running → Suspended/Locked → Recovering → Running
```

All lifecycle, focus, device, hook, and Num Lock checks enter the centralized
`resync_input_state(reason)` path. Focus changes are diagnostic and do not re-install a healthy
hook; lifecycle/hook recovery re-arms it on the owning message-loop thread, while ordinary device
arrival/removal comes from an independent device-interface subscription and clears stale runtime
input state.

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

### Foreground and keyboard-device notifications

The same message-only window receives `EVENT_SYSTEM_FOREGROUND` notifications through an
out-of-context WinEvent hook. Keyboard reconnect uses `RegisterDeviceNotificationW` filtered by
`GUID_DEVINTERFACE_KEYBOARD`; `WM_DEVICECHANGE` is translated into the existing hook-thread resync
message. The foreground callback only posts a message; process inspection and logging happen
outside callbacks. A normal device notification does **not** re-install a healthy
`WH_KEYBOARD_LL` hook: the hook is process-wide and is not invalidated merely because a keyboard
device arrived or was removed.

After Slint creates its event loop, NumFlow removes winit's keyboard-only Raw Input registration once.
It never registers that class on the hook thread. `WH_KEYBOARD_LL` remains NumFlow's sole NumPad
event path, while normal Slint keyboard input continues through window messages. This avoids both
the focused-NumFlow delivery failure and the repeated device-arrival recovery loop.

## Suspend path

When suspend is detected NumFlow:

1. moves the input lifecycle to `Suspended` and disables NumPad interception;
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
InputRuntime = Recovering
        ↓
interception disabled
        ↓
retire current WH_KEYBOARD_LL
        ↓
install fresh WH_KEYBOARD_LL
        ↓
verify the independent keyboard device-notification subscription
        ↓
clear stale runtime input work
        ↓
restore the last confirmed Num Lock / NumFlow mode
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

Resume recovery reads `VK_NUMLOCK` for diagnostics, but does not treat the hook thread's
`GetKeyState` bit as authoritative: that API is tied to the caller's message queue and can be stale
when another process is foreground. The authoritative runtime value is the last confirmed Windows
transition/NumPad semantic; recovery never overwrites it with a stale background-thread snapshot.

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

NumFlow therefore suppresses that transition as a mode authority while the lifecycle transaction is
frozen. If an external toggle implies that Windows may now differ from the tracked mode, NumFlow
records the mismatch and performs one tagged Num Lock replay when the lifecycle guard is finalized.

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

No settings-window focus change is required for recovery. A foreground transition is logged with the
target executable, PID, integrity, and elevation status. If the target is `Taskmgr.exe` and NumFlow is not
elevated, the hook may still observe the key but `SendInput` pointer injection can be rejected by
UIPI; the log reports this as an integrity limitation rather than attempting a hook-reinstall loop.

## Raw Input ownership

Raw keyboard events are not a second NumPad input path. Windows permits only one process-wide Raw
Input registration per device class. NumFlow removes winit's keyboard registration immediately
after `AppWindow` initialization and does not claim it for another window. Keyboard arrival/removal
diagnostics use the independent device-interface notification handle owned by the hook thread.
Focus changes therefore cannot move Raw Input ownership or change `INTERCEPTION_ENABLED`.

## Diagnostics

A typical locked Sleep/Resume cycle with NumFlow enabled (`Num Lock Off`) should keep `tracked=false` through all phases:

```text
NumFlow: session lifecycle window registered
NumFlow: session lock detected
NumFlow: suspend detected
NumFlow: resume automatic detected
NumFlow: hook restored (reason=resume-automatic, generation=...)
NumFlow: input state resynchronized (reason=resume-automatic, vk_numlock_snapshot=..., num_lock_on=false, numflow_enabled=true, hook_alive=true, raw_input_state=keyboard-disabled, keyboard_device_notifications=true)
NumFlow: NumLock resume state (tracked=false, source=last-confirmed-transition)
NumFlow: resume user detected
NumFlow: hook restored (reason=resume-user, generation=...)
NumFlow: input state resynchronized (reason=resume-user, vk_numlock_snapshot=..., num_lock_on=false, numflow_enabled=true, hook_alive=true, raw_input_state=keyboard-disabled, keyboard_device_notifications=true)
NumFlow: NumLock resume state (tracked=false, source=last-confirmed-transition)
NumFlow: resume session-unlock detected
NumFlow: hook restored (reason=session-unlock, generation=...)
NumFlow: input state resynchronized (reason=session-unlock, vk_numlock_snapshot=..., num_lock_on=false, numflow_enabled=true, hook_alive=true, raw_input_state=keyboard-disabled, keyboard_device_notifications=true)
NumFlow: NumLock resume state (tracked=false, source=last-confirmed-transition)
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

If desktop-ready is delivered, another `reason=desktop-ready` re-arm may follow.

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
- use `GetKeyState(VK_NUMLOCK)` after recovery as a diagnostic snapshot only; never overwrite the
  last confirmed mode from the background thread's message-queue bit;
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
- input runtime state/reason encoding;
- keyboard device-interface filter behavior;
- device notifications not rearming a healthy low-level hook;
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

1. Ordinary application → NumPad movement/click/hold/release, including `0`, `. / Del`, `+`, and custom bindings.
2. Task Manager → NumPad, recording `foreground changed -> Taskmgr.exe`, hook callback count, target integrity/elevation, and any UIPI `SendInput` failure.
3. Ordinary application → Task Manager → ordinary application again.
4. Lock → Unlock → NumPad without attempting input on the lock/secure desktop.
5. Sleep/Hibernate → Resume → NumPad.
6. Several physical Num Lock toggles, checking LED, runtime mode, tray, HUD, and settings state after each edge.
7. Disconnect/reconnect the keyboard and confirm `WM_DEVICECHANGE`, `raw_input_state=keyboard-disabled`, stable hook generation, and NumPad recovery.
8. Hold a NumPad movement key across Lock/Resume and confirm movement stops and pressed-key state is cleared.
9. Hold a NumFlow mouse button across Lock/Resume and confirm Windows receives the release.
10. Repeat with elevated and non-elevated applications. A normal NumFlow process must report the documented UIPI limitation for elevated targets; run NumFlow elevated only as an explicit deployment choice, never as a hook-reinstall workaround.
11. Test with the settings window closed/minimized and with `--background` startup.
12. Confirm tray and HUD survive each cycle without opening the main window.
13. Verify multi-monitor and DPI scenarios separately before release approval.
