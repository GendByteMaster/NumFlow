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
