from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    if text.count(old) != 1:
        raise SystemExit(f"{label}: expected exactly one marker, found {text.count(old)}")
    return text.replace(old, new, 1)


hook_path = Path("crates/numflow-windows/src/hook.rs")
hook = hook_path.read_text(encoding="utf-8")

hook = replace_once(
    hook,
    "const NUMFLOW_NUM_LOCK_INJECTION_TAG: usize = 0x4E46_4E4C;\n",
    "const NUMFLOW_NUM_LOCK_INJECTION_TAG: usize = 0x4E46_4E4C;\nconst NUM_LOCK_SCAN_CODE: u16 = 0x45;\n",
    "scan code constant",
)

hook = replace_once(
    hook,
    "static NUM_LOCK_KEY_DOWN: AtomicBool = AtomicBool::new(false);\nstatic NUM_LOCK_REPLAY_FALLBACK: AtomicBool = AtomicBool::new(false);\n",
    "static NUM_LOCK_KEY_DOWN: AtomicBool = AtomicBool::new(false);\n",
    "remove replay fallback state",
)

hook = replace_once(
    hook,
    "    NumLockChanged { num_lock_on: bool },\n",
    "    NumLockChanged {\n        num_lock_on: bool,\n        sync_system: bool,\n    },\n",
    "NumLockChanged payload",
)

hook = replace_once(
    hook,
    "    pub fn set_interception_enabled(&self, enabled: bool) {\n        let should_intercept = enabled && !self.num_lock_on();\n        INTERCEPTION_ENABLED.store(should_intercept, Ordering::Release);\n    }\n\n    pub fn emergency_disable(&self) {\n",
    "    pub fn set_interception_enabled(&self, enabled: bool) {\n        let should_intercept = enabled && !self.num_lock_on();\n        INTERCEPTION_ENABLED.store(should_intercept, Ordering::Release);\n    }\n\n    /// Replays the already-intercepted physical Num Lock toggle after the low-level hook callback\n    /// has returned. Keeping input injection out of `keyboard_hook_proc` avoids re-entrant keyboard\n    /// state changes while Windows is still processing the physical key-down event.\n    #[must_use]\n    pub fn sync_num_lock_to_windows(&self) -> bool {\n        replay_num_lock_to_windows()\n    }\n\n    pub fn emergency_disable(&self) {\n",
    "deferred replay method",
)

hook = replace_once(
    hook,
    "    NUM_LOCK_ON.store(key_state & 1 != 0, Ordering::Release);\n    NUM_LOCK_KEY_DOWN.store(key_state < 0, Ordering::Release);\n    NUM_LOCK_REPLAY_FALLBACK.store(false, Ordering::Release);\n",
    "    NUM_LOCK_ON.store(key_state & 1 != 0, Ordering::Release);\n    NUM_LOCK_KEY_DOWN.store(key_state < 0, Ordering::Release);\n",
    "startup fallback reset",
)

hook = replace_once(
    hook,
    "    INTERCEPTION_ENABLED.store(false, Ordering::Release);\n    NUM_LOCK_REPLAY_FALLBACK.store(false, Ordering::Release);\n    clear_dispatcher();\n",
    "    INTERCEPTION_ENABLED.store(false, Ordering::Release);\n    clear_dispatcher();\n",
    "shutdown fallback reset",
)

hook = replace_once(
    hook,
    "    if changed == Some(true) {\n        // Num Lock ON means normal number entry. Stop interception in the hook immediately;\n        // the runtime will safely release any held pointer state as it consumes the mode event.\n        INTERCEPTION_ENABLED.store(false, Ordering::Release);\n    }\n",
    "    if changed == Some(false) {\n        // Num Lock OFF means pointer control. Start interception immediately in the hook so a\n        // NumPad key pressed directly after Num Lock cannot leak through before the runtime wakes.\n        INTERCEPTION_ENABLED.store(true, Ordering::Release);\n    } else if changed == Some(true) {\n        // Num Lock ON means normal number entry. Stop interception immediately; the runtime will\n        // release any held pointer state and then synchronize the Windows lock state.\n        INTERCEPTION_ENABLED.store(false, Ordering::Release);\n    }\n",
    "immediate interception transition",
)

