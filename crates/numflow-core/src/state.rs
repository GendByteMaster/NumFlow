use crate::{ClickKind, CoreEffect, InputAction, MouseButton, PointerEffect, StateChange};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerState {
    enabled: bool,
    precision: bool,
    selected_button: MouseButton,
    held_button: Option<MouseButton>,
}

impl Default for ControllerState {
    fn default() -> Self {
        Self {
            enabled: false,
            precision: false,
            selected_button: MouseButton::Left,
            held_button: None,
        }
    }
}

impl ControllerState {
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    #[must_use]
    pub const fn is_precision_enabled(&self) -> bool {
        self.precision
    }

    #[must_use]
    pub const fn selected_button(&self) -> MouseButton {
        self.selected_button
    }

    #[must_use]
    pub const fn held_button(&self) -> Option<MouseButton> {
        self.held_button
    }

    #[must_use]
    pub const fn is_dragging(&self) -> bool {
        self.held_button.is_some()
    }

    pub fn apply(&mut self, action: InputAction) -> Vec<CoreEffect> {
        match action {
            InputAction::Move(direction) if self.enabled => {
                vec![CoreEffect::Pointer(PointerEffect::Move(direction))]
            }
            InputAction::Click if self.can_click() => {
                vec![CoreEffect::Pointer(PointerEffect::Click {
                    button: self.selected_button,
                    kind: ClickKind::Single,
                })]
            }
            InputAction::DoubleClick if self.can_click() => {
                vec![CoreEffect::Pointer(PointerEffect::Click {
                    button: self.selected_button,
                    kind: ClickKind::Double,
                })]
            }
            InputAction::Hold => self.hold_button(self.selected_button),
            InputAction::Release => self.release_held_button(),
            InputAction::SelectButton(button) => self.select_button(button),
            InputAction::ToggleEnabled => self.set_enabled(!self.enabled),
            InputAction::SetEnabled(enabled) => self.set_enabled(enabled),
            InputAction::SetPrecision(precision) => self.set_precision(precision),
            InputAction::Move(_) | InputAction::Click | InputAction::DoubleClick => Vec::new(),
        }
    }

    /// Transitions the controller into a safe stopped state.
    ///
    /// Any physically-held button is released before the disabled state is emitted. Calling this
    /// method repeatedly is safe and produces no duplicate release or state-change effects.
    #[must_use = "shutdown effects must be dispatched so held mouse buttons are actually released"]
    pub fn shutdown(&mut self) -> Vec<CoreEffect> {
        let mut effects = self.release_held_button();

        if self.enabled {
            self.enabled = false;
            effects.push(CoreEffect::State(StateChange::Enabled(false)));
        }

        effects
    }

    const fn can_click(self) -> bool {
        self.enabled && self.held_button.is_none()
    }

    /// Holds a specific physical mouse button without changing the selected click button.
    ///
    /// Repeated calls while any button is already held are idempotent and never emit a duplicate
    /// `ButtonDown`.
    pub fn hold_button(&mut self, button: MouseButton) -> Vec<CoreEffect> {
        if !self.enabled || self.held_button.is_some() {
            return Vec::new();
        }

        self.held_button = Some(button);
        vec![CoreEffect::Pointer(PointerEffect::ButtonDown(button))]
    }

    fn release_held_button(&mut self) -> Vec<CoreEffect> {
        let Some(button) = self.held_button.take() else {
            return Vec::new();
        };

        vec![CoreEffect::Pointer(PointerEffect::ButtonUp(button))]
    }

    fn select_button(&mut self, button: MouseButton) -> Vec<CoreEffect> {
        if self.selected_button == button {
            return Vec::new();
        }

        self.selected_button = button;
        vec![CoreEffect::State(StateChange::SelectedButton(button))]
    }

    fn set_enabled(&mut self, enabled: bool) -> Vec<CoreEffect> {
        if self.enabled == enabled {
            return Vec::new();
        }

        if !enabled {
            return self.shutdown();
        }

        self.enabled = true;
        vec![CoreEffect::State(StateChange::Enabled(true))]
    }

    fn set_precision(&mut self, precision: bool) -> Vec<CoreEffect> {
        if self.precision == precision {
            return Vec::new();
        }

        self.precision = precision;
        vec![CoreEffect::State(StateChange::Precision(precision))]
    }
}
