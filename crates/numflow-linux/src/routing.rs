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
    use evdev::{EventType, InputEvent, KeyCode, SynchronizationCode};
    use numflow_core::NumpadKey;

    use super::{
        LinuxInputEvent, LinuxKeyCode, LinuxKeyState, NumLockRouter, RoutingDecision,
        process_event_batch,
    };

    fn key(code: KeyCode, value: i32) -> InputEvent {
        InputEvent::new(EventType::KEY.0, code.0, value)
    }

    fn syn_report() -> InputEvent {
        InputEvent::new(
            EventType::SYNCHRONIZATION.0,
            SynchronizationCode::SYN_REPORT.0,
            0,
        )
    }

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

    #[test]
    fn batch_with_num_lock_on_replays_keys_in_original_order_without_syn_report() {
        let mut router = NumLockRouter::new(true);
        let events = [
            key(KeyCode::KEY_A, 1),
            key(KeyCode::KEY_KP8, 1),
            syn_report(),
        ];

        let processed = process_event_batch(&events, &mut router);

        assert_eq!(processed.replay, events[..2]);
        assert!(processed.runtime.is_empty());
    }

    #[test]
    fn batch_with_num_lock_off_consumes_only_mapped_keypad_events() {
        let mut router = NumLockRouter::new(false);
        let events = [
            key(KeyCode::KEY_A, 1),
            key(KeyCode::KEY_KP8, 1),
            key(KeyCode::KEY_KP8, 0),
            syn_report(),
        ];

        let processed = process_event_batch(&events, &mut router);

        assert_eq!(processed.replay, vec![events[0]]);
        assert_eq!(
            processed.runtime,
            vec![
                LinuxInputEvent::Numpad {
                    key: NumpadKey::Num8,
                    state: LinuxKeyState::Pressed,
                },
                LinuxInputEvent::Numpad {
                    key: NumpadKey::Num8,
                    state: LinuxKeyState::Released,
                },
            ]
        );
    }

    #[test]
    fn batch_replays_num_lock_and_emits_one_transition_per_press_edge() {
        let mut router = NumLockRouter::new(true);
        let events = [
            key(KeyCode::KEY_NUMLOCK, 1),
            key(KeyCode::KEY_NUMLOCK, 2),
            key(KeyCode::KEY_NUMLOCK, 0),
            syn_report(),
        ];

        let processed = process_event_batch(&events, &mut router);

        assert_eq!(processed.replay, events[..3]);
        assert_eq!(
            processed.runtime,
            vec![LinuxInputEvent::NumLockChanged { num_lock_on: false }]
        );
    }
}