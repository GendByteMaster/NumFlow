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
