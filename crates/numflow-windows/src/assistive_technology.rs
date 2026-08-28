use std::{ffi::c_void, mem::size_of};

use numflow_core::{Bindings, Direction, InputAction, MotionConfig, MouseButton, NumpadKey};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, WIN32_ERROR},
        System::Registry::{
            HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_DWORD, RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
            RegGetValueW, RegSetKeyValueW,
        },
        UI::Input::KeyboardAndMouse::{
            INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP,
            SendInput, VIRTUAL_KEY, VK_LWIN,
        },
    },
    core::PCWSTR,
};

pub const AT_KEY_NAME: &str = "GendByteMaster_NumFlow_v1";
pub const SECURE_AT_KEY_NAME: &str = "GendByteMaster_NumFlowSecure_v1";

const AT_REGISTRY_ROOT: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion\Accessibility\ATs";
const AT_CONFIG_ROOT: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Accessibility\ATConfig";
const SETTINGS_SCHEMA_VERSION: u32 = 1;
const SCALE: f64 = 1_000.0;
const ACCESSIBILITY_TEMP: &str = r"Software\Microsoft\Windows NT\CurrentVersion\AccessibilityTemp";
const AT_SESSION_STARTING: u32 = 0x0003;
const AT_SESSION_EXITING: u32 = 0x0002;

const KEYS: [NumpadKey; 15] = [
    NumpadKey::Num0,
    NumpadKey::Num1,
    NumpadKey::Num2,
    NumpadKey::Num3,
    NumpadKey::Num4,
    NumpadKey::Num5,
    NumpadKey::Num6,
    NumpadKey::Num7,
    NumpadKey::Num8,
    NumpadKey::Num9,
    NumpadKey::Add,
    NumpadKey::Decimal,
    NumpadKey::Divide,
    NumpadKey::Multiply,
    NumpadKey::Subtract,
];

#[derive(Debug, Clone, PartialEq)]
pub struct SecureSettings {
    pub enabled: bool,
    pub motion: MotionConfig,
    pub selected_button: MouseButton,
    pub precision: bool,
    pub bindings: Bindings,
}

