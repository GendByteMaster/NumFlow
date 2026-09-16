#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use evdev::AttributeSet;

    use crate::{DeviceIdentity, KeyboardCandidate, LinuxInputError};

    use super::CapturedKeyboard;

    fn missing_candidate() -> KeyboardCandidate {
        KeyboardCandidate {
            identity: DeviceIdentity {
                path: PathBuf::from("/dev/input/numflow-test-missing-device"),
                name: Some("NumFlow Test Keyboard".to_owned()),
                physical_path: Some("numflow-test/input0".to_owned()),
            },
            supported_keys: AttributeSet::new(),
        }
    }

    #[test]
    fn missing_input_device_fails_before_exclusive_grab() {
        let expected_path = missing_candidate().identity.path;
        let result = CapturedKeyboard::grab(missing_candidate());

        match result {
            Err(LinuxInputError::InputRead { path, .. }) => assert_eq!(path, expected_path),
            Err(other) => panic!("unexpected error: {other}"),
            Ok(_) => panic!("missing input device must not be captured"),
        }
    }
}
