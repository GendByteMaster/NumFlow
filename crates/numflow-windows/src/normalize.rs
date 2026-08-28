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
