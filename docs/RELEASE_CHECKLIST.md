# NumFlow v0.1 release checklist

This checklist tracks release-readiness evidence that is not safe to infer from implementation alone. The product roadmap remains [Roadmap #1](https://github.com/GendByteMaster/NumFlow/issues/1).

## Current automated baseline

The Phase 11 idle-runtime change delivered to `dev/master` passed the Windows quality gate before commit delivery:

- [x] `cargo fmt --all -- --check`
- [x] `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- [x] `cargo test --locked --workspace --all-features`
- [x] `cargo build --locked --workspace --release --all-features`
- [x] 78 automated tests at that validation point
  - 30 application tests
  - 30 `numflow-core` tests
  - 18 `numflow-windows` tests
- [x] `Cargo.lock` committed and CI uses `--locked`
- [x] fixed-interval Slint UI polling removed in favour of event-driven wakeups
- [x] background worker sleeps on channels while pointer motion is idle
- [x] runtime command queue bounded
- [x] keyboard-hook event queue bounded and hook delivery non-blocking
- [x] startup interception race fixed so interception is not enabled before runtime readiness
- [x] Num Lock mode switching implemented in the global hook
- [x] physical Num Lock observation implemented
- [x] tagged Num Lock `SendInput` replay implemented to preserve Windows toggle/LED state
- [x] Num Lock autorepeat is suppressed by edge-state tracking
- [x] physical Num Lock is always passed through; it does not depend on a deferred replay
- [x] separate asynchronous audio cues implemented for NumFlow On/Off
- [x] Num Lock interception change passed formatting, Clippy, tests, and release build on Windows CI

These items are automated evidence only. They do not replace the manual matrix below.

## Phase 11 work still open

### Runtime/backpressure

- [x] `RuntimeEvent → UI` delivery uses a bounded, non-blocking event queue with stale-event eviction under pressure.
- [ ] Verify a stalled/minimized UI cannot cause unbounded memory growth.
- [ ] Verify fault delivery is not silently lost under UI backpressure.
- [ ] Add focused tests for event coalescing/overflow behaviour.

### Resource stability

- [ ] Record idle CPU behaviour on a real Windows system.
- [ ] Record idle memory behaviour.
- [ ] 15-minute active-use soak test.
- [ ] 2-hour idle/background soak test.
- [ ] Confirm no monotonic memory growth during soak.
- [ ] Confirm no stuck held-button state after soak/error/exit scenarios.

## Manual Windows test matrix

Do not check these from CI alone.

### Operating systems

- [ ] Windows 10.
- [ ] Windows 11.

### DPI / scaling

- [ ] 100% DPI.
- [ ] 125% DPI.
- [ ] 150% DPI.
- [ ] 175% DPI.
- [ ] 200% DPI.

For every scaling level verify:

- settings layout remains usable;
- no clipped labels/controls;
- Bindings panel remains readable and reachable;
- HUD remains visible and sensibly positioned;
- focus indication remains clear.

### NumPad and keyboard behaviour

- [ ] Start NumFlow while Windows Num Lock is On and confirm NumFlow starts Off for pointer interception.
- [ ] Start NumFlow while Windows Num Lock is Off and confirm NumFlow starts On for pointer interception.
- [ ] Physical Num Lock press is observed by NumFlow and passed through while runtime mode changes
      immediately from the same hook edge.
- [ ] Num Lock On → NumPad `0–9` enter ordinary digits in a text editor.
- [ ] Num Lock Off → NumPad controls the system cursor.
- [ ] Windows Num Lock LED/toggle state follows every successful NumFlow mode switch.
- [ ] NumFlow does not double-toggle when its own tagged Num Lock replay re-enters `WH_KEYBOARD_LL`.
- [ ] Holding Num Lock does not toggle repeatedly from key autorepeat.
- [ ] Rapid repeated Num Lock presses remain synchronized with Windows state/LED.
- [ ] Num Lock switching works with the NumFlow settings window unfocused/minimized.
- [ ] Num Lock switching works while another standard desktop application is foreground.
- [ ] Separate short audio cue is heard for NumFlow On.
- [ ] Separate short audio cue is heard for NumFlow Off.
- [ ] Mode audio does not introduce noticeable keyboard/input delay.
- [ ] Audio worker remains stable during rapid Num Lock toggling.
- [ ] Externally injected Num Lock input is not consumed and NumFlow mirrors the resulting mode change.
- [ ] Explicit UI/lifecycle replay failure leaves NumFlow in a safe disabled/recovering state and
      reports the target integrity/UIPI diagnostic.
- [ ] Switching Num Lock On during movement stops NumFlow pointer interception immediately.
- [ ] Switching Num Lock On during hold/drag safely releases the held mouse button.
- [ ] key-down/key-up remains correct during rapid NumPad input.
- [ ] diagonal movement remains correct.
- [ ] repeated click/select input does not desynchronize state.
- [ ] emergency disable remains reachable.
- [ ] settings keyboard navigation does not trap focus.

### Foreground/background applications

- [ ] File Explorer.
- [ ] Browser.
- [ ] Text/code editor.
- [ ] Standard desktop application while NumFlow settings are unfocused.
- [ ] Behaviour with an elevated/admin application documented as UIPI limitation.

### Pointer and drag safety

- [ ] Left click.
- [ ] Right click.
- [ ] Middle click.
- [ ] Double click.
- [ ] Hold / drag lock.
- [ ] Release.
- [ ] Change selected button while another button is held.
- [ ] Disable NumFlow while dragging.
- [ ] Exit application while dragging.
- [ ] Error/fail-safe path releases held button.
- [ ] Disabled NumFlow produces no intentional pointer movement.

### System lifecycle

- [ ] Main window close/minimize behaviour.
- [ ] Tray open/settings action.
- [ ] Tray mode/status synchronization with Num Lock.
- [ ] Start minimized.
- [ ] Start with Windows registration uses `--background` and opens no settings window after sign-in.
- [ ] Single-instance behaviour.
- [ ] Five consecutive Sleep → Wake → Unlock cycles restore NumPad immediately without toggling
  Num Lock, opening the window, restarting NumFlow, or waiting several seconds.
- [ ] Lock → Unlock, Task Manager → Sleep → Wake, and Ctrl+Alt+Del → Cancel preserve lifecycle
  recovery and NumPad input.
- [ ] Keyboard reconnect preserves `raw_input_state=keyboard-disabled-hook-owned` and
  `keyboard_device_notifications=true` while restoring NumPad input.
- [ ] Movement and NumFlow-owned mouse holds are released across Sleep/Lock.
- [ ] Multi-monitor movement/use.
- [ ] Repeated Num Lock On/Off cycles.
- [ ] Clean application shutdown.

## Accessibility pass

- [ ] Complete keyboard navigation through all settings.
- [ ] Visible focus on interactive controls.
- [ ] Status is not communicated by color alone.
- [ ] Selected mouse button is available as text/icon state.
- [ ] NumFlow On/Off state is available as text/icon state.
- [ ] Num Lock mode meaning is understandable without relying only on the hardware LED.
- [ ] Precision state is available as text/icon state.
- [ ] Accessible labels exist where Slint/platform support allows.
- [ ] Sufficient contrast under normal Windows themes.
- [ ] High-contrast behaviour reviewed.
- [ ] Reduce-motion-friendly behaviour reviewed.
- [ ] HUD does not obscure the active pointer target in normal use.
- [ ] Drag state remains visually understandable.

## Windows AT / desktop transition matrix

Run this section only with the per-machine MSI installed. Repeat the higher-integrity cases with the
explicit `--elevated` profile and, separately, with a signed production `uiAccess=true` build.

For every row verify cursor movement, left/right click, hold/release, no duplicate input, no dropped
NumPad events, correct Num Lock restoration, `mouse_hold=false` after transition, and exactly one
active hook on the current input desktop.

| Foreground or transition | Movement/click/hold | No duplicates/drops | Num Lock restored | Diagnostics captured |
| --- | --- | --- | --- | --- |
| NumFlow focused | [ ] | [ ] | [ ] | [ ] |
| File Explorer | [ ] | [ ] | [ ] | [ ] |
| Chrome/browser | [ ] | [ ] | [ ] | [ ] |
| IDE/editor | [ ] | [ ] | [ ] | [ ] |
| Normal Terminal | [ ] | [ ] | [ ] | [ ] |
| Task Manager | [ ] | [ ] | [ ] | [ ] |
| Administrator Terminal | [ ] | [ ] | [ ] | [ ] |
| Elevated PowerShell | [ ] | [ ] | [ ] | [ ] |
| NumFlow background/tray | [ ] | [ ] | [ ] | [ ] |
| Num Lock On/Off | [ ] | [ ] | [ ] | [ ] |
| Keyboard disconnect/reconnect | [ ] | [ ] | [ ] | [ ] |
| Sleep then resume | [ ] | [ ] | [ ] | [ ] |
| Default → UAC secure → Default | [ ] | [ ] | [ ] | [ ] |
| Default → Lock → Unlock → Default | [ ] | [ ] | [ ] | [ ] |
| Logon desktop, when Windows starts the registered AT | [ ] | [ ] | [ ] | [ ] |

- [ ] `numflow-secure.exe` never opens the main window, tray, or HUD.
- [ ] Default runtime logs `hook_active=false` while the protected desktop owns input.
- [ ] Secure runtime exits after the protected desktop loses input.
- [ ] Installer repair updates both AT records.
- [ ] Uninstall removes both AT records without stale `StartExe` paths.
- [ ] Installed executable signatures and MSI signature validate against the intended publisher.
- [ ] Installed production build reports `at_registered=true` and `uiaccess=true` where expected.

## Packaging and Phase 12

- [x] Generate production Windows application icon resources from `assets/numflow-icon.svg`.
- [ ] Add/finalize executable metadata and production code signing.
- [ ] Confirm release version.
- [x] Distribution format selected: WiX Toolset 4 x64 MSI plus portable x64 ZIP.
- [x] Release workflow produces a clean locked release build before packaging.
- [x] Release workflow generates SHA-256 checksums for MSI and portable ZIP.
- [ ] Verify installer, upgrade, and uninstall on clean Windows 10 and Windows 11 environments.
- [x] README and dedicated installation/releasing documentation describe packaged builds.
- [x] UIPI and unsigned-build limitations are documented.
- [x] `CHANGELOG.md` tracks the release candidate.
- [x] Portable archive includes project and UI-SFX license/source notices.
- [x] Release-PR workflow builds and structurally verifies the MSI plus portable ZIP contents.

## Final release gate

Before merging `dev/master` into `master`:

- [ ] all required automated CI checks green on the exact release candidate commit;
- [ ] Phase 11 reliability/backpressure work complete;
- [ ] required manual Windows matrix recorded;
- [ ] no unresolved safety-critical drag/input issue;
- [ ] packaging reproducible;
- [ ] release documentation matches actual behaviour;
- [ ] explicit approval to open/merge the release PR.

`master` must not be updated directly during v0.1 development.