impl Default for SecureSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            motion: MotionConfig::default(),
            selected_button: MouseButton::Left,
            precision: false,
            bindings: Bindings::default(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SecureSettingsError {
    #[error("Windows registry operation for accessibility setting {value} failed: {status:?}")]
    Registry {
        value: &'static str,
        status: WIN32_ERROR,
    },
    #[error("accessibility settings contain an invalid value for {0}")]
    Invalid(&'static str),
}

#[derive(Debug, thiserror::Error)]
pub enum AssistiveTechnologySessionError {
    #[error("failed to notify Ease of Access through the accessibility session registry: {0:?}")]
    Registry(WIN32_ERROR),
    #[error("Windows accepted {inserted} of {expected} Ease of Access notification inputs")]
    NotificationInput { inserted: u32, expected: u32 },
}

/// Notifies Ease of Access about a directly-started, non-job `NumFlow` AT session.
///
/// Microsoft requires this handshake when `TerminateOnDesktopSwitch=0` and an AT can be launched
/// outside Ease of Access. Unregistered development and portable builds perform no notification.
#[derive(Debug)]
pub struct AssistiveTechnologySession {
    active: bool,
}

impl AssistiveTechnologySession {
    /// Starts the documented `AccessibilityTemp` plus Windows+U handshake when registered.
    ///
    /// # Errors
    ///
    /// Returns a registry or `SendInput` failure. The caller may continue as an ordinary runtime,
    /// but Windows is then not expected to launch the secure accommodation for that session.
    pub fn start() -> Result<Self, AssistiveTechnologySessionError> {
        if !assistive_technology_registered() {
            return Ok(Self { active: false });
        }
        notify_ease_of_access(AT_SESSION_STARTING)?;
        Ok(Self { active: true })
    }
}

impl Drop for AssistiveTechnologySession {
    fn drop(&mut self) {
        if self.active {
            let _ = notify_ease_of_access(AT_SESSION_EXITING);
        }
    }
}

impl SecureSettings {
    /// Writes the strictly bounded settings subset consumed by `numflow-secure.exe`.
    ///
    /// The schema marker is cleared first and committed last. A desktop transition during a
    /// partial update therefore makes the secure runtime fail closed instead of consuming a mixed
    /// settings generation.
    ///
    /// # Errors
    ///
    /// Returns the first per-user registry write failure.
    pub fn store_for_locked_desktop(&self) -> Result<(), SecureSettingsError> {
        let settings = Self {
            motion: self.motion.sanitized(),
            ..self.clone()
        };

        write_dword("SchemaVersion", 0)?;
        write_dword("Enabled", u32::from(settings.enabled))?;
        write_dword("BaseSpeedMilli", scaled(settings.motion.base_speed))?;
        write_dword("MaxSpeedMilli", scaled(settings.motion.max_speed))?;
        write_dword("AccelerationMilli", scaled(settings.motion.acceleration))?;
        write_dword(
            "PrecisionMultiplierMilli",
            scaled(settings.motion.precision_multiplier),
        )?;
        write_dword(
            "BoostMultiplierMilli",
            scaled(settings.motion.boost_multiplier),
        )?;
        write_dword("SelectedButton", encode_button(settings.selected_button))?;
        write_dword("PrecisionEnabled", u32::from(settings.precision))?;

        for key in KEYS {
            let action = settings.bindings.action_for(key).map_or(0, encode_action);
            write_dword(binding_value_name(key), action)?;
        }

        write_dword("SchemaVersion", SETTINGS_SCHEMA_VERSION)
    }

    /// Loads only the documented `ATConfig` DWORD values copied by Ease of Access.
    ///
    /// Missing settings disable `NumFlow`. Invalid or out-of-range values are rejected rather than
    /// interpreted as commands, paths, plug-ins, or executable content.
    ///
    /// # Errors
    ///
    /// Returns malformed-data and registry failures other than a missing value.
    pub fn load_for_current_desktop() -> Result<Self, SecureSettingsError> {
        if read_dword("SchemaVersion")? != Some(SETTINGS_SCHEMA_VERSION) {
            return Ok(Self::default());
        }

        let Some(enabled) = read_dword("Enabled")? else {
            return Ok(Self::default());
        };
        let motion = MotionConfig {
            base_speed: read_scaled("BaseSpeedMilli")?,
            max_speed: read_scaled("MaxSpeedMilli")?,
            acceleration: read_scaled("AccelerationMilli")?,
            precision_multiplier: read_scaled("PrecisionMultiplierMilli")?,
            boost_multiplier: read_scaled("BoostMultiplierMilli")?,
        }
        .sanitized();
        let selected_button = decode_button(required_dword("SelectedButton")?)
            .ok_or(SecureSettingsError::Invalid("SelectedButton"))?;
        let precision = decode_bool(required_dword("PrecisionEnabled")?, "PrecisionEnabled")?;
        let enabled = decode_bool(enabled, "Enabled")?;
        let mut bindings = Bindings::empty();

        for key in KEYS {
            let value_name = binding_value_name(key);
            let code = required_dword(value_name)?;
            if code == 0 {
                continue;
            }
            let action = decode_action(code).ok_or(SecureSettingsError::Invalid(value_name))?;
            bindings.bind(key, action);
        }

        Ok(Self {
            enabled,
            motion,
            selected_button,
            precision,
            bindings,
        })
    }
}

/// Returns whether both production AT entries are present.
#[must_use]
pub fn assistive_technology_registered() -> bool {
    registry_string_exists(HKEY_LOCAL_MACHINE, &at_path(AT_KEY_NAME), "ATExe")
        && registry_string_exists(HKEY_LOCAL_MACHINE, &at_path(SECURE_AT_KEY_NAME), "ATExe")
        && registry_string_exists(
            HKEY_LOCAL_MACHINE,
            &at_path(AT_KEY_NAME),
            "SecureDesktopAccommodation",
        )
}

fn at_path(key: &str) -> String {
    format!(r"{AT_REGISTRY_ROOT}\{key}")
}

fn config_path() -> String {
    format!(r"{AT_CONFIG_ROOT}\{AT_KEY_NAME}")
}

fn registry_string_exists(
    root: windows::Win32::System::Registry::HKEY,
    path: &str,
    name: &str,
) -> bool {
    let path = wide(path);
    let name = wide(name);
    let mut size = 0;
    unsafe {
        RegGetValueW(
            root,
            PCWSTR(path.as_ptr()),
            PCWSTR(name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&raw mut size),
        )
    }
    .is_ok()
}

fn write_dword(name: &'static str, value: u32) -> Result<(), SecureSettingsError> {
    let status = write_registry_dword(&config_path(), name, value);
    if status.is_ok() {
        Ok(())
    } else {
        Err(SecureSettingsError::Registry {
            value: name,
            status,
        })
    }
}

fn write_registry_dword(path: &str, name: &str, value: u32) -> WIN32_ERROR {
    let path = wide(path);
    let name_wide = wide(name);
    let bytes = value.to_le_bytes();
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            PCWSTR(name_wide.as_ptr()),
            REG_DWORD.0,
            Some(bytes.as_ptr().cast()),
            u32::try_from(bytes.len()).expect("DWORD size fits in u32"),
        )
    }
}

