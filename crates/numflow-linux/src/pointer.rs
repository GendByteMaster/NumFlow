#[cfg(test)]
mod tests {
    use std::{cell::RefCell, io, rc::Rc};

    use evdev::{EventType, InputEvent, KeyCode, RelativeAxisCode};
    use numflow_core::MouseButton;

    use super::{EventWriter, PointerState};

    #[derive(Clone, Default)]
    struct RecordingWriter {
        batches: Rc<RefCell<Vec<Vec<InputEvent>>>>,
    }

    impl EventWriter for RecordingWriter {
        fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
            self.batches.borrow_mut().push(events.to_vec());
            Ok(())
        }
    }

    fn raw(event: &InputEvent) -> (u16, u16, i32) {
        (event.event_type().0, event.code(), event.value())
    }

    #[test]
    fn relative_move_emits_x_and_y_in_one_batch() {
        let writer = RecordingWriter::default();
        let batches = Rc::clone(&writer.batches);
        let mut pointer = PointerState::new(writer);

        pointer.move_relative(12, -7).expect("move should emit");

        let batches = batches.borrow();
        assert_eq!(batches.len(), 1);
        assert_eq!(
            batches[0].iter().map(raw).collect::<Vec<_>>(),
            vec![
                (EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, 12),
                (EventType::RELATIVE.0, RelativeAxisCode::REL_Y.0, -7),
            ]
        );
    }

    #[test]
    fn left_button_down_and_up_emit_key_events() {
        let writer = RecordingWriter::default();
        let batches = Rc::clone(&writer.batches);
        let mut pointer = PointerState::new(writer);

        pointer
            .button_down(MouseButton::Left)
            .expect("button down should emit");
        pointer
            .button_up(MouseButton::Left)
            .expect("button up should emit");

        let batches = batches.borrow();
        assert_eq!(
            raw(&batches[0][0]),
            (EventType::KEY.0, KeyCode::BTN_LEFT.0, 1)
        );
        assert_eq!(
            raw(&batches[1][0]),
            (EventType::KEY.0, KeyCode::BTN_LEFT.0, 0)
        );
    }

    #[test]
    fn release_all_only_releases_buttons_still_tracked_as_held() {
        let writer = RecordingWriter::default();
        let batches = Rc::clone(&writer.batches);
        let mut pointer = PointerState::new(writer);

        pointer
            .button_down(MouseButton::Left)
            .expect("left down should emit");
        pointer
            .button_down(MouseButton::Right)
            .expect("right down should emit");
        pointer
            .button_up(MouseButton::Right)
            .expect("right up should emit");
        pointer.release_all().expect("release all should emit");
        pointer
            .release_all()
            .expect("second release all should be idempotent");

        let batches = batches.borrow();
        assert_eq!(batches.len(), 4);
        assert_eq!(
            batches[3].iter().map(raw).collect::<Vec<_>>(),
            vec![(EventType::KEY.0, KeyCode::BTN_LEFT.0, 0)]
        );
    }
}
