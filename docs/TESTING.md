# NumFlow testing architecture

NumFlow keeps tests at the narrowest boundary that can verify the behavior without weakening
production encapsulation.

## Test layers

```text
tests/
├── common/mod.rs          shared public keyboard-event fixtures
├── core_behavior.rs       portable black-box tests for numflow-core
├── windows_keyboard.rs    Windows-only black-box mapping/normalizer contracts
└── windows_system.rs      explicit interactive-desktop smoke tests

src/** and crates/**/src/**
                           private algorithm, Win32, UI, queue, and runtime tests
```

The root `tests/` files exercise public APIs from outside their defining module. They must not
require production items to become `pub` solely for testing. `tests/common` contains only reusable
fixtures and mocks; it is not a separate Cargo test target.

Inline tests remain appropriate when they inspect private implementation details, for example:

- `RuntimeMachine` and `RuntimeEventSink` fail-safe/queue behavior;
- Win32 hook message ordering, session recovery phases, Num Lock replay structures, and hook
  retirement helpers;
- `SendInput` structure construction and private pressed-button tracking;
- config/UI/HUD presentation helpers and private serialization fixtures.

## Environment classes

- Portable: `tests/core_behavior.rs` is deterministic and does not require Windows input or a
  desktop.
- Windows-only: `tests/windows_keyboard.rs` is compiled only on Windows and exercises public
  keyboard contracts without installing a global hook or injecting input.
- Interactive Windows desktop: `tests/windows_system.rs` contains an ignored global-hook singleton
  smoke test. Run it deliberately with `cargo test --test windows_system -- --ignored`, with no
  running NumFlow instance. It is excluded from normal CI because hook installation changes the
  desktop input environment.
- Manual release validation: physical keyboard reconnect, Task Manager/elevation, lock/unlock,
  Sleep/Resume, LED synchronization, and real pointer injection cannot be made deterministic in a
  hosted unit-test process. These scenarios are recorded in `WINDOWS_RESUME_RECOVERY.md` and
  `RELEASE_CHECKLIST.md`.

No test relies on polling or arbitrary retry sleeps. Time-based core tests use explicit durations
passed to the motion API; Windows lifecycle behavior is tested through event/state decisions and
manual evidence where the operating system owns the transition.

## Quality gate

The existing CI commands automatically discover root integration tests:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release --all-features
```

The default suite includes all portable, Windows-only, and private deterministic tests. The
interactive smoke test is reported as ignored rather than silently pretending to validate a real
desktop hook.

## Manual regression matrix

Record the runtime log and the observed Num Lock LED/mode/HUD state for each case:

1. ordinary application → NumPad;
2. Task Manager → NumPad;
3. ordinary application → Task Manager → ordinary application;
4. Windows Lock → Unlock → NumPad;
5. Sleep/Hibernate → Resume → NumPad;
6. several physical Num Lock toggles;
7. keyboard disconnect/reconnect;
8. NumPad movement key held across Lock/Resume;
9. mouse hold active across Lock/Resume;
10. elevated and non-elevated foreground applications.

Do not attempt to control the Windows lock screen or Secure Desktop from a normal user process.
The expected result is recovery immediately after the interactive user desktop returns.
