#![cfg(windows)]

use numflow_windows::{HookError, KeyboardHook};

/// Requires an interactive user desktop and must be run with `cargo test -- --ignored`.
/// It is deliberately not part of the default CI suite because installing a global hook is an
/// external side effect and cannot be made deterministic in a shared desktop session.
#[test]
#[ignore = "requires an interactive Windows desktop and no running NumFlow instance"]
fn keyboard_hook_is_singleton_and_reports_liveness() {
    let (hook, _events) = KeyboardHook::start_with_capacity(2).expect("hook should start");
    assert!(hook.hook_alive());
    assert!(hook.resync_input_state(numflow_windows::InputResyncReason::Startup));

    let second = KeyboardHook::start_with_capacity(2);
    assert!(matches!(second, Err(HookError::AlreadyActive)));
    assert!(
        hook.hook_alive(),
        "a rejected duplicate must not retire the active hook"
    );

    hook.stop().expect("hook should stop cleanly");
}
