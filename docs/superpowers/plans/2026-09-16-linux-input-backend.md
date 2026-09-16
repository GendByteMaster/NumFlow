# NumFlow Linux Input Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a fail-safe Linux global input backend for NumFlow that captures physical NumPad input with evdev and emits replay keyboard/pointer events with uinput on Wayland and X11.

**Architecture:** Add a dedicated `numflow-linux` crate that owns Linux device discovery, event routing, exclusive grabs, uinput replay/pointer devices, permission diagnostics, and lifecycle recovery. Refactor the existing application runtime so the pointer/controller state machine is platform-neutral, then adapt both Windows and Linux input events into the same runtime event type while keeping `numflow-core` free of OS code.

**Tech Stack:** Rust 1.98, Rust 2024, `evdev` 0.13.2, `crossbeam-channel` 0.5, `thiserror` 2, Linux evdev/uinput, existing `numflow-core`.

**Spec:** `docs/superpowers/specs/2026-09-16-linux-input-backend-design.md`

## Global Constraints

- Branch: `feat/linux-input-backend`; base: `dev/master`; related Issue: #6; Draft PR: #5.
- Risk is **High** because the backend reads/grabs system keyboard event devices and prepares local input-permission policy.
- Do not run the NumFlow GUI as root and do not add a setuid executable.
- Do not install or activate udev/group/uaccess permission changes automatically; repository work may only prepare the rule/documentation until explicit user/admin approval is given.
- Do not use world-writable (`0666`) access to `/dev/input` or `/dev/uinput`.
- Ordinary keyboard events are transient only: never persist, transmit, or log key content.
- Create and verify replay/output devices before any `EVIOCGRAB` call.
- On any fatal replay/capture/output error after a grab, stop movement, release NumFlow-held mouse buttons, and ungrab physical keyboards.
- Device enumeration and capability inspection are non-exclusive and may occur before uinput creation; the exclusive `grab()` remains the final ownership step after replay/output devices are ready.
- Keep motion, bindings, controller state, acceleration, selected-button state, and held-button semantics in `numflow-core` / shared runtime code.
- Keep the Linux crate `unsafe_code = "deny"`.
- Do not add XTest, GlobalShortcuts, RemoteDesktop/libei/EIS, secure-login-screen support, or compositor-specific injection in this plan.
- Preserve the Cargo lockfile and update it deliberately when `evdev` is added.
- Windows behavior and Windows CI must remain green throughout the work.

---

## File Structure

### New Linux crate

- `crates/numflow-linux/Cargo.toml` — Linux backend dependencies and lint policy.
- `crates/numflow-linux/src/lib.rs` — public Linux backend API and exported types.
- `crates/numflow-linux/src/events.rs` — Linux key state, normalized input/lifecycle events, and key-code mapping.
- `crates/numflow-linux/src/routing.rs` — Num Lock routing decisions, repeat suppression, replay/consume classification.
- `crates/numflow-linux/src/devices.rs` — physical keyboard discovery, capability union, NumFlow virtual-device recursion filtering.
- `crates/numflow-linux/src/replay.rs` — uinput replay keyboard construction and event emission.
- `crates/numflow-linux/src/pointer.rs` — uinput virtual mouse implementing `numflow_core::PointerBackend`.
- `crates/numflow-linux/src/capture.rs` — evdev reads, exclusive-grab ownership, packet forwarding, and ungrab RAII.
- `crates/numflow-linux/src/lifecycle.rs` — process-wide supervisor, startup ordering, fatal teardown, rescan/backoff.
- `crates/numflow-linux/src/permissions.rs` — explicit permission/error classification and diagnostics.

### Existing application files

