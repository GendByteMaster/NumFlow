use std::{
    mem::size_of,
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};

use numflow_core::{MouseButton, PointerBackend};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEINPUT, SendInput,
};

use crate::{
    desktop::{RuntimeKind, current_runtime_kind},
    diagnostics::{current_process_elevated, current_process_ui_access},
    input_protocol::{ButtonAction, Message},
    input_service::{
        HelperClient, InputServiceError, RECONNECT_COOLDOWN, connect_or_spawn_helper,
        reconnect_allowed,
    },
};

static MOUSE_HOLD_ACTIVE: AtomicBool = AtomicBool::new(false);

#[must_use]
pub fn mouse_hold_active() -> bool {
    MOUSE_HOLD_ACTIVE.load(Ordering::Acquire)
}

fn set_mouse_hold_active(active: bool) {
    MOUSE_HOLD_ACTIVE.store(active, Ordering::Release);
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PointerError {
    #[error(
        "SendInput inserted {inserted} of {expected} mouse events; Windows UIPI may block input into higher-integrity applications"
    )]
    InjectionIncomplete { expected: u32, inserted: u32 },
    #[error("cannot click {button:?} while NumFlow is tracking that button as held")]
    ButtonAlreadyHeld { button: MouseButton },
    #[error("UIAccess input helper failed: {reason}")]
    Helper { reason: String },
}

impl From<InputServiceError> for PointerError {
    fn from(error: InputServiceError) -> Self {
        Self::Helper {
            reason: error.to_string(),
        }
    }
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

    const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// Direct `SendInput` implementation used both by `NumFlow` fallback and inside the `UIAccess` helper.
#[derive(Debug, Default)]
pub(crate) struct DirectWindowsPointer {
    pressed: PressedButtons,
}

impl DirectWindowsPointer {
    const fn is_button_held(&self, button: MouseButton) -> bool {
        self.pressed.contains(button)
    }

    const fn has_held_buttons(&self) -> bool {
        !self.pressed.is_empty()
    }

    fn ensure_clickable(&self, button: MouseButton) -> Result<(), PointerError> {
        if self.pressed.contains(button) {
            return Err(PointerError::ButtonAlreadyHeld { button });
        }
        Ok(())
    }
}

impl PointerBackend for DirectWindowsPointer {
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
        set_mouse_hold_active(true);
        Ok(())
    }

    fn button_up(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if !self.pressed.contains(button) {
            return Ok(());
        }

        send_inputs(&[mouse_input(0, 0, button_up_flag(button))])?;
        self.pressed.remove(button);
        set_mouse_hold_active(!self.pressed.is_empty());
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
        set_mouse_hold_active(false);
        Ok(())
    }
}

impl Drop for DirectWindowsPointer {
    fn drop(&mut self) {
        let _ = self.release_all();
    }
}

/// Pointer backend for the normal `NumFlow` runtime.
///
/// Installed production builds prefer the signed `numflow-input.exe` `UIAccess` helper. Development,
/// portable, elevated, and secure runtimes keep using direct `SendInput` when the helper is absent
/// or Windows did not grant its `UIAccess` token.
#[derive(Debug)]
pub struct WindowsPointer {
    direct: DirectWindowsPointer,
    helper: Option<HelperClient>,
    helper_pressed: PressedButtons,
    last_helper_attempt: Option<Instant>,
    helper_disabled: bool,
}

impl Default for WindowsPointer {
    fn default() -> Self {
        let helper_disabled = current_runtime_kind() != RuntimeKind::Normal
            || current_process_elevated().unwrap_or(false)
            || current_process_ui_access().unwrap_or(false);
        let mut pointer = Self {
            direct: DirectWindowsPointer::default(),
            helper: None,
            helper_pressed: PressedButtons::default(),
            last_helper_attempt: None,
            helper_disabled,
        };
        pointer.ensure_helper();
        pointer
    }
}

impl WindowsPointer {
    #[must_use]
    pub const fn is_button_held(&self, button: MouseButton) -> bool {
        self.helper_pressed.contains(button) || self.direct.is_button_held(button)
    }

