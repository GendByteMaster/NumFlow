# Repository Guidelines

## Project Structure & Module Organization

NumFlow is a Rust 2024 workspace targeting Windows with Slint for the UI.

- `src/` contains the application layer: configuration, runtime orchestration, HUD, and UI bindings.
- `crates/numflow-core/` contains platform-independent bindings, state, motion, and pointer-effect logic.
- `crates/numflow-windows/` contains Win32 hooks, key normalization, `SendInput`, audio, startup, HUD, and single-instance integration.
- `ui/` contains Slint views and design tokens; `assets/` contains icons and sound effects.
- `docs/` contains development, installation, release, and validation guidance; `installer/` contains WiX packaging.
- Tests are primarily inline Rust unit tests under `src/` and crate modules; there is no separate top-level test directory.

## Build, Test, and Development Commands

Use pinned Rust 1.98 and preserve the lockfile.

```powershell
cargo run --locked
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release --all-features
```

The first command runs the app; the rest are the formatting, lint, test, and release-build quality gate used by GitHub Actions.

## Coding Style & Naming Conventions

Run `cargo fmt` before committing. Follow idiomatic Rust: `snake_case` for functions/modules, `PascalCase` for types, and `SCREAMING_SNAKE_CASE` for constants. Keep `unsafe` Win32 work isolated in `numflow-windows`; the root crate denies unsafe code. Keep keyboard-hook callbacks short and non-blocking, and use bounded queues for runtime/event delivery.

## Testing Guidelines

Add focused `#[test]` cases beside the code they exercise, with behavior-oriented names. Cover state transitions, binding/config round trips, motion math, Num Lock replay, input normalization, queue behavior, and fail-safe release paths. No coverage threshold is documented. Real Windows input, DPI, resume, and accessibility behavior requires manual validation in `docs/RELEASE_CHECKLIST.md`.

## Commit & Pull Request Guidelines

Use concise conventional prefixes with an optional scope, for example `fix(windows): ...`, `fix(ui): ...`, `docs: ...`, or `chore(ci): ...`. Normal work belongs on `dev/master`; do not commit directly to `master`.

PRs should target `dev/master`, explain the behavior and safety impact, list validation commands, and update relevant documentation. Include screenshots or short manual-test notes for UI or Windows-only changes, and link the related issue or roadmap item when applicable. Keep `Cargo.lock` changes deliberate.

## Configuration & Safety

User settings live at `%APPDATA%\NumFlow\config.toml` and must flow through the typed, versioned configuration model with validation, safe-default recovery, and atomic writes. Preserve the invariants that Num Lock remains synchronized, injected input cannot recursively toggle mode, and disabling or shutting down always releases held mouse buttons.

<!-- forgeguard:managed-start -->
## ForgeGuard

- Before substantial ambiguous work, use ForgeGuard Goal Intelligence to define measurable success when needed.
- Before repository implementation, bug fixes, refactors, migrations, security-sensitive changes, production changes, or commit preparation, load and apply the `engineering-guardrails` skill.
- Follow ForgeGuard's Risk Gate and explicit subagent approval gate.
- Keep repository-local instructions authoritative within their scope.
- If the skill cannot be loaded, report that explicitly and continue with the repository's existing instructions; do not invent missing ForgeGuard policy.
<!-- forgeguard:managed-end -->
