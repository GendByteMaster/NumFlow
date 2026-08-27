from pathlib import Path

hook_path = Path('crates/numflow-windows/src/hook.rs')
runtime_path = Path('src/runtime.rs')

hook = hook_path.read_text(encoding='utf-8')
runtime = runtime_path.read_text(encoding='utf-8')

old_enum = '''    NumLockChanged {
        num_lock_on: bool,
        sync_system: bool,
    },'''
new_enum = '''    NumLockChanged {
        num_lock_on: bool,
        sync_system: bool,
        play_feedback: bool,
    },'''
assert old_enum in hook
hook = hook.replace(old_enum, new_enum, 1)

old_dispatch = '''fn dispatch_num_lock_change(state: KeyState, sync_system: bool) -> Option<bool> {
    let changed = observe_num_lock(state);
    if let Some(num_lock_on) = changed {
        let _ = dispatch_event(
            KeyboardHookEvent::NumLockChanged {
                num_lock_on,
                sync_system,
            },
            true,
        );
    }
    changed
}
'''
new_dispatch = '''fn dispatch_num_lock_change(
    state: KeyState,
    sync_system: bool,
    play_feedback: bool,
) -> Option<bool> {
    let changed = observe_num_lock(state);
    if let Some(num_lock_on) = changed {
        let _ = dispatch_event(
            KeyboardHookEvent::NumLockChanged {
                num_lock_on,
                sync_system,
                play_feedback,
            },
            true,
        );
    }
    changed
}

fn infer_num_lock_from_numpad(event: PhysicalKeyEvent) -> Option<bool> {
    if event.extended {
        return None;
    }

    match (event.scan_code, event.vk_code) {
        // Num Lock ON: Windows reports the physical digit keys as VK_NUMPAD0..VK_NUMPAD9.
        (0x52, 0x60)
        | (0x4F, 0x61)
        | (0x50, 0x62)
        | (0x51, 0x63)
        | (0x4B, 0x64)
        | (0x4C, 0x65)
        | (0x4D, 0x66)
        | (0x47, 0x67)
        | (0x48, 0x68)
        | (0x49, 0x69)
        | (0x53, 0x6E) => Some(true),

        // Num Lock OFF: the same physical scan codes are reported as navigation keys.
        (0x52, 0x2D) // Insert
        | (0x4F, 0x23) // End
        | (0x50, 0x28) // Down
        | (0x51, 0x22) // Page Down
        | (0x4B, 0x25) // Left
        | (0x4C, 0x0C) // Clear
        | (0x4D, 0x27) // Right
        | (0x47, 0x24) // Home
        | (0x48, 0x26) // Up
        | (0x49, 0x21) // Page Up
        | (0x53, 0x2E) => Some(false), // Delete
        _ => None,
    }
}

fn reconcile_num_lock_from_numpad(event: PhysicalKeyEvent) {
    let Some(observed_num_lock_on) = infer_num_lock_from_numpad(event) else {
        return;
    };

    let previous = NUM_LOCK_ON.swap(observed_num_lock_on, Ordering::AcqRel);
    if previous == observed_num_lock_on {
        return;
    }

    // GetKeyState is thread-message-queue based and can be stale on a newly-created background
    // hook thread. A physical NumPad event carries the actual Windows interpretation, so use it to
    // repair startup state before deciding whether this same event should be intercepted.
    INTERCEPTION_ENABLED.store(!observed_num_lock_on, Ordering::Release);
    let _ = dispatch_event(
        KeyboardHookEvent::NumLockChanged {
            num_lock_on: observed_num_lock_on,
            sync_system: false,
            play_feedback: false,
        },
        true,
    );
}
'''
assert old_dispatch in hook
hook = hook.replace(old_dispatch, new_dispatch, 1)

hook = hook.replace('dispatch_num_lock_change(state, false)', 'dispatch_num_lock_change(state, false, true)')
hook = hook.replace('dispatch_num_lock_change(state, true)', 'dispatch_num_lock_change(state, true, true)')

