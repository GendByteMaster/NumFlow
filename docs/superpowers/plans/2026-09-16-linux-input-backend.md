# NumFlow Linux Input Backend Implementation Plan

> **For Codex:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a production-oriented Linux input backend for NumFlow that preserves the existing NumPad semantics on Wayland/Hyprland and X11 by using `evdev` for physical keyboard input and `uinput` for replay/pointer output.

**Architecture:** `numflow-linux` owns Linux device discovery, Num Lock routing, replay/pointer virtual devices, exclusive capture, lifecycle recovery, and permission diagnostics. Physical keyboards are never grabbed until virtual output devices exist. Any initialization or runtime failure tears down grabs and releases held pointer buttons. The shared application consumes backend-neutral `LinuxInputEvent`/`PointerBackend` behavior rather than Linux raw key codes.

**Tech Stack:** Rust 2024, Rust 1.98, `evdev` 0.13.2, `thiserror`, existing `numflow-core`, GitHub Actions.

---

### Task 1: Bootstrap the standalone Linux backend crate

**Files:**
- Create: `crates/numflow-linux/Cargo.toml`
- Create: `crates/numflow-linux/src/lib.rs`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Produces: standalone `numflow-linux` crate that is not yet wired into the Windows root workspace/app.
- Consumes: `numflow-core` by path.

- [x] **Step 1: Create the Linux crate manifest**

```toml
[package]
name = "numflow-linux"
version = "0.1.0"
edition = "2024"
rust-version = "1.98"
publish = false

[dependencies]
evdev = "=0.13.2"
numflow-core = { path = "../numflow-core" }
thiserror = "2"
```

- [x] **Step 2: Add the crate root**

Start with `#![deny(unsafe_code)]` and empty/private modules only.

- [x] **Step 3: Add Linux-only CI commands**

Use the crate manifest directly so the Windows root workspace stays unchanged during bootstrap:

```text
cargo fmt --manifest-path crates/numflow-linux/Cargo.toml -- --check
cargo test --manifest-path crates/numflow-linux/Cargo.toml
cargo clippy --manifest-path crates/numflow-linux/Cargo.toml --all-targets --all-features -- -D warnings
```

- [x] **Step 4: Commit**

---

### Task 2: Implement Linux event mapping and Num Lock routing

**Files:**
- Create: `crates/numflow-linux/src/events.rs`
- Create: `crates/numflow-linux/src/routing.rs`
- Create: `crates/numflow-linux/src/permissions.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Produces: backend-neutral Linux input events and deterministic Num Lock routing.
- Consumes: Linux key codes only inside `numflow-linux`.

- [x] **Step 1: Define Linux input event types**

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinuxInputEvent {
    Numpad { key: NumpadKey, state: LinuxKeyState },
    NumLockChanged { num_lock_on: bool },
}
```

- [x] **Step 2: Map only NumFlow keypad keys**

Map all NumFlow keypad codes explicitly in `events.rs`; do not map top-row digits.

`NumLockRouter` owns `num_lock_on: bool` and `num_lock_pressed: bool`. `KEY_NUMLOCK` value `1` toggles once per physical press, value `2` is replay-only repeat, and value `0` clears the pressed latch. When Num Lock is On, keypad keys return `Replay`; when Off, mapped keypad keys return `Consume(...)`; unrelated keys always return `Replay`.

- [x] **Step 3: Add permission error taxonomy without touching live permissions**

Implemented `LinuxInputError` for input read/grab/ungrab, uinput creation/output, and runtime failures. No live permission mutation is performed by this crate.

- [x] **Step 4: Run pure crate tests and quality checks**

---

### Task 3: Implement physical keyboard discovery and recursion filtering

**Files:**
- Create: `crates/numflow-linux/src/devices.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Produces: `KeyboardCandidate`, `discover_keyboards()`, `union_supported_keys()`.
- Consumes: evdev device metadata only; does not call `grab()`.

- [x] **Step 1: Write deterministic capability tests around extracted metadata**

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

- [x] **Step 2: Implement non-exclusive discovery**

Uses `evdev::enumerate()` to inspect devices and never calls `Device::grab()` during discovery.

Reserved virtual names:

```rust
pub const REPLAY_DEVICE_NAME: &str = "NumFlow Virtual Keyboard";
pub const POINTER_DEVICE_NAME: &str = "NumFlow Virtual Mouse";
pub const VIRTUAL_DEVICE_PREFIX: &str = "NumFlow Virtual ";
```

- [x] **Step 3: Verify**

---

### Task 4: Implement uinput replay keyboard and virtual mouse

**Files:**
- Create: `crates/numflow-linux/src/replay.rs`
- Create: `crates/numflow-linux/src/pointer.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Produces: `ReplayKeyboard`, `LinuxPointer`, and pure translation seams.
- Consumes: the capability union from Task 3.

- [x] **Step 1: Add unit tests for pointer event translation behind a writer abstraction**

