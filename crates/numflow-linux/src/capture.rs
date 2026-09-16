use std::path::PathBuf;

use evdev::{Device, InputEvent};

use crate::{KeyboardCandidate, LinuxInputError};

pub struct CapturedKeyboard {
    device: Device,
    path: PathBuf,
    grabbed: bool,
}

impl CapturedKeyboard {
    /// Opens and exclusively grabs one physical keyboard candidate.
    ///
    /// Replay and pointer output devices must already exist before this method is called so that a
    /// later runtime can fail open without leaving the physical keyboard captured.
    ///
    /// # Errors
    ///
    /// Returns [`LinuxInputError::InputRead`] when the input device cannot be opened and
    /// [`LinuxInputError::InputGrab`] when exclusive capture cannot be acquired.
    pub fn grab(candidate: KeyboardCandidate) -> Result<Self, LinuxInputError> {
        let path = candidate.identity.path;
        let mut device = Device::open(&path).map_err(|source| LinuxInputError::InputRead {
            path: path.clone(),
            source,
        })?;

        device.grab().map_err(|source| LinuxInputError::InputGrab {
            path: path.clone(),
            source,
        })?;

        Ok(Self {
            device,
            path,
            grabbed: true,
        })
    }

    /// Reads the next available input packet from the captured device without logging its content.
    ///
    /// # Errors
    ///
    /// Returns [`LinuxInputError::InputRead`] when evdev cannot fetch the next packet.
    pub fn fetch_events(&mut self) -> Result<Vec<InputEvent>, LinuxInputError> {
        self.device
            .fetch_events()
            .map(std::iter::Iterator::collect)
            .map_err(|source| LinuxInputError::InputRead {
                path: self.path.clone(),
                source,
            })
    }

    /// Releases exclusive ownership. Calling this more than once is a no-op.
    ///
    /// # Errors
    ///
    /// Returns [`LinuxInputError::InputUngrab`] when evdev cannot release the grab.
    pub fn ungrab(&mut self) -> Result<(), LinuxInputError> {
        if !self.grabbed {
            return Ok(());
        }

        self.device
            .ungrab()
            .map_err(|source| LinuxInputError::InputUngrab {
                path: self.path.clone(),
                source,
            })?;
        self.grabbed = false;
        Ok(())
    }
}

impl Drop for CapturedKeyboard {
    fn drop(&mut self) {
        if self.grabbed {
            let _ = self.device.ungrab();
            self.grabbed = false;
        }
    }
}

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
