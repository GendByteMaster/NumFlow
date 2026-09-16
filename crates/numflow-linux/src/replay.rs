use std::io;

use evdev::{AttributeSetRef, InputEvent, KeyCode, uinput::VirtualDevice};

use crate::{LinuxInputError, REPLAY_DEVICE_NAME};

trait ReplayWriter {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()>;
}

impl ReplayWriter for VirtualDevice {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        VirtualDevice::emit(self, events)
    }
}

struct ReplayEmitter<W> {
    writer: W,
}

impl<W: ReplayWriter> ReplayEmitter<W> {
    const fn new(writer: W) -> Self {
        Self { writer }
    }

    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        self.writer.emit(events)
    }
}

pub struct ReplayKeyboard {
    emitter: ReplayEmitter<VirtualDevice>,
}

impl ReplayKeyboard {
    /// Creates the uinput keyboard used to replay non-NumFlow key events.
    ///
    /// # Errors
    ///
    /// Returns an error when `/dev/uinput` cannot be opened or the virtual keyboard cannot be
    /// configured and registered.
    pub fn create(keys: &AttributeSetRef<KeyCode>) -> Result<Self, LinuxInputError> {
        let builder = VirtualDevice::builder().map_err(LinuxInputError::UinputOpen)?;
        let device = builder
            .name(REPLAY_DEVICE_NAME)
            .with_keys(keys)
            .map_err(LinuxInputError::VirtualDevice)?
            .build()
            .map_err(LinuxInputError::VirtualDevice)?;

        Ok(Self {
            emitter: ReplayEmitter::new(device),
        })
    }

    /// Replays a batch without inspecting or logging key content.
    ///
    /// # Errors
    ///
    /// Returns an error when the virtual keyboard cannot emit the complete batch.
    pub fn emit(&mut self, events: &[InputEvent]) -> Result<(), LinuxInputError> {
        self.emitter
            .emit(events)
            .map_err(LinuxInputError::VirtualDevice)
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, io, rc::Rc};

    use evdev::{EventType, InputEvent, KeyCode};

    use super::{ReplayEmitter, ReplayWriter};

    #[derive(Clone, Default)]
    struct RecordingWriter {
        batches: Rc<RefCell<Vec<Vec<InputEvent>>>>,
    }

    impl ReplayWriter for RecordingWriter {
        fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
            self.batches.borrow_mut().push(events.to_vec());
            Ok(())
        }
    }

    #[test]
    fn replay_forwards_event_batch_without_rewriting_key_content() {
        let writer = RecordingWriter::default();
        let batches = Rc::clone(&writer.batches);
        let mut replay = ReplayEmitter::new(writer);
        let input = [
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_A.0, 1),
            InputEvent::new(EventType::KEY.0, KeyCode::KEY_A.0, 0),
        ];

        replay.emit(&input).expect("replay should emit");

        let batches = batches.borrow();
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0], input);
    }
}