- `Cargo.toml` — add `crates/numflow-linux` workspace member and target-specific Linux dependencies.
- `Cargo.lock` — lock `evdev` 0.13.2 and transitive dependencies.
- `src/runtime.rs` — make runtime machine/input event representation reusable by Windows and Linux; replace Linux no-op runtime with a real worker.
- `src/platform_input/linux.rs` — thin Linux setup boundary only.
- `.github/workflows/ci.yml` — add Linux quality gate without pretending privileged device tests ran.
- `packaging/linux/99-numflow.rules` — narrowly scoped example installation rule; not automatically applied by CI/runtime.
- `docs/LINUX_INPUT.md` — Linux permissions, diagnostics, limitations, and manual setup.
- `README.md`, `docs/DEVELOPMENT.md`, `docs/PLATFORM_BACKENDS.md`, `docs/INSTALLATION.md`, `docs/RELEASE_CHECKLIST.md` — implementation status and validation requirements.

---

### Task 1: Extract a platform-neutral runtime input machine

**Files:**
- Modify: `src/runtime.rs`

**Interfaces:**
- Produces: `RuntimeKeyState`, `RuntimeKeyEvent`, and a platform-neutral `RuntimeMachine<B: PointerBackend>` used by both Windows and Linux worker modules.
- Preserves: all existing `BackgroundRuntime` public methods and Windows behavior.

- [ ] **Step 1: Add failing tests for the platform-neutral event type**

