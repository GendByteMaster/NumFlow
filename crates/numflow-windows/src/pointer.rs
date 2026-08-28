use std::mem::size_of;

use numflow_core::{MouseButton, PointerBackend};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEINPUT, SendInput,
};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PointerError {
    #[error(
        "SendInput inserted {inserted} of {expected} mouse events; Windows UIPI may block input into higher-integrity applications"
    )]
    InjectionIncomplete { expected: u32, inserted: u32 },
    #[error("cannot click {button:?} while NumFlow is tracking that button as held")]
    ButtonAlreadyHeld { button: MouseButton },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct PressedButtons(u8);

impl PressedButtons {
    const LEFT: u8 = 1 << 0;
    const RIGHT: u8 = 1 << 1;
    const MIDDLE: u8 = 1 << 2;

    const fn bit(button: MouseButton) -> u8 {
        match button {
            MouseButton::Left => Self::LEFT,
            MouseButton::Right => Self::RIGHT,
            MouseButton::Middle => Self::MIDDLE,
        }
    }

    const fn contains(self, button: MouseButton) -> bool {
        self.0 & Self::bit(button) != 0
    }

    fn insert(&mut self, button: MouseButton) {
        self.0 |= Self::bit(button);
    }

    fn remove(&mut self, button: MouseButton) {
        self.0 &= !Self::bit(button);
    }

    fn clear(&mut self) {
        self.0 = 0;
    }
}

#[derive(Debug, Default)]
pub struct WindowsPointer {
    pressed: PressedButtons,
}

impl WindowsPointer {
    #[must_use]
    pub const fn is_button_held(&self, button: MouseButton) -> bool {
        self.pressed.contains(button)
    }
}

impl PointerBackend for WindowsPointer {
    type Error = PointerError;

    fn move_relative(&mut self, dx: i32, dy: i32) -> Result<(), Self::Error> {
        if dx == 0 && dy == 0 {
            return Ok(());
        }

        send_inputs(&[mouse_input(dx, dy, MOUSEEVENTF_MOVE)])
    }

    fn button_down(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if self.pressed.contains(button) {
            return Ok(());
        }

        send_inputs(&[mouse_input(0, 0, button_down_flag(button))])?;
        self.pressed.insert(button);
        Ok(())
    }

    fn button_up(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if !self.pressed.contains(button) {
            return Ok(());
        }

        send_inputs(&[mouse_input(0, 0, button_up_flag(button))])?;
        self.pressed.remove(button);
        Ok(())
    }

    fn click(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.ensure_clickable(button)?;
        send_inputs(&click_inputs(button))
    }

    fn double_click(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        self.ensure_clickable(button)?;
        send_inputs(&double_click_inputs(button))
    }

    fn release_all(&mut self) -> Result<(), Self::Error> {
        let inputs = release_inputs(self.pressed);
        if inputs.is_empty() {
            return Ok(());
        }

        send_inputs(&inputs)?;
        self.pressed.clear();
        Ok(())
    }
}

impl WindowsPointer {
    fn ensure_clickable(&self, button: MouseButton) -> Result<(), PointerError> {
        if self.pressed.contains(button) {
            return Err(PointerError::ButtonAlreadyHeld { button });
        }
        Ok(())
    }
}

impl Drop for WindowsPointer {
    fn drop(&mut self) {
        let _ = self.release_all();
    }
}

fn send_inputs(inputs: &[INPUT]) -> Result<(), PointerError> {
    if inputs.is_empty() {
        return Ok(());
    }

    let expected = u32::try_from(inputs.len()).expect("mouse input batch length fits in u32");
    let input_size = i32::try_from(size_of::<INPUT>()).expect("INPUT size fits in i32");
    let inserted = unsafe { SendInput(inputs, input_size) };

    if inserted == expected {
        Ok(())
    } else {
        if let Some(target) = crate::foreground_process_info() {
            eprintln!(
                "NumFlow: SendInput incomplete (inserted={inserted}, expected={expected}, foreground={}, pid={}, integrity={}, elevated={:?}); Windows UIPI may block input",
                target.process_name,
                target.process_id,
                target.integrity.unwrap_or("unknown"),
                target.elevated
            );
        } else {
            eprintln!(
                "NumFlow: SendInput incomplete (inserted={inserted}, expected={expected}); foreground process could not be diagnosed"
            );
        }
        Err(PointerError::InjectionIncomplete { expected, inserted })
    }
}

