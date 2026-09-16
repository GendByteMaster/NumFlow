# ForgeScope review — UIAccess input helper (no kernel driver)

Scope: implement `NumFlow.exe → restricted IPC → numflow-input.exe (UIAccess) → Windows input` so
pointer injection reaches elevated targets without a kernel driver. Branch: `dev/master`.
Constraints from the request: keep the Slint UI non-elevated; keep `numflow-core` untouched; keep a
strict typed whitelisted IPC with ownership/authentication; preserve Num Lock, hook, pointer,
drag/fail-safe, Sleep/Resume semantics; no secure-desktop/lock-screen work; keep `--elevated` as a
diagnostic/fallback path; account for signing + Program Files install; portable must not pretend to
be a supported UIAccess deployment.

## External evidence (Microsoft docs, verified via research agent)

- `SendInput` is subject to UIPI; medium-IL injection into high-IL targets is silently dropped and
  neither the return value nor `GetLastError` reports UIPI as the cause.
  https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput
- UIAccess is the documented user-mode bypass: a UIAccess process "can drive any application window
  using SendInput".
  https://learn.microsoft.com/en-us/previous-versions/dotnet/articles/bb625963(v=msdn.10)
- UIAccess requirements: signature chaining to a trusted root in the local machine store + install in
  a secure location (`%ProgramFiles%`, `%WinDir%` minus exclusions, or any local admin-only-writable
  folder). Same source as above; manifest schema:
  https://learn.microsoft.com/en-us/windows/win32/sbscs/application-manifests
- Undocumented (design conservatively, fail closed): exact launch behavior for unsigned/out-of-location
  `uiAccess=true` images; `%ProgramFiles(x86)%` acceptance; any certificate policy beyond trusted-root
  chaining; whether `ChangeWindowMessageFilterEx` can re-enable `SendInput` (assume no).
- Pipe security: the DACL is the boundary; PID/image-name checks are hygiene only (docs do not
  guarantee them as a security boundary). `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE`
  used for hardening.

## Architecture decision (CONFIRMED)

```
NumFlow.exe (medium IL: Slint UI, WH_KEYBOARD_LL, Num Lock, motion, state, audio, tray)
        │  spawns numflow-input.exe from its own install directory (same user, same session)
        │  named pipe: \\.\pipe\numflow-input-{session}  (local-only, first-instance, typed frames)
        ▼
numflow-input.exe (asInvoker + uiAccess="true"; signed + Program Files in production)
        │  SendInput only: pointer move, button down/up, click, double click, release
        ▼
Windows input stack → high-IL targets reachable when UIAccess is honored
```

- The keyboard hook, Num Lock lifecycle, recovery, singleton ownership, audio, and UI stay in
  NumFlow.exe — LL hooks receive input regardless of target integrity, so only the *injection* path
  needs UIAccess. This preserves all existing semantics with a minimal blast radius.
- `numflow-core` is untouched; the new `RoutedPointer` implements the existing `PointerBackend`
  trait and composes the helper transport with the existing direct `WindowsPointer` fallback.
- Helper reuses `WindowsPointer` internally, so release/fail-safe semantics (only release buttons
  NumFlow itself injected) are shared, not duplicated.
- Diagnostics: `RoutedPointer` mirrors helper-held button state into the existing
  `mouse_hold_active()` static so hook/lifecycle logs stay truthful.

## IPC / security model (CONFIRMED)

- Transport: per-session named pipe `\\.\pipe\numflow-input-{session}`, `PIPE_REJECT_REMOTE_CLIENTS`,
  `FILE_FLAG_FIRST_PIPE_INSTANCE` (also serves as the helper single-instance guard), default DACL of
  the helper token (same interactive user + SYSTEM + Administrators; no remote access).
- Framing: fixed 10-byte header (magic `0x4E46_4950`, protocol version, command, payload length) +
  payload capped at 32 bytes; unknown magic/version/command or wrong payload length → reject/close.
- Whitelisted commands only: Handshake, Ping, PointerMove, PointerButton (down/up), Click,
  DoubleClick, ReleaseAll, Shutdown. Every request gets a typed Ack. No shell/exec/file/registry
  surface, no arbitrary API.
- Ownership/authentication, both directions: after connect, each peer resolves the other side's PID
  via `GetNamedPipeClientProcessId`/`GetNamedPipeServerProcessId`, opens the process with
  `PROCESS_QUERY_LIMITED_INFORMATION`, and requires `QueryFullProcessImageNameW` to resolve to
  `numflow.exe` / `numflow-input.exe` **in the same directory as its own executable**. The PID is also
  carried inside the handshake frame and cross-checked against the pipe-reported PID. Documented
  honestly: peer pinning is hygiene; the DACL is the boundary; same-user same-directory compromise is
  out of scope (Program Files is admin-writable only).
- The helper reports its actual UIAccess token state (`current_process_ui_access`) in the handshake;
  the app logs it. Injection behavior never branches on the flag — the SendInput result decides, so a
  portable/dev helper that is not honored degrades exactly like today instead of pretending.

## Tasks

- [ ] FS-UI-1 protocol module (pure, unit-tested)
- [ ] FS-UI-2 pipe transport + client + server + handshake (numflow-windows)
- [ ] FS-UI-3 `RoutedPointer` backend with direct fallback + cooldown + fail-safe release
- [ ] FS-UI-4 helper crate `numflow-input` with uiAccess manifest
- [ ] FS-UI-5 runtime/config/app integration (schema v1 preserved, `input_helper` default true)
- [ ] FS-UI-6 WiX component + release pipeline (MSI + portable + verification lists)
- [ ] FS-UI-7 docs: README, DEVELOPMENT, PLATFORM_BACKENDS, RELEASE_CHECKLIST, INSTALLATION, RELEASING
- [ ] FS-UI-8 quality gate + regression tests


