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
