use std::collections::BTreeMap;

use crate::{Direction, InputAction, MouseButton};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NumpadKey {
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Add,
    Decimal,
    Divide,
    Multiply,
    Subtract,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bindings {
    entries: BTreeMap<NumpadKey, InputAction>,
}

impl Default for Bindings {
    fn default() -> Self {
        let entries = [
            (NumpadKey::Num8, InputAction::Move(Direction::Up)),
            (NumpadKey::Num2, InputAction::Move(Direction::Down)),
            (NumpadKey::Num4, InputAction::Move(Direction::Left)),
            (NumpadKey::Num6, InputAction::Move(Direction::Right)),
            (NumpadKey::Num7, InputAction::Move(Direction::UpLeft)),
            (NumpadKey::Num9, InputAction::Move(Direction::UpRight)),
            (NumpadKey::Num1, InputAction::Move(Direction::DownLeft)),
            (NumpadKey::Num3, InputAction::Move(Direction::DownRight)),
            (NumpadKey::Num5, InputAction::Click),
            (NumpadKey::Add, InputAction::DoubleClick),
            (NumpadKey::Num0, InputAction::Hold),
            (NumpadKey::Decimal, InputAction::Release),
            (
                NumpadKey::Divide,
                InputAction::SelectButton(MouseButton::Left),
            ),
            (
                NumpadKey::Multiply,
                InputAction::SelectButton(MouseButton::Right),
            ),
            (
                NumpadKey::Subtract,
                InputAction::SelectButton(MouseButton::Middle),
            ),
        ]
        .into_iter()
        .collect();

        Self { entries }
    }
}

impl Bindings {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn action_for(&self, key: NumpadKey) -> Option<InputAction> {
        self.entries.get(&key).copied()
    }

    pub fn bind(&mut self, key: NumpadKey, action: InputAction) -> Option<InputAction> {
        self.entries.insert(key, action)
    }

    pub fn unbind(&mut self, key: NumpadKey) -> Option<InputAction> {
        self.entries.remove(&key)
    }

    pub fn iter(&self) -> impl Iterator<Item = (NumpadKey, InputAction)> + '_ {
        self.entries.iter().map(|(&key, &action)| (key, action))
    }
}

#[cfg(test)]
mod tests {
    use super::{Bindings, NumpadKey};
    use crate::{Direction, InputAction, MouseButton};

    #[test]
    fn empty_bindings_resolve_no_actions() {
        let bindings = Bindings::empty();

        assert_eq!(bindings.action_for(NumpadKey::Num5), None);
        assert_eq!(bindings.iter().count(), 0);
    }

    #[test]
    fn default_bindings_match_numflow_controls() {
        let bindings = Bindings::default();

        let expected = [
            (NumpadKey::Num8, InputAction::Move(Direction::Up)),
            (NumpadKey::Num2, InputAction::Move(Direction::Down)),
            (NumpadKey::Num4, InputAction::Move(Direction::Left)),
            (NumpadKey::Num6, InputAction::Move(Direction::Right)),
            (NumpadKey::Num7, InputAction::Move(Direction::UpLeft)),
            (NumpadKey::Num9, InputAction::Move(Direction::UpRight)),
            (NumpadKey::Num1, InputAction::Move(Direction::DownLeft)),
            (NumpadKey::Num3, InputAction::Move(Direction::DownRight)),
            (NumpadKey::Num5, InputAction::Click),
            (NumpadKey::Add, InputAction::DoubleClick),
            (NumpadKey::Num0, InputAction::Hold),
            (NumpadKey::Decimal, InputAction::Release),
            (
                NumpadKey::Divide,
                InputAction::SelectButton(MouseButton::Left),
            ),
            (
                NumpadKey::Multiply,
                InputAction::SelectButton(MouseButton::Right),
            ),
            (
                NumpadKey::Subtract,
                InputAction::SelectButton(MouseButton::Middle),
            ),
        ];

        for (key, action) in expected {
            assert_eq!(bindings.action_for(key), Some(action));
        }

        assert_eq!(bindings.iter().count(), expected.len());
    }

    #[test]
    fn bindings_can_be_reassigned_and_removed() {
        let mut bindings = Bindings::default();

        assert_eq!(
            bindings.bind(NumpadKey::Num5, InputAction::DoubleClick),
            Some(InputAction::Click)
        );
        assert_eq!(
            bindings.action_for(NumpadKey::Num5),
            Some(InputAction::DoubleClick)
        );
        assert_eq!(
            bindings.unbind(NumpadKey::Num5),
            Some(InputAction::DoubleClick)
        );
        assert_eq!(bindings.action_for(NumpadKey::Num5), None);
    }
}