## Baseline

- Rust 2024 workspace, Windows-first, Slint UI, `WH_KEYBOARD_LL` global hook.
- Quality gate: `cargo fmt --all -- --check`, `cargo clippy --locked --workspace --all-targets
  --all-features -- -D warnings`, `cargo test --locked --workspace --all-features`,
  `cargo build --locked --workspace --release --all-features`.
- Already-optimized areas (do not re-open without new evidence): idle worker blocking instead of
  polling, event-driven Slint bridge, bounded event queues, non-blocking hook delivery.

## Confirmed findings

### FS-1 — Continuous slider controls performed a full durable write per pointer move (CONFIRMED → FIXED)

Evidence chain:

1. `ui/design-system.slint:342-352` — `SliderControl` forwards `Slider.changed` to `root.changed`.
2. `i-slint-compiler-1.17.1/widgets/common/slider-base.slint:60-68,117-124` — while dragging,
   `TouchArea.moved` calls `set-value()`, which calls `root.changed(root.value)` on **every pointer
   move**. `released(value)` fires once per gesture.
3. `ui/app.slint:610`, `ui/app.slint:742` — re-emit `speed-changed`, `acceleration-changed`,
   `sound-volume-changed` per change.
4. `src/app.rs` handlers called `persist_configuration()` per change.
5. `persist_configuration` (`src/app.rs:381-387`) → `ConfigStore::save` (`src/config.rs:498-515`:
   `toml::to_string_pretty` + atomic temp write + **`sync_all()` fsync** + rename) **and**
   `sync_secure_desktop_settings` → `SecureSettings::store_for_locked_desktop`
   (`crates/numflow-windows/src/assistive_technology.rs:131-159`: ~25 `RegSetValueExW` writes with
   the schema marker cleared first) plus `assistive_technology_registered()`
   (`assistive_technology.rs:213-221`: 3 HKLM reads).

Impact: one slider drag = hundreds of fsync + registry operations on the UI thread (UI jank, disk
churn). Severity: Medium (performance, no correctness/security impact).

Precedent in-repo: `on_sound_volume_changed` already guards redundant writes by quantizing to the
persisted `u8` percent (`src/app.rs`), i.e. avoiding per-move durability is the existing intent.
Speed/acceleration store `f64`, so a redundancy guard cannot help; coalescing is required.

Fix (implemented): `ConfigPersistence::defer()` keeps in-memory settings, the background runtime, and
the HUD immediate, and defers only the durable write; the settle timer (400 ms) is restarted on each
change; `ConfigPersistence::flush()` writes the last pending change after the Slint event loop exits.
Discrete controls (button, precision, profile, HUD/sound toggles, bindings, resets, tray, runtime
events) keep writing immediately, so `enabled`/`selected_button`/`precision` in the secure-desktop
snapshot are never left pending.

Status: FIXED. Verification: `cargo fmt`, `cargo clippy`, `cargo test` (see Verification).

## Open candidates (not changed — separate decision required)

- FS-2 (low, startup): `HudController::new()` creates the Slint HUD window even when the HUD is
  disabled or NumFlow starts with `--background` (`src/app.rs` `run`). Lazy creation would cut
  startup window cost but changes HUD lifecycle structure.
- FS-3 (low): `handle_desktop_guard` polls `current_thread_owns_input_desktop()` every 100 ms on the
  runtime worker (`DesktopKind`/desktop switch Win32 calls) although desktop-switch and session
  notifications already exist. Safety net value likely outweighs ~30 syscalls/s.
- FS-4 (low): `keyboard_hook_proc` `eprintln!` on the drop path (`hook.rs`) formats on the hook
  thread; only reachable when dispatch fails.

## Rejected hypotheses

- Hook callback locking: `dispatch_event` uses `OnceLock` + `try_lock` and a bounded non-blocking
  queue; `NUM_LOCK_STATE_ORDER`/`LIFECYCLE_EVENT_ORDER` are `try_lock`-only on the hook path. No
  blocking work found. Rejected.
- Motion engine math: `normalized_vector` iterates 8 directions with one `hypot` per 8 ms tick.
  Negligible. Rejected.
- Idle runtime polling: worker blocks on channels while idle; `crossbeam_channel::tick` senders park
  when the receiver is not drained. Rejected.

## Verification log

| Check | Command | Result |
| --- | --- | --- |
| Formatting | `cargo fmt --all -- --check` | PASS (exit 0, no diff) |
| Lint | `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings` | PASS (finished 11.88s, exit 0) |
| Tests | `cargo test --locked --workspace --all-features` | PASS — 46 root unit tests (`app::tests::continuous_control_changes_are_deferred_until_they_settle`, `app::tests::flushing_without_a_pending_change_does_not_write_the_configuration`, `app::tests::pending_configuration_write_is_consumed_exactly_once` all ok), 42 `numflow-windows`, 12 `core_behavior`, 6 `windows_keyboard`, 1 ignored interactive hook smoke test, exit 0 |
| Release build | `cargo build --locked --workspace --release --all-features` | PASS (`Finished release profile [optimized] in 4m 12s`, exit 0) |

Lint failures found and fixed during verification: two `clippy::doc_markdown` findings (`NumFlow` requires
backticks in doc comments), plus one `cargo fmt` normalization of the inserted test helper.

Not executed (environment limits): real slider-drag interaction, MSI-installed `assistive_technology_registered()`
registry path, and manual Windows release checklist items. The deferred-write path is covered by unit tests and
static evidence only; interactive end-to-end drag validation remains manual.