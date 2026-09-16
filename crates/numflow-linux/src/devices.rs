use std::path::PathBuf;

use evdev::{AttributeSet, AttributeSetRef, KeyCode};

pub const REPLAY_DEVICE_NAME: &str = "NumFlow Virtual Keyboard";
pub const POINTER_DEVICE_NAME: &str = "NumFlow Virtual Mouse";
pub const VIRTUAL_DEVICE_PREFIX: &str = "NumFlow Virtual ";

const REQUIRED_NUMFLOW_KEYS: [KeyCode; 16] = [
    KeyCode::KEY_NUMLOCK,
    KeyCode::KEY_KP0,
    KeyCode::KEY_KP1,
    KeyCode::KEY_KP2,
    KeyCode::KEY_KP3,
    KeyCode::KEY_KP4,
    KeyCode::KEY_KP5,
    KeyCode::KEY_KP6,
    KeyCode::KEY_KP7,
    KeyCode::KEY_KP8,
    KeyCode::KEY_KP9,
    KeyCode::KEY_KPPLUS,
    KeyCode::KEY_KPDOT,
    KeyCode::KEY_KPSLASH,
    KeyCode::KEY_KPASTERISK,
    KeyCode::KEY_KPMINUS,
];

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

fn is_keyboard_candidate(name: Option<&str>, supported_keys: &AttributeSetRef<KeyCode>) -> bool {
    if name.is_some_and(|name| name.starts_with(VIRTUAL_DEVICE_PREFIX)) {
        return false;
    }

    REQUIRED_NUMFLOW_KEYS
        .iter()
        .all(|key| supported_keys.contains(*key))
}

#[must_use]
pub fn discover_keyboards() -> Vec<KeyboardCandidate> {
    let mut candidates = evdev::enumerate()
        .filter_map(|(path, device)| {
            let supported_keys = device.supported_keys()?;
            if !is_keyboard_candidate(device.name(), supported_keys) {
                return None;
            }

            Some(KeyboardCandidate {
                identity: DeviceIdentity {
                    path,
                    name: device.name().map(str::to_owned),
                    physical_path: device.physical_path().map(str::to_owned),
                },
                supported_keys: supported_keys.iter().collect(),
            })
        })
        .collect::<Vec<_>>();

    candidates.sort_unstable_by(|left, right| left.identity.path.cmp(&right.identity.path));
    candidates
}

#[must_use]
pub fn union_supported_keys(candidates: &[KeyboardCandidate]) -> AttributeSet<KeyCode> {
    candidates
        .iter()
        .flat_map(|candidate| candidate.supported_keys.iter())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use evdev::{AttributeSet, KeyCode};

    use super::{
        DeviceIdentity, KeyboardCandidate, VIRTUAL_DEVICE_PREFIX, is_keyboard_candidate,
        union_supported_keys,
    };

    fn required_keys() -> AttributeSet<KeyCode> {
        let mut keys = AttributeSet::new();
        for key in [
            KeyCode::KEY_NUMLOCK,
            KeyCode::KEY_KP0,
            KeyCode::KEY_KP1,
            KeyCode::KEY_KP2,
            KeyCode::KEY_KP3,
            KeyCode::KEY_KP4,
            KeyCode::KEY_KP5,
            KeyCode::KEY_KP6,
            KeyCode::KEY_KP7,
            KeyCode::KEY_KP8,
            KeyCode::KEY_KP9,
            KeyCode::KEY_KPPLUS,
            KeyCode::KEY_KPDOT,
            KeyCode::KEY_KPSLASH,
            KeyCode::KEY_KPASTERISK,
            KeyCode::KEY_KPMINUS,
        ] {
            keys.insert(key);
        }
        keys
    }

    fn candidate(path: &str, extra: KeyCode) -> KeyboardCandidate {
        let mut supported_keys = required_keys();
        supported_keys.insert(extra);
        KeyboardCandidate {
            identity: DeviceIdentity {
                path: PathBuf::from(path),
                name: Some("Physical Keyboard".to_owned()),
                physical_path: Some("usb-test/input0".to_owned()),
            },
            supported_keys,
        }
    }

    #[test]
    fn candidate_requires_num_lock_and_every_numflow_keypad_key() {
        let keys = required_keys();
        assert!(is_keyboard_candidate(Some("Keyboard"), &keys));

        let mut missing_num_lock = keys.clone();
        missing_num_lock.remove(KeyCode::KEY_NUMLOCK);
        assert!(!is_keyboard_candidate(Some("Keyboard"), &missing_num_lock));

        let mut missing_kp8 = keys;
        missing_kp8.remove(KeyCode::KEY_KP8);
        assert!(!is_keyboard_candidate(Some("Keyboard"), &missing_kp8));
    }

    #[test]
    fn numflow_virtual_devices_are_excluded_from_discovery() {
        let keys = required_keys();
        assert!(!is_keyboard_candidate(
            Some(&format!("{VIRTUAL_DEVICE_PREFIX}Keyboard")),
            &keys
        ));
    }

    #[test]
    fn supported_key_union_contains_capabilities_from_all_candidates() {
        let candidates = [
            candidate("/dev/input/event2", KeyCode::KEY_A),
            candidate("/dev/input/event4", KeyCode::KEY_VOLUMEUP),
        ];

        let union = union_supported_keys(&candidates);

        assert!(union.contains(KeyCode::KEY_A));
        assert!(union.contains(KeyCode::KEY_VOLUMEUP));
        assert!(union.contains(KeyCode::KEY_KP8));
    }
}