fn notify_ease_of_access(state: u32) -> Result<(), AssistiveTechnologySessionError> {
    let status = write_registry_dword(ACCESSIBILITY_TEMP, AT_KEY_NAME, state);
    if !status.is_ok() {
        return Err(AssistiveTechnologySessionError::Registry(status));
    }

    let inputs = accessibility_notification_inputs();
    let expected = u32::try_from(inputs.len()).expect("notification input count fits in u32");
    let input_size = i32::try_from(size_of::<INPUT>()).expect("INPUT size fits in i32");
    let inserted = unsafe { SendInput(&inputs, input_size) };
    if inserted == expected {
        Ok(())
    } else {
        Err(AssistiveTechnologySessionError::NotificationInput { inserted, expected })
    }
}

fn accessibility_notification_inputs() -> [INPUT; 4] {
    [
        keyboard_input(VK_LWIN, false),
        keyboard_input(VIRTUAL_KEY(u16::from(b'U')), false),
        keyboard_input(VIRTUAL_KEY(u16::from(b'U')), true),
        keyboard_input(VK_LWIN, true),
    ]
}

fn keyboard_input(key: VIRTUAL_KEY, released: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                wScan: 0,
                dwFlags: if released {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS::default()
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn read_dword(name: &'static str) -> Result<Option<u32>, SecureSettingsError> {
    let path = wide(&config_path());
    let name_wide = wide(name);
    let mut value = 0_u32;
    let mut size = u32::try_from(size_of::<u32>()).expect("DWORD size fits in u32");
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            PCWSTR(path.as_ptr()),
            PCWSTR(name_wide.as_ptr()),
            RRF_RT_REG_DWORD,
            None,
            Some((&raw mut value).cast::<c_void>()),
            Some(&raw mut size),
        )
    };
    if status.is_ok() {
        Ok(Some(value))
    } else if status == ERROR_FILE_NOT_FOUND || status == ERROR_PATH_NOT_FOUND {
        Ok(None)
    } else {
        Err(SecureSettingsError::Registry {
            value: name,
            status,
        })
    }
}

fn required_dword(name: &'static str) -> Result<u32, SecureSettingsError> {
    read_dword(name)?.ok_or(SecureSettingsError::Invalid(name))
}

fn read_scaled(name: &'static str) -> Result<f64, SecureSettingsError> {
    Ok(f64::from(required_dword(name)?) / SCALE)
}