    fn ensure_helper(&mut self) {
        if self.helper_disabled || self.helper.is_some() || self.direct.has_held_buttons() {
            return;
        }

        let now = Instant::now();
        if !reconnect_allowed(self.last_helper_attempt, now, RECONNECT_COOLDOWN) {
            return;
        }
        self.last_helper_attempt = Some(now);

        match connect_or_spawn_helper() {
            Ok(mut helper) => {
                if helper.ui_access() {
                    eprintln!(
                        "NumFlow: UIAccess input helper connected (pid={})",
                        helper.server_pid()
                    );
                    self.helper = Some(helper);
                } else {
                    eprintln!(
                        "NumFlow: input helper started without UIAccess; using direct SendInput fallback"
                    );
                    let _ = helper.shutdown();
                    self.helper_disabled = true;
                }
            }
            Err(InputServiceError::HelperMissing) => {
                self.helper_disabled = true;
            }
            Err(error) => {
                eprintln!("NumFlow: input helper unavailable: {error}");
            }
        }
    }

    fn helper_request(&mut self, message: Message) -> Option<Result<(), PointerError>> {
        self.ensure_helper();
        let helper = self.helper.as_mut()?;
        let result = helper.request(message).map_err(PointerError::from);
        if result.is_err() {
            self.helper.take();
            self.helper_pressed.clear();
            set_mouse_hold_active(self.direct.has_held_buttons());
        }
        Some(result)
    }

    fn helper_active(&self) -> bool {
        self.helper.is_some()
    }
}

impl PointerBackend for WindowsPointer {
    type Error = PointerError;

    fn move_relative(&mut self, dx: i32, dy: i32) -> Result<(), Self::Error> {
        if dx == 0 && dy == 0 {
            return Ok(());
        }
        if let Some(result) = self.helper_request(Message::PointerMove { dx, dy }) {
            return result;
        }
        self.direct.move_relative(dx, dy)
    }

    fn button_down(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if self.helper_pressed.contains(button) || self.direct.is_button_held(button) {
            return Ok(());
        }

        if (self.helper_active() || (!self.helper_disabled && !self.direct.has_held_buttons()))
            && let Some(result) = self.helper_request(Message::PointerButton {
                button,
                action: ButtonAction::Down,
            })
        {
            result?;
            self.helper_pressed.insert(button);
            set_mouse_hold_active(true);
            return Ok(());
        }

        self.direct.button_down(button)
    }

    fn button_up(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if self.helper_pressed.contains(button) {
            let Some(result) = self.helper_request(Message::PointerButton {
                button,
                action: ButtonAction::Up,
            }) else {
                return Err(PointerError::Helper {
                    reason: "helper connection disappeared while a button was held".to_owned(),
                });
            };
            result?;
            self.helper_pressed.remove(button);
            set_mouse_hold_active(
                !self.helper_pressed.is_empty() || self.direct.has_held_buttons(),
            );
            return Ok(());
        }
        self.direct.button_up(button)
    }

    fn click(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if self.is_button_held(button) {
            return Err(PointerError::ButtonAlreadyHeld { button });
        }
        if let Some(result) = self.helper_request(Message::Click { button }) {
            return result;
        }
        self.direct.click(button)
    }

    fn double_click(&mut self, button: MouseButton) -> Result<(), Self::Error> {
        if self.is_button_held(button) {
            return Err(PointerError::ButtonAlreadyHeld { button });
        }
        if let Some(result) = self.helper_request(Message::DoubleClick { button }) {
            return result;
        }
        self.direct.double_click(button)
    }

    fn release_all(&mut self) -> Result<(), Self::Error> {
        let helper_result = if self.helper.is_some() {
            self.helper_request(Message::ReleaseAll).transpose()
        } else {
            Ok(None)
        };

        let direct_result = self.direct.release_all();
        if helper_result.is_ok() {
            self.helper_pressed.clear();
        }
        set_mouse_hold_active(false);

        direct_result?;
        helper_result?;
        Ok(())
    }
}

impl Drop for WindowsPointer {
    fn drop(&mut self) {
        let _ = self.release_all();
        if let Some(mut helper) = self.helper.take() {
            let _ = helper.shutdown();
        }
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
