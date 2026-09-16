#[cfg(test)]
mod tests {
    use numflow_core::NumpadKey;

    use super::{LinuxInputEvent, LinuxKeyCode, LinuxKeyState, NumLockRouter, RoutingDecision};

    #[test]
    fn num_lock_on_replays_keypad_input() {
        let mut router = NumLockRouter::new(true);

        assert_eq!(
            router.route(LinuxKeyCode::Kp8, LinuxKeyState::Pressed),
            RoutingDecision::Replay
        );
    }

    #[test]
    fn num_lock_off_consumes_mapped_keypad_input() {
        let mut router = NumLockRouter::new(false);

        assert_eq!(
            router.route(LinuxKeyCode::Kp8, LinuxKeyState::Pressed),
            RoutingDecision::Consume(LinuxInputEvent::Numpad {
                key: NumpadKey::Num8,
                state: LinuxKeyState::Pressed,
            })
        );
    }

    #[test]
    fn num_lock_press_replays_and_emits_one_transition_until_release() {
        let mut router = NumLockRouter::new(true);

        assert_eq!(
            router.route(LinuxKeyCode::NumLock, LinuxKeyState::Pressed),
            RoutingDecision::ReplayAndEmit(LinuxInputEvent::NumLockChanged {
                num_lock_on: false,
            })
        );
        assert_eq!(
            router.route(LinuxKeyCode::NumLock, LinuxKeyState::Repeated),
            RoutingDecision::Replay
        );
        assert_eq!(
            router.route(LinuxKeyCode::NumLock, LinuxKeyState::Released),
            RoutingDecision::Replay
        );
        assert_eq!(
            router.route(LinuxKeyCode::NumLock, LinuxKeyState::Pressed),
            RoutingDecision::ReplayAndEmit(LinuxInputEvent::NumLockChanged {
                num_lock_on: true,
            })
        );
    }
}
