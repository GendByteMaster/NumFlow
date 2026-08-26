#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
}

impl Direction {
    pub const ALL: [Self; 8] = [
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
        Self::UpLeft,
        Self::UpRight,
        Self::DownLeft,
        Self::DownRight,
    ];

    #[must_use]
    pub const fn unit_vector(self) -> (i8, i8) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
            Self::UpLeft => (-1, -1),
            Self::UpRight => (1, -1),
            Self::DownLeft => (-1, 1),
            Self::DownRight => (1, 1),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClickKind {
    Single,
    Double,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputAction {
    Move(Direction),
    Click,
    DoubleClick,
    Hold,
    Release,
    SelectButton(MouseButton),
    ToggleEnabled,
    SetEnabled(bool),
    SetPrecision(bool),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerEffect {
    Move(Direction),
    Click {
        button: MouseButton,
        kind: ClickKind,
    },
    ButtonDown(MouseButton),
    ButtonUp(MouseButton),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateChange {
    Enabled(bool),
    Precision(bool),
    SelectedButton(MouseButton),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CoreEffect {
    Pointer(PointerEffect),
    State(StateChange),
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::Direction;

    #[test]
    fn every_direction_has_a_unique_unit_vector() {
        let vectors = Direction::ALL
            .into_iter()
            .map(Direction::unit_vector)
            .collect::<HashSet<_>>();

        assert_eq!(vectors.len(), Direction::ALL.len());
    }

    #[test]
    fn direction_vectors_match_screen_coordinates() {
        assert_eq!(Direction::Up.unit_vector(), (0, -1));
        assert_eq!(Direction::Down.unit_vector(), (0, 1));
        assert_eq!(Direction::Left.unit_vector(), (-1, 0));
        assert_eq!(Direction::Right.unit_vector(), (1, 0));
        assert_eq!(Direction::UpLeft.unit_vector(), (-1, -1));
        assert_eq!(Direction::UpRight.unit_vector(), (1, -1));
        assert_eq!(Direction::DownLeft.unit_vector(), (-1, 1));
        assert_eq!(Direction::DownRight.unit_vector(), (1, 1));
    }
}
