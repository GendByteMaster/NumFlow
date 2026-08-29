# Windows suspend/resume recovery

This document defines NumFlow's Windows input-lifecycle contract for Sleep, Hibernate, session
lock/unlock, protected-desktop switches, foreground changes, and keyboard reconnects.

The implementation is contained in `crates/numflow-windows` plus the runtime-side recovery ACK in
`src/runtime.rs`. It does not change NumPad mappings, mouse semantics, the UI, tray, startup, or the
Num Lock mode rule.

## Root cause fixed

The old implementation did not have one lifecycle authority. It combined three atomic state flags,
two resume-stage counters, and several event-specific handlers.

Two non-power paths wrote the power state directly:

- `EVENT_SYSTEM_DESKTOPSWITCH` immediately stored `INPUT_RUNTIME_STATE=SUSPENDED`;
- `WTS_SESSION_LOCK` did the same and then queued `WM_NUMFLOW_DESKTOP_INACTIVE`.

The hook-owner message loop handled `WM_NUMFLOW_DESKTOP_INACTIVE` and the authoritative
`PBT_APMSUSPEND` message through the same `handle_suspend_notification()` branch. Consequently one
physical Sleep could produce a power suspend, a session-lock suspend, and one or more desktop-switch
suspends. A desktop switch delivered after successful resume could set the runtime back to
`Suspended`, set the resume Num Lock guard again, and leave later physical transitions suppressed.

Each of `PBT_APMRESUMEAUTOMATIC`, `PBT_APMRESUMESUSPEND`, `WTS_SESSION_UNLOCK`, and desktop-ready
also ran a complete hook replacement independently. Stage counters reduced some reorderings but did
not represent one recovery transaction and did not protect private messages with a lifecycle token.

## Lifecycle authority

`lifecycle.rs` owns the deterministic state machine:

```text
Running -> Suspending -> Suspended -> Resuming -> Running
```

Session lock and input-desktop availability are orthogonal facts. They can quiesce input and delay
recovery, but they do not create a power `Suspended` transition.

All Win32 lifecycle notifications are serialized into the hook-owner thread. That thread is the only
place that calls `LifecycleMachine::handle_event(...)`, publishes `InputRuntimeState`, retires or
installs the hook, and begins recovery.

Published states are:

- `Running`;
- `Suspending`;
- `Suspended`;
- `Resuming`;
- `SessionLocked`;
- `Recovering` for an inactive non-session desktop.

The previous independently writable `RESUME_NUM_LOCK_GUARD`, `SESSION_LOCK_PENDING`, power-stage,
and session-stage state is gone. Num Lock is frozen whenever the published lifecycle is not
`Running`; there is no separate guard that can remain true after the state machine completes.

## Authoritative and non-authoritative events

### Power

NumFlow registers `RegisterSuspendResumeNotification` in callback mode. It does not currently
register a separate `WM_POWERBROADCAST` window handler. The callback maps these Windows power events
to private hook-thread messages:

| Event | Meaning |
| --- | --- |
| `PBT_APMSUSPEND` | the only authoritative transition into power suspend |
| `PBT_APMRESUMEAUTOMATIC` | resume signal; recover now if the interactive desktop is available |
| `PBT_APMRESUMECRITICAL` | handled as automatic resume |
| `PBT_APMRESUMESUSPEND` | user-visible resume signal for the same transaction |

If `WM_POWERBROADCAST` support is added later, it must feed the same state machine and must not add a
second suspend pipeline.

### Session and desktop

The hook thread owns an `HWND_MESSAGE` window registered with
`WTSRegisterSessionNotification(NOTIFY_FOR_THIS_SESSION)`.

- `WTS_SESSION_LOCK` quiesces input and records the locked-session fact.
- `WTS_SESSION_UNLOCK` clears that fact and continues the current transaction when the input desktop
  is ready.
- WTS desktop-ready reason `0x0F` marks the desktop available and continues recovery.
- `EVENT_SYSTEM_DESKTOPSWITCH` is classified on the hook-owner thread by comparing its desktop with
  `OpenInputDesktop`; it becomes either `DesktopInactive` or `DesktopReady`.

None of these events is treated as `PBT_APMSUSPEND`.

### Foreground and device changes

`EVENT_SYSTEM_FOREGROUND` and keyboard `WM_DEVICECHANGE` are health checkpoints only while the
lifecycle is `Running`. They cannot suspend the runtime, clear or finalize recovery, or reinstall a
healthy hook.

Keyboard reconnect is observed through `RegisterDeviceNotificationW` with
`GUID_DEVINTERFACE_KEYBOARD`. The notification subscription is independent from Raw Input.

## Duplicate and stale events

Every external lifecycle callback obtains a monotonically increasing token while holding one short
ordering mutex, then posts the token with its private message. The hook-owner machine rejects a token
that is not newer than the last accepted token.

Accepted transitions also have a lifecycle generation:

```text
power suspend        generation=5
power resume         generation=6
runtime recovery ACK generation=6
late suspend token from the older delivery -> ignored
```

Idempotence rules include:

- `Suspend -> Suspend` is ignored as `duplicate-suspend`;
- automatic/user/unlock/desktop-ready signals share the current `Resuming` transaction;
- a second signal cannot start recovery while `recovery_started=true`;
- a completion ACK for a different generation is ignored;
- desktop/session quiesce is not repeated when input is already quiesced;
- foreground and device events received outside `Running` are ignored.

There are no sleeps, polling retries, or periodic hook replacements in this ordering mechanism.

## Suspend and quiesce

An accepted authoritative suspend performs one pipeline:

1. publish `Suspending`;
2. disable interception and clear the Num Lock key-down edge state;
3. queue runtime cleanup through the bounded lifecycle queue;
4. stop movement, clear pressed/repeat state, and release NumFlow-owned mouse holds in the runtime;
5. retire the hook on its owner thread;
6. publish `Suspended`.

Session lock and desktop loss use the same safe quiesce action but retain their non-power lifecycle
meaning.

## One resume transaction

An accepted resume moves the machine to `Resuming`. If the session is locked or its desktop is not
available, it stays there until unlock/desktop-ready. Otherwise it starts exactly one transaction:

1. keep interception disabled;
2. clear Num Lock key-down transient state;
3. install `WH_KEYBOARD_LL` only when the owner has no hook handle;
4. retain the independent keyboard device-notification registration;
5. read `GetKeyState(VK_NUMLOCK)` for diagnostics only;
6. enqueue one `LifecycleRecovery { generation, ... }` event;
7. on the runtime worker, atomically clear the normalizer, stop movement, release holds, apply the
   cleanup mode, restore the last confirmed Num Lock mode, and optionally replay one tagged Windows
   Num Lock toggle;
8. post `WM_NUMFLOW_RECOVERY_COMPLETE` with the same generation;
9. only after that ACK publish `Running` and allow interception.

If hook installation, runtime application, pointer release, Num Lock replay, or the ACK fails, the
machine does not claim `Running`.

## Hook ownership

The dedicated hook message-loop thread remains the single hook owner. Suspend, lock, and desktop
loss retire its handle first. Recovery installs only when `hook.is_none()`; a present handle is never
followed by a second `SetWindowsHookExW` call. Duplicate resume events therefore cannot create
parallel hooks or increment hook generation repeatedly.

`ERROR_INVALID_HOOK_HANDLE` during retirement is treated as already retired.

## Num Lock contract

The mode rule remains unchanged:

- Num Lock On -> NumFlow pointer interception Off;
- Num Lock Off -> NumFlow pointer interception On.

The last confirmed transition is the recovery authority. `GetKeyState` on the background hook thread
is logged but cannot overwrite it because that API reflects the caller's message-queue state.

While lifecycle state is not `Running`, physical or external Num Lock transitions cannot mutate the
tracked mode. A mismatch is recorded and repaired once through NumFlow's tagged replay during the
generation-matched runtime recovery. Successful recovery necessarily publishes `Running`, so the
freeze condition cannot survive as an independent stale boolean.

## Raw Input

Raw keyboard input is intentionally not a NumPad path. NumFlow removes Slint/winit's process-wide
keyboard Raw Input registration once and logs `raw_input_state=keyboard-disabled-hook-owned`.

- global NumPad input: `WH_KEYBOARD_LL`;
- normal Slint keyboard input: ordinary window messages;
- keyboard arrival/removal: `RegisterDeviceNotificationW` / `WM_DEVICECHANGE`.

Recovery does not recreate Raw Input. A snapshot with
`raw_input_state=keyboard-disabled-hook-owned` and `keyboard_device_notifications=true` is expected.

## Diagnostics

State changes include event, source, generation, and delivery token:

```text
NumFlow: Lifecycle: Running -> Suspending event=PBT_APMSUSPEND source=RegisterSuspendResumeNotification reason=authoritative-power-event generation=7 token=41
NumFlow: Lifecycle: Suspending -> Suspended event=PBT_APMSUSPEND source=RegisterSuspendResumeNotification generation=7
NumFlow: Lifecycle: Suspended -> Resuming event=PBT_APMRESUMEAUTOMATIC source=RegisterSuspendResumeNotification reason=recovery-start generation=8 token=42
NumFlow: Lifecycle: Resuming -> Running reason=recovery-complete generation=8 hook_alive=true input_frozen=false session_locked=false mouse_hold=false
```

Ignored events are explicit:

```text
NumFlow: Lifecycle event ignored event=WTS_SESSION_UNLOCK source=WM_WTSSESSION_CHANGE state=Running reason=already-unlocked generation=8 token=45
```

Input snapshots include `lifecycle_state`, `lifecycle_generation`, hook generation/liveness, callback
counters, Num Lock mode, interception, mouse hold, Raw Input ownership, and device-notification
registration.

## Automated regression coverage

Deterministic tests cover:

- `Running -> Suspend -> Resume -> Running`;
- duplicate suspend;
- automatic resume -> user resume -> session unlock with one recovery transaction;
- stale suspend token after a newer generation;
- desktop switch after resume without a power suspend regression;
- lock -> unlock;
- Sleep -> Resume and Sleep -> Resume -> Unlock;
- duplicate recovery/hook prevention decisions;
- Num Lock mode preservation and tagged replay decisions;
- pressed/repeat reset, movement stop, and held-button release during lifecycle cleanup;
- keyboard device notification and Raw Input ownership contracts.

Run the repository quality gate:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
```

## Required physical Windows validation

Automated tests cannot suspend and unlock a physical interactive Windows session. Before release,
record logs and results for:

1. five consecutive `Sleep -> Wake -> Unlock -> immediate NumPad` cycles;
2. `Lock -> Unlock -> NumPad`;
3. `Task Manager -> NumPad`;
4. `Task Manager -> Sleep -> Wake -> Unlock -> NumPad`;
5. `Ctrl+Alt+Del -> Cancel -> NumPad`;
6. physical keyboard disconnect/reconnect;
7. several consecutive Sleep/Resume cycles;
8. movement held during Sleep/Lock;
9. NumFlow mouse hold active during Sleep/Lock.

Success requires no Num Lock toggle, window activation, process restart, or multi-second wait. The
final log for each cycle must show `Resuming -> Running`, `hook_alive=true`, `input_frozen=false`,
and `mouse_hold=false` before the first NumPad action is processed.
