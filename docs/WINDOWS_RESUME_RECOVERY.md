# Windows suspend/resume recovery

This document describes how NumFlow restores its Windows input runtime after Sleep, Hibernate, and Resume.

The implementation is Windows-specific and lives in `crates/numflow-windows`. It does not change NumPad bindings, UI behavior, tray/HUD lifetime, startup registration, or the `--background` launch mode.

## Problem

Before resume recovery was added, the global `WH_KEYBOARD_LL` hook ran on its own Win32 message-loop thread, but NumFlow did not subscribe that thread to Windows power lifecycle notifications.

That created three related failure modes after Sleep/Hibernate:

1. NumFlow had no event-driven signal telling the input runtime that Windows had resumed.
2. A low-level keyboard hook could no longer be assumed to be usable after the power transition, but there is no reliable Win32 API that answers whether a `WH_KEYBOARD_LL` hook is still operational.
3. The Raw Input keyboard registration removed after Slint/winit initialization was not reconciled again after resume.

The first recovery implementation fixed those gaps, but real hardware testing exposed two additional weaknesses:

- `PBT_APMRESUMEAUTOMATIC` can arrive before the user-visible resume transition is fully complete. Treating a hook successfully installed at that early phase as final was too optimistic.
- resume restored the cached `NUM_LOCK_ON` value instead of re-reading the Windows toggle state during the later user-resume phase, so Windows and NumFlow could remain logically desynchronized.

As a result, ordinary Windows keyboard input could already be responsive while NumFlow still failed to consume NumPad input or reported the wrong mode.

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
| `PBT_APMRESUMEAUTOMATIC` | perform provisional immediate recovery |
| `PBT_APMRESUMECRITICAL` | perform provisional immediate recovery |
| `PBT_APMRESUMESUSPEND` | always perform final late/user recovery and refresh Num Lock state |

No polling loop is introduced.

## Suspend path

When suspend is detected NumFlow:

1. disables NumPad interception;
2. clears the tracked Num Lock key-down edge state;
3. sends a fail-safe lifecycle event into the existing runtime path;
4. resets transient runtime input state through the normal Num Lock/state-machine handling;
5. records `suspend detected` in diagnostics.

The lifecycle event uses the same runtime path that already knows how to stop pointer motion and release a NumFlow-owned held mouse button.

## Resume recovery v2

Recovery now has two explicit phases.

### 1. Automatic/provisional phase

`PBT_APMRESUMEAUTOMATIC` and `PBT_APMRESUMECRITICAL` are used to recover as early as possible:

```text
automatic resume
        ↓
interception disabled
        ↓
retire previous WH_KEYBOARD_LL
        ↓
install provisional replacement hook
        ↓
reconcile Raw Input
        ↓
cleanup stale runtime input state
        ↓
restore cached Num Lock/NumFlow mode provisionally
```

This keeps recovery responsive, but the early hook is not treated as proof that recovery is finished.

### 2. Late/user phase

When `PBT_APMRESUMESUSPEND` arrives, NumFlow always performs another deterministic recovery pass even if the automatic phase succeeded:

```text
late/user resume
        ↓
interception disabled
        ↓
retire provisional/current WH_KEYBOARD_LL
        ↓
install fresh final WH_KEYBOARD_LL
        ↓
reconcile Raw Input again
        ↓
read Windows Num Lock toggle state
        ↓
compare cached vs Windows state
        ↓
cleanup stale runtime input state
        ↓
restore effective Num Lock/NumFlow mode
        ↓
restore interception only if recovery succeeded
```

This second re-arm is deliberate. A stored `HHOOK` value is not a liveness proof, and a hook installed during the early power transition can become unreliable before the interactive session is fully usable.

## Hook recovery

There is no reliable `WH_KEYBOARD_LL` liveness query. NumFlow therefore uses deterministic re-arming instead of pretending that a stored `HHOOK` value proves that the hook still works.

Recovery first calls `UnhookWindowsHookEx` for the previous handle. `ERROR_INVALID_HOOK_HANDLE` is treated as "already retired". A replacement hook is installed only after the old hook is confirmed retired.

This ordering is a safety invariant: NumFlow must never intentionally leave two active global keyboard hooks installed at the same time.