fn decode_bool(value: u32, name: &'static str) -> Result<bool, SecureSettingsError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(SecureSettingsError::Invalid(name)),
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn scaled(value: f64) -> u32 {
    // All callers pass a sanitized MotionConfig value. Clamp still makes the conversion explicit
    // and total if the settings representation grows independently in the future.
    (value * SCALE).round().clamp(0.0, f64::from(u32::MAX)) as u32
}

const fn encode_button(button: MouseButton) -> u32 {
    match button {
        MouseButton::Left => 1,
        MouseButton::Right => 2,
        MouseButton::Middle => 3,
    }
}

const fn decode_button(value: u32) -> Option<MouseButton> {
    match value {
        1 => Some(MouseButton::Left),
        2 => Some(MouseButton::Right),
        3 => Some(MouseButton::Middle),
        _ => None,
    }
}

const fn encode_action(action: InputAction) -> u32 {
    match action {
        InputAction::Move(Direction::Up) => 1,
        InputAction::Move(Direction::Down) => 2,
        InputAction::Move(Direction::Left) => 3,
        InputAction::Move(Direction::Right) => 4,
        InputAction::Move(Direction::UpLeft) => 5,
        InputAction::Move(Direction::UpRight) => 6,
        InputAction::Move(Direction::DownLeft) => 7,
        InputAction::Move(Direction::DownRight) => 8,
        InputAction::Click => 9,
        InputAction::DoubleClick => 10,
        InputAction::Hold => 11,
        InputAction::Release => 12,
        InputAction::SelectButton(MouseButton::Left) => 13,
        InputAction::SelectButton(MouseButton::Right) => 14,
        InputAction::SelectButton(MouseButton::Middle) => 15,
        InputAction::ToggleEnabled => 16,
        InputAction::SetEnabled(false) => 17,
        InputAction::SetEnabled(true) => 18,
        InputAction::SetPrecision(false) => 19,
        InputAction::SetPrecision(true) => 20,
    }
}

const fn decode_action(value: u32) -> Option<InputAction> {
    Some(match value {
        1 => InputAction::Move(Direction::Up),
        2 => InputAction::Move(Direction::Down),
        3 => InputAction::Move(Direction::Left),
        4 => InputAction::Move(Direction::Right),
        5 => InputAction::Move(Direction::UpLeft),
        6 => InputAction::Move(Direction::UpRight),
        7 => InputAction::Move(Direction::DownLeft),
        8 => InputAction::Move(Direction::DownRight),
        9 => InputAction::Click,
        10 => InputAction::DoubleClick,
        11 => InputAction::Hold,
        12 => InputAction::Release,
        13 => InputAction::SelectButton(MouseButton::Left),
        14 => InputAction::SelectButton(MouseButton::Right),
        15 => InputAction::SelectButton(MouseButton::Middle),
        16 => InputAction::ToggleEnabled,
        17 => InputAction::SetEnabled(false),
        18 => InputAction::SetEnabled(true),
        19 => InputAction::SetPrecision(false),
        20 => InputAction::SetPrecision(true),
        _ => return None,
    })
}

const fn binding_value_name(key: NumpadKey) -> &'static str {
    match key {
        NumpadKey::Num0 => "BindingNum0",
        NumpadKey::Num1 => "BindingNum1",
        NumpadKey::Num2 => "BindingNum2",
        NumpadKey::Num3 => "BindingNum3",
        NumpadKey::Num4 => "BindingNum4",
        NumpadKey::Num5 => "BindingNum5",
        NumpadKey::Num6 => "BindingNum6",
        NumpadKey::Num7 => "BindingNum7",
        NumpadKey::Num8 => "BindingNum8",
        NumpadKey::Num9 => "BindingNum9",
        NumpadKey::Add => "BindingAdd",
        NumpadKey::Decimal => "BindingDecimal",
        NumpadKey::Divide => "BindingDivide",
        NumpadKey::Multiply => "BindingMultiply",
        NumpadKey::Subtract => "BindingSubtract",
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(test)]
mod tests {
    use numflow_core::{Bindings, Direction, InputAction, MouseButton, NumpadKey};

    use windows::Win32::UI::Input::KeyboardAndMouse::{KEYEVENTF_KEYUP, VK_LWIN};

    use super::{
        accessibility_notification_inputs, decode_action, decode_button, encode_action,
        encode_button,
    };

    #[test]
    fn action_codes_round_trip_for_every_supported_binding() {
        let bindings = Bindings::default();
        for (_, action) in bindings.iter() {
            assert_eq!(decode_action(encode_action(action)), Some(action));
        }
        for action in [
            InputAction::Move(Direction::Up),
            InputAction::ToggleEnabled,
            InputAction::SetEnabled(false),
            InputAction::SetEnabled(true),
            InputAction::SetPrecision(false),
            InputAction::SetPrecision(true),
        ] {
            assert_eq!(decode_action(encode_action(action)), Some(action));
        }
        assert_eq!(decode_action(0), None);
        assert_eq!(decode_action(u32::MAX), None);
        assert!(bindings.action_for(NumpadKey::Num8).is_some());
    }

    #[test]
    fn button_codes_reject_untrusted_values() {
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            assert_eq!(decode_button(encode_button(button)), Some(button));
        }
        assert_eq!(decode_button(0), None);
        assert_eq!(decode_button(4), None);
    }

    #[test]
    fn ease_of_access_notification_balances_windows_and_u_keys() {
        let fields = accessibility_notification_inputs().map(|input| unsafe { input.Anonymous.ki });

        assert_eq!(fields[0].wVk, VK_LWIN);
        assert_eq!(fields[1].wVk.0, u16::from(b'U'));
        assert_eq!(fields[0].dwFlags.0, 0);
        assert_eq!(fields[1].dwFlags.0, 0);
        assert_eq!(fields[2].dwFlags.0, KEYEVENTF_KEYUP.0);
        assert_eq!(fields[3].dwFlags.0, KEYEVENTF_KEYUP.0);
    }
}