hook = replace_once(
    hook,
    "fn dispatch_num_lock_change(state: KeyState) -> Option<bool> {\n    let changed = observe_num_lock(state);\n    if let Some(num_lock_on) = changed {\n        let _ = dispatch_event(KeyboardHookEvent::NumLockChanged { num_lock_on }, true);\n    }\n    changed\n}\n",
    "fn dispatch_num_lock_change(state: KeyState, sync_system: bool) -> Option<bool> {\n    let changed = observe_num_lock(state);\n    if let Some(num_lock_on) = changed {\n        let _ = dispatch_event(\n            KeyboardHookEvent::NumLockChanged {\n                num_lock_on,\n                sync_system,\n            },\n            true,\n        );\n    }\n    changed\n}\n",
    "dispatch Num Lock source",
)

hook = replace_once(
    hook,
    "                wVk: VK_NUMLOCK,\n                wScan: 0,\n",
    "                wVk: VK_NUMLOCK,\n                wScan: NUM_LOCK_SCAN_CODE,\n",
    "Num Lock scan code",
)

old_intercept = '''fn intercept_physical_num_lock(state: KeyState) -> bool {\n    let fallback = NUM_LOCK_REPLAY_FALLBACK.load(Ordering::Acquire);\n    let changed = dispatch_num_lock_change(state);\n\n    if fallback {\n        if state == KeyState::Released {\n            NUM_LOCK_REPLAY_FALLBACK.store(false, Ordering::Release);\n        }\n        return false;\n    }\n\n    if changed.is_some() && state == KeyState::Pressed && !replay_num_lock_to_windows() {\n        // If SendInput cannot replay the Num Lock press, pass this physical key sequence through\n        // until release. That keeps Windows' toggle state and LED synchronized instead of leaving\n        // NumFlow and the OS in different modes.\n        NUM_LOCK_REPLAY_FALLBACK.store(true, Ordering::Release);\n        return false;\n    }\n\n    true\n}\n'''
new_intercept = '''fn intercept_physical_num_lock(state: KeyState) -> bool {\n    // Always consume the physical Num Lock sequence. The runtime performs the tagged Windows\n    // replay after this low-level hook callback returns, avoiding re-entrant SendInput here.\n    let _ = dispatch_num_lock_change(state, true);\n    true\n}\n'''
hook = replace_once(hook, old_intercept, new_intercept, "physical Num Lock interception")

hook = replace_once(
    hook,
    "                    let _ = dispatch_num_lock_change(state);\n",
    "                    let _ = dispatch_num_lock_change(state, false);\n",
    "external injected Num Lock",
)

hook = replace_once(
    hook,
    "    use super::{NUMFLOW_NUM_LOCK_INJECTION_TAG, num_lock_replay_inputs, num_lock_transition};\n",
    "    use super::{\n        NUMFLOW_NUM_LOCK_INJECTION_TAG, NUM_LOCK_SCAN_CODE, num_lock_replay_inputs,\n        num_lock_transition,\n    };\n",
    "test imports",
)

hook = replace_once(
    hook,
    "        assert_eq!(down.wVk, VK_NUMLOCK);\n        assert_eq!(up.wVk, VK_NUMLOCK);\n        assert_eq!(down.dwFlags, KEYEVENTF_EXTENDEDKEY);\n",
    "        assert_eq!(down.wVk, VK_NUMLOCK);\n        assert_eq!(up.wVk, VK_NUMLOCK);\n        assert_eq!(down.wScan, NUM_LOCK_SCAN_CODE);\n        assert_eq!(up.wScan, NUM_LOCK_SCAN_CODE);\n        assert_eq!(down.dwFlags, KEYEVENTF_EXTENDEDKEY);\n",
    "replay scan-code test",
)

hook_path.write_text(hook, encoding="utf-8")

runtime_path = Path("src/runtime.rs")
runtime = runtime_path.read_text(encoding="utf-8")

runtime = replace_once(
    runtime,
    "            KeyboardHookEvent::NumLockChanged { num_lock_on } => {\n                normalizer.reset();\n",
    "            KeyboardHookEvent::NumLockChanged {\n                num_lock_on,\n                sync_system,\n            } => {\n                normalizer.reset();\n",
    "runtime NumLockChanged pattern",
)

runtime = replace_once(
    runtime,
    "                match apply_num_lock_mode(machine, num_lock_on) {\n                    Ok(effects) => {\n                        hook.set_interception_enabled(machine.enabled());\n",
    "                match apply_num_lock_mode(machine, num_lock_on) {\n                    Ok(effects) => {\n                        if sync_system && !hook.sync_num_lock_to_windows() {\n                            tracing::warn!(\n                                num_lock_on,\n                                \"failed to replay intercepted Num Lock toggle to Windows\"\n                            );\n                        }\n                        hook.set_interception_enabled(machine.enabled());\n",
    "deferred system replay",
)

runtime_path.write_text(runtime, encoding="utf-8")