Every successful hook installation increments an internal generation counter used only for diagnostics. This makes repeated resume behavior observable without adding polling or timing delays.

If re-arming fails, interception remains disabled rather than running in an uncertain partial state.

## Raw Input reconciliation

NumFlow removes winit's process-wide raw-keyboard device-event registration because that registration can interfere with `WH_KEYBOARD_LL` delivery while a NumFlow window owns foreground focus.

The removal is intentionally idempotent and is repeated after both automatic and late/user resume recovery. Raw mouse registration is not modified.

## Num Lock and NumFlow state

The mode relation remains:

- Num Lock On → NumFlow pointer interception Off;
- Num Lock Off → NumFlow pointer interception On.

During the automatic/provisional phase, the cached Num Lock state is used only as an immediate recovery value.

During `PBT_APMRESUMESUSPEND`, NumFlow re-reads the Windows Num Lock toggle state with `GetKeyState(VK_NUMLOCK)`, compares it with the cached state, updates `NUM_LOCK_ON`, and treats the Windows value as the effective late-resume mode.

The physical NumPad semantic reconciliation remains as an additional repair path: digit/navigation scan-code interpretation can still correct the tracked mode on the next physical NumPad event if Windows reports a different interpretation.

Resume recovery dispatches a fail-safe cleanup state and then the effective Num Lock state. This reuses the normal runtime/state-machine path instead of introducing a second resume-only state machine.

The cleanup/restore sequence resets the keyboard normalizer, stops movement, releases NumFlow-owned held-button state, and then restores the correct NumFlow On/Off mode.

Interception is only re-enabled when both the replacement hook is installed and the lifecycle state events were delivered successfully.

## Diagnostics

Recovery v2 emits phase-aware diagnostics such as:

```text
NumFlow: suspend detected
NumFlow: resume automatic detected
NumFlow: hook restored (phase=automatic, generation=2)
NumFlow: NumLock resynced (phase=automatic, num_lock_on=..., numflow_enabled=..., interception=...)
NumFlow: resume user detected
NumFlow: hook restored (phase=user, generation=3)
NumFlow: NumLock resume state (cached=..., windows=..., effective=...)
NumFlow: NumLock resynced (phase=user, num_lock_on=..., numflow_enabled=..., interception=...)
```

Hook-recovery and Raw Input reconciliation failures are also logged. These diagnostics are intended for troubleshooting real hardware Sleep/Hibernate issues without adding timing delays to the input path.

## Safety invariants

Changes to this subsystem must preserve the following rules:

- never create a replacement hook before the previous hook is retired;
- always perform a fresh final re-arm on `PBT_APMRESUMESUSPEND`;
- never enable interception while hook recovery is incomplete;
- never keep stale pointer movement active across suspend/resume;
- never keep a NumFlow-owned mouse hold latched across lifecycle cleanup;
- refresh Windows Num Lock state during the late/user phase instead of trusting only pre-suspend cache;
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
- proving that the user-resume phase is the phase that refreshes Windows Num Lock state;
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

## Manual release validation

Real Windows lifecycle validation remains required before release approval because CI does not reproduce firmware, USB keyboard, HID driver, session-lock, and hardware Num Lock LED behavior.

At minimum verify on real Windows hardware:

1. Enable NumFlow and start continuous NumPad movement.
2. Enter Sleep, resume, and confirm movement is no longer stale.
3. Confirm NumPad control is available immediately after Windows input becomes responsive.
4. Repeat while a drag/hold is latched and confirm no mouse button remains stuck after resume.
5. Repeat with NumFlow Off / Num Lock On and confirm ordinary number entry remains ordinary number entry.
6. Verify Num Lock LED and NumFlow mode stay synchronized.
7. Inspect logs and confirm both `resume automatic` and `resume user` recovery phases occur when Windows emits them.
8. Confirm hook generation increases on the final user-phase re-arm.
9. Repeat from Hibernate where supported.
10. Repeat while NumFlow was started with `--background` / Start with Windows.
11. Confirm tray and HUD survive the lifecycle transition without opening the main window.
12. Repeat several suspend/resume cycles to catch duplicate-hook or stale-state regressions.

Manual results should be recorded in `docs/RELEASE_CHECKLIST.md` before the v0.1 release is approved.
