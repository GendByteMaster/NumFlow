use std::io;

use evdev::{AttributeSet, EventType, InputEvent, KeyCode, RelativeAxisCode, uinput::VirtualDevice};
use numflow_core::{MouseButton, PointerBackend};

use crate::{LinuxInputError, POINTER_DEVICE_NAME};

trait EventWriter {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()>;
}

impl EventWriter for VirtualDevice {
    fn emit(&mut self, events: &[InputEvent]) -> io::Result<()> {
        VirtualDevice::emit(self, events)
    }
}

struct PointerState<W> {
    writer: W,
    held: [bool; 3],
}

impl<W: EventWriter> PointerState<W> {
    const fn new(writer: W) -> Self {
        Self {
            writer,
            held: [false; 3],
        }
    }

    fn move_relative(&mut self, dx: i32, dy: i32) -> io::Result<()> {
        let mut events = Vec::with_capacity(2);
        if dx != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_X.0,
                dx,
            ));
        }
        if dy != 0 {
            events.push(InputEvent::new(
                EventType::RELATIVE.0,
                RelativeAxisCode::REL_Y.0,
                dy,
            ));
        }
        if events.is_empty() {
            return Ok(());
        }
        self.writer.emit(&events)
    }

    fn button_down(&mut self, button: MouseButton) -> io::Result<()> {
        let index = button_index(button);
        if self.held[index] {
            return Ok(());
        }

        self.writer.emit(&[button_event(button, 1)])?;
        self.held[index] = true;
        Ok(())
    }

    fn button_up(&mut self, button: MouseButton) -> io::Result<()> {
        let index = button_index(button);
        if !self.held[index] {
            return Ok(());
        }

        self.writer.emit(&[button_event(button, 0)])?;
        self.held[index] = false;
        Ok(())
    }

    fn click(&mut self, button: MouseButton) -> io::Result<()> {
        self.writer
            .emit(&[button_event(button, 1), button_event(button, 0)])
    }

    fn double_click(&mut self, button: MouseButton) -> io::Result<()> {
        self.writer.emit(&[
            button_event(button, 1),
            button_event(button, 0),
            button_event(button, 1),
            button_event(button, 0),
        ])
    }

    fn release_all(&mut self) -> io::Result<()> {
        let mut events = Vec::with_capacity(3);
        for button in [MouseButton::Left, MouseButton::Right, MouseButton::Middle] {
            if self.held[button_index(button)] {
                events.push(button_event(button, 0));
            }
        }

        if events.is_empty() {
            return Ok(());
        }

        self.writer.emit(&events)?;
        self.held = [false; 3];
        Ok(())
    }
}

const fn button_index(button: MouseButton) -> usize {
    match button {
        MouseButton::Left => 0,
        MouseButton::Right => 1,
        MouseButton::Middle => 2,
    }
}

const fn button_code(button: MouseButton) -> KeyCode {
    match button {
        MouseButton::Left => KeyCode::BTN_LEFT,
        MouseButton::Right => KeyCode::BTN_RIGHT,
        MouseButton::Middle => KeyCode::BTN_MIDDLE,
    }
}

fn button_event(button: MouseButton, value: i32) -> InputEvent {
    InputEvent::new(EventType::KEY.0, button_code(button).0, value)
}

pub struct LinuxPointer {
    state: PointerState<VirtualDevice>,
}

impl LinuxPointer {
    /// Creates the uinput relative mouse used by the Linux backend.
    ///
    /// # Errors
    ///
    /// Returns an error when `/dev/uinput` cannot be opened or the virtual pointer cannot be
    /// configured and registered.
    pub fn create() -> Result<Self, LinuxInputError> {
        let mut axes = AttributeSet::<RelativeAxisCode>::new();
        axes.insert(RelativeAxisCode::REL_X);
        axes.insert(RelativeAxisCode::REL_Y);

        let mut buttons = AttributeSet::<KeyCode>::new();
        buttons.insert(KeyCode::BTN_LEFT);
        buttons.insert(KeyCode::BTN_RIGHT);
        buttons.insert(KeyCode::BTN_MIDDLE);

        let builder = VirtualDevice::builder().map_err(LinuxInputError::UinputOpen)?;
        let device = builder
            .name(POINTER_DEVICE_NAME)
            .with_keys(&buttons)
            .map_err(LinuxInputError::VirtualDevice)?
            .with_relative_axes(&axes)
            .map_err(LinuxInputError::VirtualDevice)?
            .build()
            .map_err(LinuxInputError::VirtualDevice)?;

        Ok(Self {
            state: PointerState::new(device),
        })
    }
}

impl PointerBackend for LinuxPointer {
    type Error = LinuxInputError;

    fn move_relative(&mut self, dx: i32, dy: i32) -> Result<(), Self::Error> {
        self.state
            .move_relative(dx, dy)
            .map_err(LinuxInputError::VirtualDevice)
    }

    fn button_down(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.state
            .button_down(button)
            .map_err(LinuxInputError::VirtualDevice)
    }

    fn button_up(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.state
            .button_up(button)
            .map_err(LinuxInputError::VirtualDevice)
    }

    fn click(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.state
            .click(button)
            .map_err(LinuxInputError::VirtualDevice)
    }

    fn double_click(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.state
            .double_click(button)
            .map_err(LinuxInputError::VirtualDevice)
    }

    fn release_all(&mut self) -> Result<(), Self::Error> {
        self.state
            .release_all()
            .map_err(LinuxInputError::VirtualDevice)
    }
}

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