- [x] **Step 2: Build the virtual replay keyboard**

`ReplayKeyboard::emit` forwards event batches through `VirtualDevice::emit` without logging key codes or values.

- [x] **Step 3: Build the virtual pointer**

Supports `REL_X`, `REL_Y`, `BTN_LEFT`, `BTN_RIGHT`, and `BTN_MIDDLE`; held buttons are tracked locally and `release_all()` is idempotent.

- [x] **Step 4: Verify pure translation tests**

---

### Task 5: Implement capture packets, exclusive-grab ownership, and fail-open teardown

**Files:**
- Create: `crates/numflow-linux/src/capture.rs`
- Modify: `crates/numflow-linux/src/routing.rs`
- Modify: `crates/numflow-linux/src/permissions.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Produces: `CapturedKeyboard`, `process_event_batch(...)`.
- Requires: `ReplayKeyboard` and `LinuxPointer` must be created by the future supervisor before `CapturedKeyboard::grab(...)` is invoked.

- [x] **Step 1: Add routing tests for mixed event batches**

Tests prove:

- Num Lock On replays key events in original order.
- Num Lock Off consumes mapped NumPad key press/release while unrelated keys remain in replay output.
- `KEY_NUMLOCK` is replayed and produces exactly one `NumLockChanged` transition per physical press edge.
- `SYN_REPORT` is omitted from replay output because `VirtualDevice::emit` terminates emitted batches.

- [x] **Step 2: Implement deterministic batch processing**

`ProcessedBatch` contains replay and runtime events. `process_event_batch(...)` has no I/O and does not log key content.

- [x] **Step 3: Implement RAII grab ownership**

`CapturedKeyboard` opens the selected evdev path, calls `Device::grab()`, maps open/grab failures to typed errors, and tracks grab ownership.

- [x] **Step 4: Add explicit and drop-time fail-open release**

`CapturedKeyboard::ungrab()` is idempotent and reports `InputUngrab`; `Drop` performs best-effort `ungrab()` if ownership is still active.

- [x] **Step 5: Add capture read API**

`fetch_events()` returns the next evdev event packet as a `Vec<InputEvent>` and maps read failures to `InputRead` without logging event contents.

- [x] **Step 6: Verify without live input access**

CI regression coverage uses a guaranteed-missing `/dev/input/...` path to prove open failure occurs before any exclusive grab is attempted. Real `EVIOCGRAB` behavior is intentionally not exercised by hosted CI.

Verification on head `47566b27e207065effb2a7f4a27b518f74f28d5a` / CI #286:

```text
Linux backend formatting: PASS
Linux backend tests: 16 passed, 0 failed
Linux backend Clippy (-D warnings): PASS
Windows diff whitespace: PASS
Windows formatting: PASS
Windows Clippy: PASS
Windows tests: PASS
Windows release build: PASS
```

---

### Task 6: Add the process-wide Linux supervisor and lifecycle recovery

**Files:**
- Create: `crates/numflow-linux/src/lifecycle.rs`
- Modify: `crates/numflow-linux/src/lib.rs`

**Interfaces:**
- Will own replay/pointer output devices before any physical grab.
- Will connect `CapturedKeyboard::fetch_events() -> process_event_batch() -> ReplayKeyboard/runtime`.
- Will own teardown, device churn, and resume recovery.

- [ ] **Step 1: Define supervisor state and startup ordering**

Create all required virtual output devices before calling `CapturedKeyboard::grab(...)`. If any output creation fails, return without grabbing physical input.

- [ ] **Step 2: Add deterministic rollback tests using abstractions/mocks**

Prove partial startup failures release every acquired grab and any held pointer buttons.

- [ ] **Step 3: Implement the capture/replay/runtime loop**

Do not log raw key codes, values, or ordinary keyboard content.

- [ ] **Step 4: Add device churn and resume recovery**

On read/device loss: fail open, release all grabs/buttons, rediscover devices, rebuild output devices, and only then reacquire grabs.

- [ ] **Step 5: Verify without requiring hosted CI to access `/dev/input` or `/dev/uinput`**

---

### Task 7: Add explicit Linux permission diagnostics and opt-in activation

**Files:**
- Add documented permission diagnostics and packaging/install integration only after the runtime supervisor is verified.

- [ ] No GUI root execution.
- [ ] No setuid binary.
- [ ] No permission broadening during tests.
- [ ] Any udev/uaccess policy must be explicit, documented, least-privilege, and manually activated.

---

### Task 8: Integrate Linux backend with the application and validate real devices

- [ ] Wire the Linux backend into the application/runtime without regressing Windows.
- [ ] Add Linux release packaging/build coverage.
- [ ] Validate Num Lock ON/OFF, every NumPad mapping, click/double-click/hold/release, failure teardown, sleep/resume, reconnect, and device churn on real Omarchy/Hyprland hardware.
- [ ] Keep PR Draft until real-device and permission-path evidence is recorded.
