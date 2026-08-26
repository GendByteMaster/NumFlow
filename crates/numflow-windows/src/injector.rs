use std::{io, mem::size_of};

use numflow_core::{ClickKind, Direction, MouseButton, PointerEffect};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP,
    MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEINPUT, SendInput,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerConfig {
    pub step_pixels: i32,
    pub precision_step_pixels: i32,
}

impl Default for PointerConfig {
    fn default() -> Self {
        Self {
            step_pixels: 12,
            precision_step_pixels: 3,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InjectionError {
    #[error("SendInput accepted {sent} of {requested} mouse events: {source}")]
    PartialSend {
        sent: u32,
        requested: u32,
        #[source]
        source: io::Error,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MouseCommand {
    dx: i32,
    dy: i32,
    flags: u32,
}

impl MouseCommand {
    const fn stationary(flags: u32) -> Self {
        Self { dx: 0, dy: 0, flags }
    }

    const fn movement(dx: i32, dy: i32) -> Self {
        Self {
            dx,
            dy,
            flags: MOUSEEVENTF_MOVE,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MouseInjector {
    config: PointerConfig,
}

impl MouseInjector {
    #[must_use]
    pub const fn new(config: PointerConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub const fn config(&self) -> PointerConfig {
        self.config
    }

    pub fn apply(&self, effect: PointerEffect, precision: bool) -> Result<(), InjectionError> {
        let commands = commands_for(effect, self.config, precision);
        let inputs = commands.into_iter().map(command_to_input).collect::<Vec<_>>();
        send_inputs(&inputs)
    }
}

fn commands_for(effect: PointerEffect, config: PointerConfig, precision: bool) -> Vec<MouseCommand> {
    match effect {
        PointerEffect::Move(direction) => {
            let step = if precision {
                config.precision_step_pixels
            } else {
                config.step_pixels
            };
            let (x, y) = direction.unit_vector();
            vec![MouseCommand::movement(i32::from(x) * step, i32::from(y) * step)]
        }
        PointerEffect::Click { button, kind } => {
            let (down, up) = button_flags(button);
            let repetitions = match kind {
                ClickKind::Single => 1,
                ClickKind::Double => 2,
            };
            let mut commands = Vec::with_capacity(repetitions * 2);
            for _ in 0..repetitions {
                commands.push(MouseCommand::stationary(down));
                commands.push(MouseCommand::stationary(up));
            }
            commands
        }
        PointerEffect::ButtonDown(button) => {
            let (down, _) = button_flags(button);
            vec![MouseCommand::stationary(down)]
        }
        PointerEffect::ButtonUp(button) => {
            let (_, up) = button_flags(button);
            vec![MouseCommand::stationary(up)]
        }
    }
}

const fn button_flags(button: MouseButton) -> (u32, u32) {
    match button {
        MouseButton::Left => (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP),
        MouseButton::Right => (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP),
        MouseButton::Middle => (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP),
    }
}

fn command_to_input(command: MouseCommand) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx: command.dx,
                dy: command.dy,
                mouseData: 0,
                dwFlags: command.flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn send_inputs(inputs: &[INPUT]) -> Result<(), InjectionError> {
    let requested = u32::try_from(inputs.len()).expect("mouse input batches fit in u32");
    let sent = unsafe {
        SendInput(
            requested,
            inputs.as_ptr(),
            i32::try_from(size_of::<INPUT>()).expect("INPUT size fits in i32"),
        )
    };

    if sent == requested {
        Ok(())
    } else {
        Err(InjectionError::PartialSend {
            sent,
            requested,
            source: io::Error::last_os_error(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{MouseCommand, PointerConfig, button_flags, commands_for};
    use numflow_core::{ClickKind, Direction, MouseButton, PointerEffect};
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN,
        MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP,
    };

    #[test]
    fn movement_uses_normal_and_precision_steps() {
        let config = PointerConfig {
            step_pixels: 12,
            precision_step_pixels: 3,
        };

        assert_eq!(
            commands_for(PointerEffect::Move(Direction::UpRight), config, false),
            vec![MouseCommand {
                dx: 12,
                dy: -12,
                flags: MOUSEEVENTF_MOVE,
            }]
        );
        assert_eq!(
            commands_for(PointerEffect::Move(Direction::UpRight), config, true),
            vec![MouseCommand {
                dx: 3,
                dy: -3,
                flags: MOUSEEVENTF_MOVE,
            }]
        );
    }

    #[test]
    fn button_flags_match_win32_mouse_events() {
        assert_eq!(button_flags(MouseButton::Left), (MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP));
        assert_eq!(
            button_flags(MouseButton::Right),
            (MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP)
        );
        assert_eq!(
            button_flags(MouseButton::Middle),
            (MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP)
        );
    }

    #[test]
    fn double_click_is_two_complete_click_cycles() {
        let commands = commands_for(
            PointerEffect::Click {
                button: MouseButton::Left,
                kind: ClickKind::Double,
            },
            PointerConfig::default(),
            false,
        );

        assert_eq!(
            commands,
            vec![
                MouseCommand::stationary(MOUSEEVENTF_LEFTDOWN),
                MouseCommand::stationary(MOUSEEVENTF_LEFTUP),
                MouseCommand::stationary(MOUSEEVENTF_LEFTDOWN),
                MouseCommand::stationary(MOUSEEVENTF_LEFTUP),
            ]
        );
    }
}
