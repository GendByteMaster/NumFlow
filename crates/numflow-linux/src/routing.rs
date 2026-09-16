use crate::events::{LinuxInputEvent, LinuxKeyCode, LinuxKeyState, map_numpad_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingDecision {
    Replay,
    Consume(LinuxInputEvent),
    ReplayAndEmit(LinuxInputEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumLockRouter {
    num_lock_on: bool,
    num_lock_pressed: bool,
}

impl NumLockRouter {
    #[must_use]
    pub const fn new(num_lock_on: bool) -> Self {
        Self {
            num_lock_on,
            num_lock_pressed: false,
        }
    }

    #[must_use]
    pub const fn num_lock_on(&self) -> bool {
        self.num_lock_on
    }

    pub fn route(&mut self, key: LinuxKeyCode, state: LinuxKeyState) -> RoutingDecision {
        if key == LinuxKeyCode::NumLock {
            return self.route_num_lock(state);
        }

        if self.num_lock_on {
            return RoutingDecision::Replay;
        }

        map_numpad_key(key).map_or(RoutingDecision::Replay, |key| {
            RoutingDecision::Consume(LinuxInputEvent::Numpad { key, state })
        })
    }

    fn route_num_lock(&mut self, state: LinuxKeyState) -> RoutingDecision {
        match state {
            LinuxKeyState::Pressed if !self.num_lock_pressed => {
                self.num_lock_pressed = true;
                self.num_lock_on = !self.num_lock_on;
                RoutingDecision::ReplayAndEmit(LinuxInputEvent::NumLockChanged {
                    num_lock_on: self.num_lock_on,
                })
            }
            LinuxKeyState::Pressed | LinuxKeyState::Repeated => RoutingDecision::Replay,
            LinuxKeyState::Released => {
                self.num_lock_pressed = false;
                RoutingDecision::Replay
            }
        }
    }
}

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
    fn unrelated_keys_replay_while_numflow_is_active() {
        let mut router = NumLockRouter::new(false);

        assert_eq!(
            router.route(LinuxKeyCode::Other(30), LinuxKeyState::Pressed),
            RoutingDecision::Replay
        );
    }

    #[test]
    fn num_lock_press_replays_and_emits_one_transition_until_release() {
        let mut router = NumLockRouter::new(true);

        assert_eq!(
            router.route(LinuxKeyCode::NumLock, LinuxKeyState::Pressed),
            RoutingDecision::ReplayAndEmit(LinuxInputEvent::NumLockChanged { num_lock_on: false })
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
            RoutingDecision::ReplayAndEmit(LinuxInputEvent::NumLockChanged { num_lock_on: true })
        );
    }
}