old_gate = '''            if INTERCEPTION_ENABLED.load(Ordering::Acquire) && !NUM_LOCK_ON.load(Ordering::Acquire)
            {
                let event = PhysicalKeyEvent::new(
                    keyboard.vkCode,
                    keyboard.scanCode,
                    keyboard.flags.0 & LLKHF_EXTENDED.0 != 0,
                    state,
                );

                if map_numpad_key(event).is_some()
                    && dispatch_event(KeyboardHookEvent::Key(event), false)
                {
                    return LRESULT(1);
                }
            }
'''
new_gate = '''            let event = PhysicalKeyEvent::new(
                keyboard.vkCode,
                keyboard.scanCode,
                keyboard.flags.0 & LLKHF_EXTENDED.0 != 0,
                state,
            );

            if map_numpad_key(event).is_some() {
                reconcile_num_lock_from_numpad(event);

                if INTERCEPTION_ENABLED.load(Ordering::Acquire)
                    && !NUM_LOCK_ON.load(Ordering::Acquire)
                    && dispatch_event(KeyboardHookEvent::Key(event), false)
                {
                    return LRESULT(1);
                }
            }
'''
assert old_gate in hook
hook = hook.replace(old_gate, new_gate, 1)

old_test_use = '''    use super::{
        NUM_LOCK_SCAN_CODE, NUMFLOW_NUM_LOCK_INJECTION_TAG, num_lock_replay_inputs,
        num_lock_transition,
    };
    use crate::KeyState;
'''
new_test_use = '''    use super::{
        NUM_LOCK_SCAN_CODE, NUMFLOW_NUM_LOCK_INJECTION_TAG, infer_num_lock_from_numpad,
        num_lock_replay_inputs, num_lock_transition,
    };
    use crate::{KeyState, PhysicalKeyEvent};
'''
assert old_test_use in hook
hook = hook.replace(old_test_use, new_test_use, 1)

insert_before = '''    #[test]
    fn num_lock_replay_is_tagged_keyboard_input() {
'''
new_tests = '''    #[test]
    fn infers_num_lock_on_from_physical_numpad_digit_semantics() {
        for (scan_code, vk_code) in [
            (0x52, 0x60),
            (0x4F, 0x61),
            (0x50, 0x62),
            (0x51, 0x63),
            (0x4B, 0x64),
            (0x4C, 0x65),
            (0x4D, 0x66),
            (0x47, 0x67),
            (0x48, 0x68),
            (0x49, 0x69),
            (0x53, 0x6E),
        ] {
            let event = PhysicalKeyEvent::new(vk_code, scan_code, false, KeyState::Pressed);
            assert_eq!(infer_num_lock_from_numpad(event), Some(true));
        }
    }

    #[test]
    fn infers_num_lock_off_from_physical_numpad_navigation_semantics() {
        for (scan_code, vk_code) in [
            (0x52, 0x2D),
            (0x4F, 0x23),
            (0x50, 0x28),
            (0x51, 0x22),
            (0x4B, 0x25),
            (0x4C, 0x0C),
            (0x4D, 0x27),
            (0x47, 0x24),
            (0x48, 0x26),
            (0x49, 0x21),
            (0x53, 0x2E),
        ] {
            let event = PhysicalKeyEvent::new(vk_code, scan_code, false, KeyState::Pressed);
            assert_eq!(infer_num_lock_from_numpad(event), Some(false));
        }
    }

    #[test]
    fn does_not_infer_num_lock_from_operator_or_extended_keys() {
        let add = PhysicalKeyEvent::new(0x6B, 0x4E, false, KeyState::Pressed);
        let navigation_cluster = PhysicalKeyEvent::new(0x28, 0x50, true, KeyState::Pressed);
        assert_eq!(infer_num_lock_from_numpad(add), None);
        assert_eq!(infer_num_lock_from_numpad(navigation_cluster), None);
    }

'''
assert insert_before in hook
hook = hook.replace(insert_before, new_tests + insert_before, 1)

old_runtime_match = '''            KeyboardHookEvent::NumLockChanged {
                num_lock_on,
                sync_system,
            } => {
                normalizer.reset();
                if let Some(audio_feedback) = audio_feedback {
                    audio_feedback.play(if num_lock_on {
                        AudioCue::NumFlowOff
                    } else {
                        AudioCue::NumFlowOn
                    });
                }
'''
new_runtime_match = '''            KeyboardHookEvent::NumLockChanged {
                num_lock_on,
                sync_system,
                play_feedback,
            } => {
                normalizer.reset();
                if play_feedback && let Some(audio_feedback) = audio_feedback {
                    audio_feedback.play(if num_lock_on {
                        AudioCue::NumFlowOff
                    } else {
                        AudioCue::NumFlowOn
                    });
                }
'''
assert old_runtime_match in runtime
runtime = runtime.replace(old_runtime_match, new_runtime_match, 1)

hook_path.write_text(hook, encoding='utf-8')
runtime_path.write_text(runtime, encoding='utf-8')
