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

These items are automated evidence only. They do not replace the manual matrix below.

## Phase 11 work still open

### Runtime/backpressure

- [ ] Replace the remaining unbounded `RuntimeEvent → UI` delivery path with bounded/coalescing semantics.
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
- NumPad visualization remains readable;
- HUD remains visible and sensibly positioned;
- focus indication remains clear.

### NumPad and keyboard behaviour

- [ ] Num Lock On.
- [ ] Num Lock Off.
- [ ] key-down/key-up remains correct during rapid input.
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
- [ ] Tray enable/disable state synchronization.
- [ ] Start minimized.
- [ ] Start with Windows registration.
- [ ] Single-instance behaviour.
- [ ] Sleep → resume.
- [ ] Multi-monitor movement/use.
- [ ] Repeated enable/disable cycles.
- [ ] Clean application shutdown.

## Accessibility pass

- [ ] Complete keyboard navigation through all settings.
- [ ] Visible focus on interactive controls.
- [ ] Status is not communicated by color alone.
- [ ] Selected mouse button is available as text/icon state.
- [ ] On/Off state is available as text/icon state.
- [ ] Precision state is available as text/icon state.
- [ ] Accessible labels exist where Slint/platform support allows.
- [ ] Sufficient contrast under normal Windows themes.
- [ ] High-contrast behaviour reviewed.
- [ ] Reduce-motion-friendly behaviour reviewed.
- [ ] HUD does not obscure the active pointer target in normal use.
- [ ] Drag state remains visually understandable.

## Packaging and Phase 12

- [ ] Generate production Windows application icon resources from `assets/numflow-icon.svg`.
- [ ] Add executable metadata.
- [ ] Confirm release version.
- [ ] Decide distribution format:
  - [ ] portable archive, or
  - [ ] traditional installer, or
  - [ ] MSIX.
- [ ] Produce clean release build from the approved commit.
- [ ] Generate checksums for release artifacts.
- [ ] Verify artifact on a clean Windows environment.
- [ ] Finalize README usage instructions for packaged builds.
- [ ] Publish known limitations.
- [ ] Add/update changelog.
- [ ] Verify license files are included where required.

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