Move the existing `RuntimeMachine` tests out of the Windows-only module and make them construct this event type:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeKeyState {
    Pressed,
    Released,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RuntimeKeyEvent {
    key: NumpadKey,
    action: InputAction,
    state: RuntimeKeyState,
    repeated: bool,
}
```

Update the test helper to:

```rust
fn pressed(key: NumpadKey, action: InputAction) -> RuntimeKeyEvent {
    RuntimeKeyEvent {
        key,
        action,
        state: RuntimeKeyState::Pressed,
        repeated: false,
    }
}
```

- [ ] **Step 2: Run the focused runtime tests and confirm failure before the refactor**

Run:

```text
cargo test --locked runtime:: --all-features
```

Expected: compile failure because `RuntimeKeyState` / `RuntimeKeyEvent` do not yet exist at the shared scope.

- [ ] **Step 3: Move `RuntimeMachine` and its generic helpers outside `#[cfg(windows)] mod platform`**

Keep these methods platform-neutral and unchanged in semantics:

```rust
impl<B: PointerBackend> RuntimeMachine<B> {
    fn new(config: RuntimeConfig, pointer: B) -> Self;
    fn enabled(&self) -> bool;
    fn snapshot(&self) -> RuntimeStateSnapshot;
    fn configure(&mut self, config: RuntimeConfig) -> Result<(), B::Error>;
    fn set_motion_config(&mut self, config: MotionConfig);
    fn set_bindings(&mut self, bindings: Bindings);
    fn apply_action(&mut self, action: InputAction) -> Result<Vec<CoreEffect>, B::Error>;
    fn handle_key_event(&mut self, event: RuntimeKeyEvent) -> Result<Vec<CoreEffect>, B::Error>;
    fn tick(&mut self, elapsed: Duration) -> Result<(), B::Error>;
    fn recover_motion_injection_failure(&mut self);
    fn shutdown(&mut self) -> Result<Vec<CoreEffect>, B::Error>;
}
```

Map Windows normalized input immediately before `handle_key_event`:

```rust
let event = RuntimeKeyEvent {
    key: normalized_event.key,
    action: normalized_event.action,
    state: match normalized_event.state {
        numflow_windows::KeyState::Pressed => RuntimeKeyState::Pressed,
        numflow_windows::KeyState::Released => RuntimeKeyState::Released,
    },
    repeated: normalized_event.repeated,
};
```

- [ ] **Step 4: Run Windows-compatible unit coverage**

Run:

```text
cargo fmt --all
cargo test --locked runtime:: --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
```

Expected: PASS with no change to Windows behavior.

- [ ] **Step 5: Commit the refactor**

```text
git add src/runtime.rs
git commit -m "refactor(runtime): share input state machine across platforms"
```

---

### Task 2: Add `numflow-linux` crate and pure key routing

**Files:**
- Create: `crates/numflow-linux/Cargo.toml`
- Create: `crates/numflow-linux/src/lib.rs`
- Create: `crates/numflow-linux/src/events.rs`
- Create: `crates/numflow-linux/src/routing.rs`
- Create: `crates/numflow-linux/src/permissions.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`

**Interfaces:**
- Produces: `LinuxKeyState`, `LinuxInputEvent`, `RoutingDecision`, `NumLockRouter`, `LinuxInputError`.
- Consumes: `numflow_core::NumpadKey` only for pure mapping/routing.

- [ ] **Step 1: Add workspace/dependencies and failing mapping tests**

Use this crate manifest:

```toml
[package]
name = "numflow-linux"
version = "0.1.0"
edition = "2024"
rust-version = "1.98"
description = "Linux input backends for NumFlow"
license = "MIT"
repository = "https://github.com/GendByteMaster/NumFlow"

[dependencies]
crossbeam-channel = "0.5"
evdev = "0.13.2"
numflow-core = { path = "../numflow-core" }
thiserror = "2"

[lints.rust]
unsafe_code = "deny"

[lints.clippy]
all = "warn"
pedantic = "warn"
```

Add `crates/numflow-linux` to `[workspace].members` and add:

```toml
[target.'cfg(target_os = "linux")'.dependencies]
crossbeam-channel = "0.5"
numflow-linux = { path = "crates/numflow-linux" }
```

The first routing tests must assert at least:

```rust
assert_eq!(map_numpad_key(KeyCode::KEY_KP8), Some(NumpadKey::Num8));
assert_eq!(map_numpad_key(KeyCode::KEY_KPPLUS), Some(NumpadKey::Add));
assert_eq!(map_numpad_key(KeyCode::KEY_KPDOT), Some(NumpadKey::Decimal));
assert_eq!(map_numpad_key(KeyCode::KEY_A), None);
```

- [ ] **Step 2: Run crate tests and confirm they fail**

Run:

```text
cargo test --locked -p numflow-linux
```

Expected: FAIL because mapping/routing functions do not exist yet.

- [ ] **Step 3: Implement exact pure event/routing types**

Use:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxKeyState {
    Pressed,
    Released,
    Repeated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxInputEvent {
    Numpad {
        key: NumpadKey,
        state: LinuxKeyState,
    },
    NumLockChanged {
        num_lock_on: bool,
    },
    InputUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecision {
    Replay,
    Consume(LinuxInputEvent),
}
```

Map all NumFlow keypad codes explicitly in `events.rs`; do not map top-row digits.

`NumLockRouter` owns `num_lock_on: bool` and `num_lock_pressed: bool`. `KEY_NUMLOCK` value `1` toggles once per physical press, value `2` is replay-only repeat, and value `0` clears the pressed latch. When Num Lock is On, keypad keys return `Replay`; when Off, mapped keypad keys return `Consume(...)`; unrelated keys always return `Replay`.

- [ ] **Step 4: Add permission error taxonomy without touching live permissions**

```rust
#[derive(Debug, thiserror::Error)]
pub enum LinuxInputError {
    #[error("cannot read Linux input device {path}: {source}")]
    InputRead {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot exclusively grab Linux keyboard {path}: {source}")]
    InputGrab {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot open /dev/uinput: {0}")]
    UinputOpen(std::io::Error),
    #[error("cannot create NumFlow virtual device: {0}")]
    VirtualDevice(std::io::Error),
    #[error("Linux input runtime is unavailable: {0}")]
    Runtime(String),
}
```

- [ ] **Step 5: Run pure crate tests and quality checks**

```text
cargo fmt --all
cargo test --locked -p numflow-linux
cargo clippy --locked -p numflow-linux --all-targets --all-features -- -D warnings
```

Expected: PASS without requiring `/dev/input` or `/dev/uinput`.

- [ ] **Step 6: Commit**

```text
git add Cargo.toml Cargo.lock crates/numflow-linux
git commit -m "feat(linux): add input routing foundation"
```

---

### Task 3: Implement physical keyboard discovery and recursion filtering

**Files:**
- Create: `crates/numflow-linux/src/devices.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Produces: `KeyboardCandidate`, `discover_keyboards()`, `union_supported_keys()`.
- Consumes: evdev device metadata only; does not call `grab()`.

- [ ] **Step 1: Write deterministic capability tests around extracted metadata**

Define testable metadata:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub path: PathBuf,
    pub name: Option<String>,
    pub physical_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct KeyboardCandidate {
    pub identity: DeviceIdentity,
    pub supported_keys: AttributeSet<KeyCode>,
}
```

Tests must prove that a candidate requires `KEY_NUMLOCK` plus every NumFlow keypad key, that a device named with the reserved prefix `NumFlow Virtual ` is excluded, and that two candidates produce a union containing capabilities from both.

- [ ] **Step 2: Run tests and confirm failure**

```text
cargo test --locked -p numflow-linux devices::
```

- [ ] **Step 3: Implement non-exclusive discovery**

Use `evdev::enumerate()` to inspect devices. Discovery may open/read capability metadata but must not call `Device::grab()`.

Reserve these virtual names:

```rust
pub const REPLAY_DEVICE_NAME: &str = "NumFlow Virtual Keyboard";
pub const POINTER_DEVICE_NAME: &str = "NumFlow Virtual Mouse";
pub const VIRTUAL_DEVICE_PREFIX: &str = "NumFlow Virtual ";
```

Return `Vec<KeyboardCandidate>` with paths/capabilities sorted by path for deterministic diagnostics.

- [ ] **Step 4: Verify**

```text
cargo test --locked -p numflow-linux devices::
cargo clippy --locked -p numflow-linux --all-targets --all-features -- -D warnings
```

- [ ] **Step 5: Commit**

```text
git add crates/numflow-linux/src/devices.rs crates/numflow-linux/src/lib.rs
git commit -m "feat(linux): discover physical NumPad keyboards"
```

---

### Task 4: Implement uinput replay keyboard and virtual mouse

**Files:**
- Create: `crates/numflow-linux/src/replay.rs`
- Create: `crates/numflow-linux/src/pointer.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Produces: `ReplayKeyboard::create(&AttributeSetRef<KeyCode>)`, `ReplayKeyboard::emit(&[InputEvent])`, `LinuxPointer::create()`, `PointerBackend for LinuxPointer`.
- Consumes: the capability union from Task 3.

- [ ] **Step 1: Add unit tests for pointer event translation behind a writer abstraction**

Create a private writer trait so event translation can be tested without `/dev/uinput`:

```rust
trait EventWriter {
    fn emit(&mut self, events: &[InputEvent]) -> std::io::Result<()>;
}
```

Tests must assert that:

```rust
move_relative(12, -7)
```

produces `REL_X=12` and `REL_Y=-7`; `button_down(MouseButton::Left)` emits `BTN_LEFT=1`; `button_up` emits `BTN_LEFT=0`; `release_all()` emits releases only for buttons tracked as held.

- [ ] **Step 2: Run pointer tests and confirm failure**

```text
cargo test --locked -p numflow-linux pointer::
```

- [ ] **Step 3: Build the virtual replay keyboard**

Use `evdev::uinput::VirtualDevice::builder()` and:

```rust
let device = VirtualDevice::builder()?
    .name(REPLAY_DEVICE_NAME)
    .with_keys(keys)?
    .build()?;
```

`ReplayKeyboard::emit` forwards event batches using `VirtualDevice::emit`; it must not log key codes or values.

- [ ] **Step 4: Build the virtual pointer**

Create an `AttributeSet<RelativeAxisCode>` containing `REL_X` and `REL_Y`, and an `AttributeSet<KeyCode>` containing `BTN_LEFT`, `BTN_RIGHT`, `BTN_MIDDLE`.

Build with:

```rust
let device = VirtualDevice::builder()?
    .name(POINTER_DEVICE_NAME)
    .with_keys(&buttons)?
    .with_relative_axes(&axes)?
    .build()?;
```

Implement all six `PointerBackend` methods and track held buttons locally so `release_all()` is idempotent.

- [ ] **Step 5: Verify pure translation tests**

```text
cargo fmt --all
cargo test --locked -p numflow-linux pointer::
cargo clippy --locked -p numflow-linux --all-targets --all-features -- -D warnings
```

Do not require the hosted runner to have `/dev/uinput` for these tests.

- [ ] **Step 6: Commit**

```text
git add crates/numflow-linux/src/replay.rs crates/numflow-linux/src/pointer.rs crates/numflow-linux/src/lib.rs
git commit -m "feat(linux): add uinput replay and pointer devices"
```

---

### Task 5: Implement capture packets, exclusive-grab ownership, and fail-open teardown

**Files:**
- Create: `crates/numflow-linux/src/capture.rs`
- Modify: `crates/numflow-linux/src/routing.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Produces: `CapturedKeyboard`, `process_event_batch(...)`.
- Requires: `ReplayKeyboard` must exist before `CapturedKeyboard::grab(...)` is invoked.

- [ ] **Step 1: Add routing tests for mixed event batches**

Given a batch containing `KEY_A`, `KEY_KP8`, and synchronization events, tests must prove:

- Num Lock On: all input events are replayed in original order.
- Num Lock Off: `KEY_KP8` press/release becomes `LinuxInputEvent::Numpad`, while `KEY_A` remains in replay output.
- `KEY_NUMLOCK` is replayed and produces exactly one `NumLockChanged` transition per press edge.
- `SYN_REPORT` is not duplicated manually when `VirtualDevice::emit` already terminates a batch.

- [ ] **Step 2: Run and confirm failure**

```text
cargo test --locked -p numflow-linux routing::
```

- [ ] **Step 3: Implement RAII grab ownership**

`CapturedKeyboard` owns `evdev::Device` plus its path. Its constructor performs `device.grab()` and returns `LinuxInputError::InputGrab` on failure. Its `Drop` explicitly calls `ungrab()` as a best-effort safety action even though evdev 0.13.2 also ungrabs on drop.

Expose:

```rust
impl CapturedKeyboard {
    pub fn grab(candidate: KeyboardCandidate) -> Result<Self, LinuxInputError>;
    pub fn fetch_events(&mut self) -> Result<Vec<InputEvent>, LinuxInputError>;
    pub fn ungrab(&mut self) -> Result<(), LinuxInputError>;
}
```

- [ ] **Step 4: Implement batch processing with no key-content logging**

Return a struct:

```rust
pub struct ProcessedBatch {
    pub replay: Vec<InputEvent>,
    pub runtime: Vec<LinuxInputEvent>,
}
```

The function must be deterministic from `(events, router state)` and have no I/O.

- [ ] **Step 5: Verify**

```text
cargo test --locked -p numflow-linux routing::
cargo test --locked -p numflow-linux capture::
cargo clippy --locked -p numflow-linux --all-targets --all-features -- -D warnings
```

- [ ] **Step 6: Commit**

```text
git add crates/numflow-linux/src/capture.rs crates/numflow-linux/src/routing.rs crates/numflow-linux/src/lib.rs
git commit -m "feat(linux): add fail-safe keyboard capture"
```

---

### Task 6: Add the process-wide Linux supervisor and lifecycle recovery

**Files:**
- Create: `crates/numflow-linux/src/lifecycle.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Produces: `LinuxInputRuntime`, `LinuxRuntimeEventReceiver`, `LinuxPointer` ownership handoff.
- Guarantees: no physical `grab()` before replay keyboard and pointer devices are created.

- [ ] **Step 1: Write state-machine tests for startup/teardown ordering**

Model lifecycle phases:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackendPhase {
    Discovering,
    OutputsReady,
    Grabbing,
    Running,
    Recovering,
    Stopped,
}
```

Tests must prove that `Grabbing` cannot be entered from `Discovering`, fatal failure from `Running` requests pointer release before ungrab, and recovery uses bounded backoff.

- [ ] **Step 2: Run and confirm failure**

```text
cargo test --locked -p numflow-linux lifecycle::
```

- [ ] **Step 3: Implement startup in this exact safety order**

1. detect session diagnostics (`XDG_SESSION_TYPE` only for diagnostics, not behavior selection);
2. enumerate physical keyboard candidates and compute their capability union without grabbing;
3. verify/create `ReplayKeyboard` from that union;
4. verify/create `LinuxPointer`;
5. start bounded event/output channels;
6. open and `grab()` selected physical keyboards;
7. start capture workers;
8. publish `Running` only after every grabbed keyboard has an active replay path.

If steps 1-5 fail, no device is grabbed. If any grab/worker step fails after a previous keyboard was grabbed, ungrab all already-grabbed devices before returning the error.

- [ ] **Step 4: Implement fatal teardown**

The supervisor shutdown sequence must be:

```text
stop accepting new runtime input
stop movement request
release_all() virtual mouse buttons
ungrab every physical keyboard
join capture workers
drop virtual devices
publish stopped/fault state
```

No retry may keep a grab while replay is absent.

- [ ] **Step 5: Implement bounded recovery**

Use backoff constants:

```rust
const RECOVERY_INITIAL_DELAY: Duration = Duration::from_millis(250);
const RECOVERY_MAX_DELAY: Duration = Duration::from_secs(5);
```

Reset to the initial delay after a stable successful rebuild. Recovery scans only while degraded; steady-state stays event-driven.

- [ ] **Step 6: Verify lifecycle tests**

```text
cargo test --locked -p numflow-linux lifecycle::
cargo clippy --locked -p numflow-linux --all-targets --all-features -- -D warnings
```

- [ ] **Step 7: Commit**

```text
git add crates/numflow-linux/src/lifecycle.rs crates/numflow-linux/src/lib.rs
git commit -m "feat(linux): add input lifecycle supervisor"
```

---

### Task 7: Integrate the Linux backend with `BackgroundRuntime`

**Files:**
- Modify: `src/runtime.rs`
- Modify: `src/platform_input/linux.rs`

**Interfaces:**
- Consumes: `numflow_linux::{LinuxInputRuntime, LinuxInputEvent, LinuxKeyState, LinuxPointer}`.
- Preserves: existing `BackgroundRuntime` public API used by UI/application code.

- [ ] **Step 1: Add Linux runtime tests with fake Linux events and mock pointer**

Tests must prove:

- `NumLockChanged { num_lock_on: false }` enables the shared runtime machine;
- a consumed `Num8 Pressed` starts upward motion and `Released` stops it;
- fatal Linux input unavailability calls runtime fail-safe and disables/clears held state;
- mapped enable/disable actions cannot override Num Lock ownership.

- [ ] **Step 2: Run focused tests and confirm failure**

```text
cargo test --locked runtime:: --all-features
```

- [ ] **Step 3: Replace the non-Windows no-op only on Linux**

Split the bottom-level cfgs so macOS keeps the stub while Linux gets a real module:

```rust
#[cfg(target_os = "linux")]
mod linux_platform;
#[cfg(target_os = "linux")]
pub use linux_platform::BackgroundRuntime;

#[cfg(target_os = "macos")]
#[derive(Debug, Default)]
pub struct BackgroundRuntime;
```

The Linux worker mirrors the existing bounded command/event model and uses the shared `RuntimeMachine<LinuxPointer>` with the same 8 ms motion tick.

Map Linux input to shared events:

```rust
let runtime_event = RuntimeKeyEvent {
    key,
    action,
    state: match state {
        LinuxKeyState::Pressed | LinuxKeyState::Repeated => RuntimeKeyState::Pressed,
        LinuxKeyState::Released => RuntimeKeyState::Released,
    },
    repeated: state == LinuxKeyState::Repeated,
};
```

- [ ] **Step 4: Keep `src/platform_input/linux.rs` thin**

It should perform only the post-UI platform preparation that genuinely belongs there. If Linux requires no winit registration fix, return `Ok(())` with no device grabbing; all grabs belong to `BackgroundRuntime::start` so lifecycle ownership has one place.

- [ ] **Step 5: Verify root tests/build on Linux-compatible code paths**

```text
cargo fmt --all
cargo test --locked --workspace --all-features
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
```

- [ ] **Step 6: Commit**

```text
git add src/runtime.rs src/platform_input/linux.rs
git commit -m "feat(linux): connect evdev backend to runtime"
```

---

### Task 8: Prepare narrow Linux permission installation assets without activating them

**Files:**
- Create: `packaging/linux/99-numflow.rules`
- Create: `docs/LINUX_INPUT.md`
- Modify: `crates/numflow-linux/src/permissions.rs`

**Interfaces:**
- Produces: auditable setup instructions and actionable runtime diagnostics.
- Does not execute: `sudo`, `udevadm`, group mutation, file installation, or permission changes.

- [ ] **Step 1: Add permission classification tests**

Map `PermissionDenied` from `/dev/input/event*` to physical-input access diagnostics, `/dev/uinput` denial to uinput diagnostics, and other I/O errors to their underlying path-specific message.

- [ ] **Step 2: Create the permission rule as an installation asset**

Use a narrow rule that grants an installation-created `numflow-input` group access to event devices and uinput instead of `0666`:

```text
KERNEL=="event*", SUBSYSTEM=="input", GROUP="numflow-input", MODE="0660"
KERNEL=="uinput", SUBSYSTEM=="misc", GROUP="numflow-input", MODE="0660"
```

Document clearly that this still grants the group broad keyboard-event visibility and therefore must be an explicit administrator installation choice. Do not copy this file into `/etc/udev/rules.d` from application runtime or CI.

- [ ] **Step 3: Document manual installation/rollback commands**

`docs/LINUX_INPUT.md` must include the explicit administrator-controlled setup and rollback, but label them manual. Rollback must remove the installed NumFlow rule/group membership and reload udev before re-login/reboot as appropriate.

- [ ] **Step 4: Verify no automatic privilege-changing code exists**

Run:

```text
git grep -nE "sudo|setuid|chmod 666|MODE=\"0666\"|udevadm control" -- ':!docs/LINUX_INPUT.md'
```

Expected: no runtime/CI code that changes Linux permissions.

- [ ] **Step 5: Commit preparation only**

```text
git add packaging/linux/99-numflow.rules docs/LINUX_INPUT.md crates/numflow-linux/src/permissions.rs
git commit -m "docs(linux): add input permission setup"
```

Do not activate the rule on any real machine in this task.

---

### Task 9: Add Linux CI without false privileged-device claims

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: `linux-quality` job parallel to `windows-quality`.

- [ ] **Step 1: Add the Linux quality job**

Use `ubuntu-latest`, Rust 1.98, rustfmt, clippy, rust-cache, and the same four quality commands:

```yaml
  linux-quality:
    name: Linux quality gate
    runs-on: ubuntu-latest
    steps:
      - name: Checkout
        uses: actions/checkout@v6
        with:
          fetch-depth: 0
      - name: Install Rust
        uses: dtolnay/rust-toolchain@1.98.0
        with:
          components: rustfmt, clippy
      - name: Cache Rust build
        uses: Swatinem/rust-cache@v2
      - name: Diff whitespace
        shell: bash
        run: |
          if [[ "${{ github.event_name }}" == "pull_request" ]]; then
            git diff --check "${{ github.event.pull_request.base.sha }}...HEAD"
          else
            git diff --check HEAD^ HEAD
          fi
      - name: Formatting
        run: cargo fmt --all -- --check
      - name: Clippy
        run: cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
      - name: Tests
        run: cargo test --locked --workspace --all-features
      - name: Release build
        run: cargo build --locked --workspace --release --all-features
```

- [ ] **Step 2: Ensure privileged integration tests are ignored/feature-gated rather than silently skipped as success**

Any test that needs real `/dev/input` or `/dev/uinput` must be named/documented as an integration/manual test and must not make hosted CI claim real device capture coverage.

- [ ] **Step 3: Verify workflow syntax/diff**

```text
git diff --check
git diff -- .github/workflows/ci.yml
```

- [ ] **Step 4: Commit**

```text
git add .github/workflows/ci.yml
git commit -m "ci: add Linux quality gate"
```

---

### Task 10: Update user/developer documentation and release evidence

**Files:**
- Modify: `README.md`
- Modify: `docs/DEVELOPMENT.md`
- Modify: `docs/PLATFORM_BACKENDS.md`
- Modify: `docs/INSTALLATION.md`
- Modify: `docs/RELEASE_CHECKLIST.md`
- Modify: `docs/LINUX_INPUT.md`
- Modify: Draft PR #5 body after implementation evidence exists

**Interfaces:**
- Documents exactly what is implemented and what still requires manual validation.

- [ ] **Step 1: Update backend status**

State that Linux uses evdev capture plus uinput replay/pointer injection, works below Wayland/X11, requires explicit device permissions, and is not production-ready until the manual matrix is completed.

- [ ] **Step 2: Add manual Omarchy/Hyprland matrix to release checklist**

Include every Issue #6 item: Num Lock On digits, Num Lock Off suppression, movement/diagonals, click/double-click, hold/release, button selection, background use, browser/terminal/native Wayland, rapid Num Lock, unplug/replug, suspend/resume, forced termination, permission failure, LED sync where supported.

- [ ] **Step 3: Keep deferred work explicit**

Mark XTest, GlobalShortcuts, RemoteDesktop/libei/EIS, logind/libseat fd acquisition, secure/login-screen support, and non-Arch packaging as deferred rather than implemented.

- [ ] **Step 4: Commit**

```text
git add README.md docs
git commit -m "docs(linux): document evdev input backend"
```

---

### Task 11: Final automated verification and PR evidence

**Files:**
- Review only: complete branch diff against `dev/master`.
- Update: Draft PR #5 description with actual verification results only.

**Interfaces:**
- Completion evidence for Issue #6 automated acceptance criteria.

- [ ] **Step 1: Run complete local quality gate**

```text
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release --all-features
git diff --check dev/master...HEAD
```

Expected: every command PASS.

- [ ] **Step 2: Review the complete diff for scope/privacy/permission safety**

```text
git diff --stat dev/master...HEAD
git diff dev/master...HEAD -- Cargo.toml crates/numflow-linux src/runtime.rs src/platform_input/linux.rs packaging/linux .github/workflows/ci.yml docs README.md
```

Confirm there is no key-content logging, no automatic permission activation, no shell/exec privilege helper, no setuid path, and no `0666` input rule.

- [ ] **Step 3: Verify GitHub Actions on the final head**

Require both `Windows quality gate` and `Linux quality gate` to be green on the same final commit before marking automated implementation complete.

- [ ] **Step 4: Keep PR Draft until manual Linux validation is done**

Automated success does not prove real `EVIOCGRAB`, uinput injection, device reconnect, suspend/resume, crash recovery, or Hyprland behavior. Do not mark the backend production-ready or merge solely from CI.

- [ ] **Step 5: Perform real-machine permission activation/manual tests only after explicit user approval**

Because this is High-risk permission/input work, installation of the udev rule/group membership and real exclusive-keyboard testing is a separate consequential step. Before executing it, re-confirm the exact Omarchy/Hyprland machine, show rollback commands, and obtain explicit approval.
