#![cfg(windows)]

mod common;

use common::{key_event, numpad_event};
use numflow_core::{Bindings, Direction, InputAction, NumpadKey};
use numflow_windows::{
    InputResyncReason, InputRuntimeState, KeyState, KeyboardEventNormalizer, map_numpad_key,
};

#[test]
fn every_supported_numpad_scan_code_maps_to_its_public_key() {
    let expected = [
        (0x52, false, NumpadKey::Num0),
        (0x4F, false, NumpadKey::Num1),
        (0x50, false, NumpadKey::Num2),
        (0x51, false, NumpadKey::Num3),
        (0x4B, false, NumpadKey::Num4),
        (0x4C, false, NumpadKey::Num5),
        (0x4D, false, NumpadKey::Num6),
        (0x47, false, NumpadKey::Num7),
        (0x48, false, NumpadKey::Num8),
        (0x49, false, NumpadKey::Num9),
        (0x4E, false, NumpadKey::Add),
        (0x53, false, NumpadKey::Decimal),
        (0x35, true, NumpadKey::Divide),
        (0x37, false, NumpadKey::Multiply),
        (0x4A, false, NumpadKey::Subtract),
    ];

    for (scan_code, extended, key) in expected {
        assert_eq!(
            map_numpad_key(key_event(scan_code, extended, KeyState::Pressed)),
            Some(key)
        );
    }
}

#[test]
fn mapping_rejects_navigation_cluster_and_regular_slash() {
    for scan_code in [0x47, 0x48, 0x49, 0x4B, 0x4D, 0x4F, 0x50, 0x51, 0x52, 0x53] {
        assert_eq!(
            map_numpad_key(key_event(scan_code, true, KeyState::Pressed)),
            None
        );
    }
    assert_eq!(map_numpad_key(numpad_event(0x35, KeyState::Pressed)), None);
    assert_eq!(
        map_numpad_key(key_event(0x35, true, KeyState::Pressed)),
        Some(NumpadKey::Divide)
    );
}

#[test]
fn normalizer_preserves_movement_repeat_but_suppresses_click_repeat() {
    let bindings = Bindings::default();
    let mut normalizer = KeyboardEventNormalizer::default();

    let first_click = normalizer
        .process(numpad_event(0x4C, KeyState::Pressed), &bindings)
        .expect("first Num5 press should be emitted");
    assert_eq!(first_click.action, InputAction::Click);
    assert!(!first_click.repeated);
    assert!(
        normalizer
            .process(numpad_event(0x4C, KeyState::Pressed), &bindings)
            .is_none()
    );
    assert!(
        normalizer
            .process(numpad_event(0x4C, KeyState::Released), &bindings)
            .is_some()
    );

    let first_move = normalizer
        .process(numpad_event(0x48, KeyState::Pressed), &bindings)
        .expect("first Num8 press should be emitted");
    let repeated_move = normalizer
        .process(numpad_event(0x48, KeyState::Pressed), &bindings)
        .expect("movement repeat should be emitted");
    assert_eq!(first_move.action, InputAction::Move(Direction::Up));
    assert!(repeated_move.repeated);
}

#[test]
fn normalizer_reset_clears_pressed_state_and_custom_move_repeats() {
    let mut bindings = Bindings::default();
    bindings.bind(NumpadKey::Num5, InputAction::Move(Direction::Left));
    let mut normalizer = KeyboardEventNormalizer::default();

    normalizer
        .process(numpad_event(0x4C, KeyState::Pressed), &bindings)
        .expect("custom move should be emitted");
    let repeated = normalizer
        .process(numpad_event(0x4C, KeyState::Pressed), &bindings)
        .expect("custom movement should repeat");
    assert!(repeated.repeated);
    assert!(normalizer.is_pressed(NumpadKey::Num5));

    normalizer.reset();
    assert!(!normalizer.is_pressed(NumpadKey::Num5));
}

#[test]
fn input_resync_reasons_have_stable_diagnostic_labels() {
    let reasons = [
        (InputResyncReason::Startup, "startup"),
        (InputResyncReason::ResumeAutomatic, "resume-automatic"),
        (InputResyncReason::ResumeUser, "resume-user"),
        (InputResyncReason::SessionUnlock, "session-unlock"),
        (InputResyncReason::DesktopReady, "desktop-ready"),
        (InputResyncReason::ForegroundChanged, "foreground-changed"),
        (
            InputResyncReason::KeyboardDeviceChanged,
            "keyboard-device-changed",
        ),
        (InputResyncReason::HookFailure, "hook-failure"),
        (InputResyncReason::NumLockChanged, "numlock-changed"),
    ];

    for (reason, label) in reasons {
        assert_eq!(reason.label(), label);
    }
}

#[test]
fn input_runtime_state_exposes_running_suspended_and_recovering_modes() {
    assert_ne!(InputRuntimeState::Running, InputRuntimeState::Suspended);
    assert_ne!(InputRuntimeState::Suspended, InputRuntimeState::Recovering);
    assert_ne!(InputRuntimeState::Recovering, InputRuntimeState::Running);
}