fn click_inputs(button: MouseButton) -> [INPUT; 2] {
    [
        mouse_input(0, 0, button_down_flag(button)),
        mouse_input(0, 0, button_up_flag(button)),
    ]
}

fn double_click_inputs(button: MouseButton) -> [INPUT; 4] {
    let [down, up] = click_inputs(button);
    [down, up, down, up]
}

fn release_inputs(pressed: PressedButtons) -> Vec<INPUT> {
    [MouseButton::Left, MouseButton::Right, MouseButton::Middle]
        .into_iter()
        .filter(|button| pressed.contains(*button))
        .map(|button| mouse_input(0, 0, button_up_flag(button)))
        .collect()
}

const fn button_down_flag(button: MouseButton) -> MOUSE_EVENT_FLAGS {
    match button {
        MouseButton::Left => MOUSEEVENTF_LEFTDOWN,
        MouseButton::Right => MOUSEEVENTF_RIGHTDOWN,
        MouseButton::Middle => MOUSEEVENTF_MIDDLEDOWN,
    }
}

const fn button_up_flag(button: MouseButton) -> MOUSE_EVENT_FLAGS {
    match button {
        MouseButton::Left => MOUSEEVENTF_LEFTUP,
        MouseButton::Right => MOUSEEVENTF_RIGHTUP,
        MouseButton::Middle => MOUSEEVENTF_MIDDLEUP,
    }
}

const fn mouse_input(dx: i32, dy: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use numflow_core::MouseButton;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
        MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    };

    use super::{PressedButtons, click_inputs, double_click_inputs, mouse_input, release_inputs};

    fn mouse_fields(input: INPUT) -> (i32, i32, u32) {
        let mouse = unsafe { input.Anonymous.mi };
        (mouse.dx, mouse.dy, mouse.dwFlags.0)
    }

    #[test]
    fn relative_move_uses_relative_mouse_event() {
        let input = mouse_input(12, -7, MOUSEEVENTF_MOVE);
        assert_eq!(mouse_fields(input), (12, -7, MOUSEEVENTF_MOVE.0));
    }

    #[test]
    fn click_sequences_use_matching_button_flags() {
        let cases = [
            (
                MouseButton::Left,
                MOUSEEVENTF_LEFTDOWN.0,
                MOUSEEVENTF_LEFTUP.0,
            ),
            (
                MouseButton::Right,
                MOUSEEVENTF_RIGHTDOWN.0,
                MOUSEEVENTF_RIGHTUP.0,
            ),
            (
                MouseButton::Middle,
                MOUSEEVENTF_MIDDLEDOWN.0,
                MOUSEEVENTF_MIDDLEUP.0,
            ),
        ];

        for (button, expected_down, expected_up) in cases {
            let [down, up] = click_inputs(button);
            assert_eq!(mouse_fields(down).2, expected_down);
            assert_eq!(mouse_fields(up).2, expected_up);
        }
    }

    #[test]
    fn double_click_is_two_serial_clicks() {
        let inputs = double_click_inputs(MouseButton::Left);
        let flags = inputs.map(|input| mouse_fields(input).2);

        assert_eq!(
            flags,
            [
                MOUSEEVENTF_LEFTDOWN.0,
                MOUSEEVENTF_LEFTUP.0,
                MOUSEEVENTF_LEFTDOWN.0,
                MOUSEEVENTF_LEFTUP.0,
            ]
        );
    }

    #[test]
    fn safe_release_only_targets_buttons_tracked_as_held() {
        let mut pressed = PressedButtons::default();
        pressed.insert(MouseButton::Left);
        pressed.insert(MouseButton::Middle);

        let inputs = release_inputs(pressed);
        let flags = inputs
            .into_iter()
            .map(|input| mouse_fields(input).2)
            .collect::<Vec<_>>();

        assert_eq!(flags, vec![MOUSEEVENTF_LEFTUP.0, MOUSEEVENTF_MIDDLEUP.0]);
    }

    #[test]
    fn pressed_button_tracking_is_idempotent() {
        let mut pressed = PressedButtons::default();
        pressed.insert(MouseButton::Right);
        pressed.insert(MouseButton::Right);
        assert!(pressed.contains(MouseButton::Right));

        pressed.remove(MouseButton::Right);
        pressed.remove(MouseButton::Right);
        assert!(!pressed.contains(MouseButton::Right));
    }
}
