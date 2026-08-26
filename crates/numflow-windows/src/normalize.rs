use std::collections::BTreeSet;

use numflow_core::{Bindings, InputAction, NumpadKey};

use crate::{KeyState, PhysicalKeyEvent, map_numpad_key};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NormalizedKeyEvent {
    pub key: NumpadKey,
    pub action: InputAction,
    pub state: KeyState,
    pub repeated: bool,
}

#[derive(Debug, Default)]
pub struct KeyboardEventNormalizer {
    pressed: BTreeSet<NumpadKey>,
}

impl KeyboardEventNormalizer {
    #[must_use]
    pub fn process(
        &mut self,
        event: PhysicalKeyEvent,
        bindings: &Bindings,
    ) -> Option<NormalizedKeyEvent> {
        let key = map_numpad_key(event)?;
        let action = bindings.action_for(key)?;

        match event.state {
            KeyState::Pressed => {
                let repeated = !self.pressed.insert(key);
                if repeated && !matches!(action, InputAction::Move(_)) {
                    return None;
                }

                Some(NormalizedKeyEvent {
                    key,
                    action,
                    state: KeyState::Pressed,
                    repeated,
                })
            }
            KeyState::Released => {
                self.pressed.remove(&key);
                Some(NormalizedKeyEvent {
                    key,
                    action,
                    state: KeyState::Released,
                    repeated: false,
                })
            }
        }
    }

    pub fn reset(&mut self) {
        self.pressed.clear();
    }

    #[must_use]
    pub fn is_pressed(&self, key: NumpadKey) -> bool {
        self.pressed.contains(&key)
    }
}

#[cfg(test)]
mod tests {
    use super::KeyboardEventNormalizer;
    use crate::{KeyState, PhysicalKeyEvent};
    use numflow_core::{Bindings, InputAction, NumpadKey};

    fn event(scan_code: u32, state: KeyState) -> PhysicalKeyEvent {
        PhysicalKeyEvent::new(0, scan_code, false, state)
    }

    #[test]
    fn click_auto_repeat_is_suppressed_until_release() {
        let bindings = Bindings::default();
        let mut normalizer = KeyboardEventNormalizer::default();

        let first = normalizer
            .process(event(0x4C, KeyState::Pressed), &bindings)
            .expect("first Num5 press should be emitted");
        assert_eq!(first.key, NumpadKey::Num5);
        assert_eq!(first.action, InputAction::Click);
        assert!(!first.repeated);

        assert!(
            normalizer
                .process(event(0x4C, KeyState::Pressed), &bindings)
                .is_none()
        );

        assert!(
            normalizer
                .process(event(0x4C, KeyState::Released), &bindings)
                .is_some()
        );

        let next = normalizer
            .process(event(0x4C, KeyState::Pressed), &bindings)
            .expect("press after release should be emitted again");
        assert!(!next.repeated);
    }

    #[test]
    fn movement_auto_repeat_is_preserved() {
        let bindings = Bindings::default();
        let mut normalizer = KeyboardEventNormalizer::default();

        let first = normalizer
            .process(event(0x48, KeyState::Pressed), &bindings)
            .expect("first Num8 press should be emitted");
        let repeated = normalizer
            .process(event(0x48, KeyState::Pressed), &bindings)
            .expect("movement repeat should be emitted");

        assert_eq!(first.action, InputAction::Move(numflow_core::Direction::Up));
        assert!(!first.repeated);
        assert!(repeated.repeated);
    }

    #[test]
    fn custom_move_binding_can_repeat_on_non_direction_key() {
        let mut bindings = Bindings::default();
        bindings.bind(
            NumpadKey::Num5,
            InputAction::Move(numflow_core::Direction::Left),
        );
        let mut normalizer = KeyboardEventNormalizer::default();

        assert!(
            normalizer
                .process(event(0x4C, KeyState::Pressed), &bindings)
                .is_some()
        );
        let repeated = normalizer
            .process(event(0x4C, KeyState::Pressed), &bindings)
            .expect("custom movement binding should repeat");
        assert!(repeated.repeated);
    }

    #[test]
    fn reset_clears_pressed_state() {
        let bindings = Bindings::default();
        let mut normalizer = KeyboardEventNormalizer::default();
        normalizer.process(event(0x4C, KeyState::Pressed), &bindings);
        assert!(normalizer.is_pressed(NumpadKey::Num5));

        normalizer.reset();
        assert!(!normalizer.is_pressed(NumpadKey::Num5));
    }
}
