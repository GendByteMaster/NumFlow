from pathlib import Path


def replace_once(text: str, old: str, new: str, label: str) -> str:
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected exactly one marker, found {count}")
    return text.replace(old, new, 1)


cargo_path = Path("crates/numflow-windows/Cargo.toml")
cargo = cargo_path.read_text(encoding="utf-8")
cargo = replace_once(
    cargo,
    '    "Win32_UI_Input_KeyboardAndMouse",\n    "Win32_UI_WindowsAndMessaging",\n',
    '    "Win32_UI_Input",\n    "Win32_UI_Input_KeyboardAndMouse",\n    "Win32_UI_WindowsAndMessaging",\n',
    "windows raw-input feature",
)
cargo_path.write_text(cargo, encoding="utf-8")


hook_path = Path("crates/numflow-windows/src/hook.rs")
hook = hook_path.read_text(encoding="utf-8")
hook = replace_once(
    hook,
    '''        UI::{\n            Input::KeyboardAndMouse::{\n                GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,\n                KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VK_NUMLOCK,\n            },\n            WindowsAndMessaging::{\n''',
    '''        UI::{\n            Input::{\n                KeyboardAndMouse::{\n                    GetKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT,\n                    KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, SendInput, VK_NUMLOCK,\n                },\n                RAWINPUTDEVICE, RIDEV_REMOVE, RegisterRawInputDevices,\n            },\n            WindowsAndMessaging::{\n''',
    "raw-input imports",
)
hook = replace_once(
    hook,
    '''const NUMFLOW_NUM_LOCK_INJECTION_TAG: usize = 0x4E46_4E4C;\nconst NUM_LOCK_SCAN_CODE: u16 = 0x45;\n''',
    '''const NUMFLOW_NUM_LOCK_INJECTION_TAG: usize = 0x4E46_4E4C;\nconst NUM_LOCK_SCAN_CODE: u16 = 0x45;\nconst HID_USAGE_PAGE_GENERIC: u16 = 0x01;\nconst HID_USAGE_GENERIC_KEYBOARD: u16 = 0x06;\n''',
    "raw keyboard HID constants",
)
hook = replace_once(
    hook,
    '''#[derive(Debug)]\npub struct KeyboardHook {\n''',
    '''fn raw_keyboard_removal_device() -> RAWINPUTDEVICE {\n    RAWINPUTDEVICE {\n        usUsagePage: HID_USAGE_PAGE_GENERIC,\n        usUsage: HID_USAGE_GENERIC_KEYBOARD,\n        dwFlags: RIDEV_REMOVE,\n        ..RAWINPUTDEVICE::default()\n    }\n}\n\n/// Removes the process-wide raw-keyboard device-event registration installed by winit.\n///\n/// Winit registers keyboards for raw `DeviceEvent` delivery on Windows. Windows can then stop\n/// dispatching this process's `WH_KEYBOARD_LL` hook while one of the same process's windows owns\n/// foreground focus. `NumFlow` does not consume winit raw `DeviceEvent::Key` events; Slint's normal\n/// window keyboard handling continues through `WM_KEYDOWN` / `WM_KEYUP`. Removing only the raw\n/// keyboard registration therefore keeps the UI keyboard-accessible while restoring `NumFlow`'s\n/// global low-level hook inside its own focused settings window. Raw mouse registration is left\n/// untouched.\n///\n/// This function is intentionally idempotent and should run after Slint/winit has initialized its\n/// event loop.\n///\n/// # Errors\n///\n/// Returns the Win32 error from `RegisterRawInputDevices` if Windows rejects the removal request.\n///\n/// # Panics\n///\n/// Panics only if the compile-time `RAWINPUTDEVICE` size cannot fit in a Win32 `UINT`.\npub fn remove_raw_keyboard_device_event_registration() -> Result<(), WindowsError> {\n    let device = raw_keyboard_removal_device();\n    let device_size = u32::try_from(size_of::<RAWINPUTDEVICE>())\n        .expect("RAWINPUTDEVICE size must fit in a Win32 UINT");\n\n    unsafe { RegisterRawInputDevices(&[device], device_size) }\n}\n\n#[derive(Debug)]\npub struct KeyboardHook {\n''',
    "raw keyboard compatibility function",
)
hook = replace_once(
    hook,
    '''    use windows::Win32::UI::Input::KeyboardAndMouse::{\n        INPUT_KEYBOARD, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_NUMLOCK,\n    };\n\n    use super::{\n        NUM_LOCK_SCAN_CODE, NUMFLOW_NUM_LOCK_INJECTION_TAG, infer_num_lock_from_numpad,\n        num_lock_replay_inputs, num_lock_transition,\n    };\n''',
    '''    use windows::Win32::UI::Input::{\n        KeyboardAndMouse::{INPUT_KEYBOARD, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP, VK_NUMLOCK},\n        RIDEV_REMOVE,\n    };\n\n    use super::{\n        HID_USAGE_GENERIC_KEYBOARD, HID_USAGE_PAGE_GENERIC, NUM_LOCK_SCAN_CODE,\n        NUMFLOW_NUM_LOCK_INJECTION_TAG, infer_num_lock_from_numpad, num_lock_replay_inputs,\n        num_lock_transition, raw_keyboard_removal_device,\n    };\n''',
    "hook test imports",
)
hook = replace_once(
    hook,
    '''    #[test]\n    fn num_lock_toggles_once_per_physical_press() {\n''',
    '''    #[test]\n    fn raw_keyboard_removal_descriptor_does_not_touch_mouse_registration() {\n        let device = raw_keyboard_removal_device();\n\n        assert_eq!(device.usUsagePage, HID_USAGE_PAGE_GENERIC);\n        assert_eq!(device.usUsage, HID_USAGE_GENERIC_KEYBOARD);\n        assert_eq!(device.dwFlags, RIDEV_REMOVE);\n    }\n\n    #[test]\n    fn num_lock_toggles_once_per_physical_press() {\n''',
    "raw keyboard compatibility test",
)
hook_path.write_text(hook, encoding="utf-8")


lib_path = Path("crates/numflow-windows/src/lib.rs")
lib = lib_path.read_text(encoding="utf-8")
lib = replace_once(
    lib,
    '''#[cfg(windows)]\npub use hook::{HookError, KeyboardHook, KeyboardHookEvent};\n''',
    '''#[cfg(windows)]\npub use hook::{\n    HookError, KeyboardHook, KeyboardHookEvent, remove_raw_keyboard_device_event_registration,\n};\n''',
    "hook compatibility export",
)
lib_path.write_text(lib, encoding="utf-8")


app_path = Path("src/app.rs")
app = app_path.read_text(encoding="utf-8")
app = replace_once(
    app,
    '''    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;\n    sync_window_from_settings(&window, &settings.borrow());\n''',
    '''    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;\n\n    #[cfg(windows)]\n    if let Err(error) = numflow_windows::remove_raw_keyboard_device_event_registration() {\n        tracing::warn!(\n            %error,\n            "failed to remove winit raw-keyboard registration; foreground NumPad interception may be unavailable"\n        );\n    } else {\n        tracing::debug!(\n            "removed winit raw-keyboard registration for foreground WH_KEYBOARD_LL compatibility"\n        );\n    }\n\n    sync_window_from_settings(&window, &settings.borrow());\n''',
    "post-window raw-keyboard compatibility",
)
app_path.write_text(app, encoding="utf-8")
