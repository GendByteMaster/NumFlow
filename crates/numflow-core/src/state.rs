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
            InputAction::Hold if self.enabled && self.held_button.is_none() => {
                self.held_button = Some(self.selected_button);
                vec![CoreEffect::Pointer(PointerEffect::ButtonDown(
                    self.selected_button,
                ))]
            }
            InputAction::Release => self.release_held_button(),
            InputAction::SelectButton(button) => self.select_button(button),
            InputAction::ToggleEnabled => self.set_enabled(!self.enabled),
            InputAction::SetEnabled(enabled) => self.set_enabled(enabled),
            InputAction::SetPrecision(precision) => self.set_precision(precision),
            InputAction::Move(_)
            | InputAction::Click
            | InputAction::DoubleClick
            | InputAction::Hold => Vec::new(),
        }
    }

    /// Transitions the controller into a safe stopped state.
    ///
    /// Any physically-held button is released before the disabled state is emitted. Calling this
    /// method repeatedly is safe and produces no duplicate release or state-change effects.
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

#[cfg(test)]
mod tests {
    use super::ControllerState;
    use crate::{
        ClickKind, CoreEffect, Direction, InputAction, MouseButton, PointerEffect, StateChange,
    };

    #[test]
    fn controller_starts_disabled_and_safe() {
        let state = ControllerState::default();

        assert!(!state.is_enabled());
        assert!(!state.is_precision_enabled());
        assert!(!state.is_dragging());
        assert_eq!(state.selected_button(), MouseButton::Left);
        assert_eq!(state.held_button(), None);
    }

    #[test]
    fn pointer_actions_are_suppressed_while_disabled() {
        let mut state = ControllerState::default();

        assert!(state.apply(InputAction::Move(Direction::Up)).is_empty());
        assert!(state.apply(InputAction::Click).is_empty());
        assert!(state.apply(InputAction::DoubleClick).is_empty());
        assert!(state.apply(InputAction::Hold).is_empty());
    }

    #[test]
    fn enabled_controller_emits_move_and_click_effects() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));

        assert_eq!(
            state.apply(InputAction::Move(Direction::DownRight)),
            vec![CoreEffect::Pointer(PointerEffect::Move(
                Direction::DownRight
            ))]
        );
        assert_eq!(
            state.apply(InputAction::Click),
            vec![CoreEffect::Pointer(PointerEffect::Click {
                button: MouseButton::Left,
                kind: ClickKind::Single,
            })]
        );
        assert_eq!(
            state.apply(InputAction::DoubleClick),
            vec![CoreEffect::Pointer(PointerEffect::Click {
                button: MouseButton::Left,
                kind: ClickKind::Double,
            })]
        );
    }

    #[test]
    fn hold_and_release_track_the_actual_held_button() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));
        state.apply(InputAction::SelectButton(MouseButton::Right));

        assert_eq!(
            state.apply(InputAction::Hold),
            vec![CoreEffect::Pointer(PointerEffect::ButtonDown(
                MouseButton::Right
            ))]
        );
        assert!(state.is_dragging());
        assert_eq!(state.held_button(), Some(MouseButton::Right));

        state.apply(InputAction::SelectButton(MouseButton::Middle));

        assert_eq!(
            state.apply(InputAction::Release),
            vec![CoreEffect::Pointer(PointerEffect::ButtonUp(
                MouseButton::Right
            ))]
        );
        assert!(!state.is_dragging());
        assert_eq!(state.held_button(), None);
        assert_eq!(state.selected_button(), MouseButton::Middle);
    }

    #[test]
    fn repeated_hold_does_not_emit_duplicate_button_down() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));

        assert!(!state.apply(InputAction::Hold).is_empty());
        assert!(state.apply(InputAction::Hold).is_empty());
    }

    #[test]
    fn release_without_a_held_button_is_a_no_op() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));

        assert!(state.apply(InputAction::Release).is_empty());
        assert!(!state.is_dragging());
    }

    #[test]
    fn click_is_suppressed_during_drag_but_movement_is_allowed() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));
        state.apply(InputAction::Hold);

        assert!(state.apply(InputAction::Click).is_empty());
        assert!(state.apply(InputAction::DoubleClick).is_empty());
        assert_eq!(
            state.apply(InputAction::Move(Direction::Right)),
            vec![CoreEffect::Pointer(PointerEffect::Move(Direction::Right))]
        );
    }

    #[test]
    fn changing_selection_during_drag_does_not_change_the_held_button() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));
        state.apply(InputAction::SelectButton(MouseButton::Right));
        state.apply(InputAction::Hold);

        assert_eq!(
            state.apply(InputAction::SelectButton(MouseButton::Middle)),
            vec![CoreEffect::State(StateChange::SelectedButton(
                MouseButton::Middle
            ))]
        );
        assert_eq!(state.selected_button(), MouseButton::Middle);
        assert_eq!(state.held_button(), Some(MouseButton::Right));

        assert_eq!(
            state.apply(InputAction::Release),
            vec![CoreEffect::Pointer(PointerEffect::ButtonUp(
                MouseButton::Right
            ))]
        );
    }

    #[test]
    fn rapid_selection_drag_release_and_click_stay_synchronized() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));

        state.apply(InputAction::SelectButton(MouseButton::Right));
        assert_eq!(
            state.apply(InputAction::Hold),
            vec![CoreEffect::Pointer(PointerEffect::ButtonDown(
                MouseButton::Right
            ))]
        );

        state.apply(InputAction::SelectButton(MouseButton::Middle));
        assert!(state.apply(InputAction::Click).is_empty());
        assert_eq!(
            state.apply(InputAction::Release),
            vec![CoreEffect::Pointer(PointerEffect::ButtonUp(
                MouseButton::Right
            ))]
        );
        assert_eq!(
            state.apply(InputAction::Click),
            vec![CoreEffect::Pointer(PointerEffect::Click {
                button: MouseButton::Middle,
                kind: ClickKind::Single,
            })]
        );
        assert!(!state.is_dragging());
        assert_eq!(state.selected_button(), MouseButton::Middle);
        assert_eq!(state.held_button(), None);
    }

    #[test]
    fn rapid_button_selection_uses_the_latest_button_for_clicks() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));

        state.apply(InputAction::SelectButton(MouseButton::Right));
        state.apply(InputAction::SelectButton(MouseButton::Middle));
        state.apply(InputAction::SelectButton(MouseButton::Left));
        state.apply(InputAction::SelectButton(MouseButton::Right));

        assert_eq!(
            state.apply(InputAction::DoubleClick),
            vec![CoreEffect::Pointer(PointerEffect::Click {
                button: MouseButton::Right,
                kind: ClickKind::Double,
            })]
        );
    }

    #[test]
    fn disabling_controller_releases_held_button_first() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));
        state.apply(InputAction::SelectButton(MouseButton::Right));
        state.apply(InputAction::Hold);

        assert_eq!(
            state.apply(InputAction::SetEnabled(false)),
            vec![
                CoreEffect::Pointer(PointerEffect::ButtonUp(MouseButton::Right)),
                CoreEffect::State(StateChange::Enabled(false)),
            ]
        );
        assert!(!state.is_enabled());
        assert!(!state.is_dragging());
        assert_eq!(state.held_button(), None);
    }

    #[test]
    fn shutdown_releases_held_button_before_disabling_and_is_idempotent() {
        let mut state = ControllerState::default();
        state.apply(InputAction::SetEnabled(true));
        state.apply(InputAction::SelectButton(MouseButton::Middle));
        state.apply(InputAction::Hold);

        assert_eq!(
            state.shutdown(),
            vec![
                CoreEffect::Pointer(PointerEffect::ButtonUp(MouseButton::Middle)),
                CoreEffect::State(StateChange::Enabled(false)),
            ]
        );
        assert!(!state.is_enabled());
        assert!(!state.is_dragging());
        assert_eq!(state.held_button(), None);
        assert!(state.shutdown().is_empty());
    }

    #[test]
    fn toggle_enabled_is_deterministic() {
        let mut state = ControllerState::default();

        assert_eq!(
            state.apply(InputAction::ToggleEnabled),
            vec![CoreEffect::State(StateChange::Enabled(true))]
        );
        assert_eq!(
            state.apply(InputAction::ToggleEnabled),
            vec![CoreEffect::State(StateChange::Enabled(false))]
        );
    }

    #[test]
    fn redundant_state_changes_are_no_ops() {
        let mut state = ControllerState::default();

        assert!(state.apply(InputAction::SetEnabled(false)).is_empty());
        assert!(state.apply(InputAction::SetPrecision(false)).is_empty());
        assert!(
            state
                .apply(InputAction::SelectButton(MouseButton::Left))
                .is_empty()
        );
    }

    #[test]
    fn precision_state_is_independent_from_enabled_state() {
        let mut state = ControllerState::default();

        assert_eq!(
            state.apply(InputAction::SetPrecision(true)),
            vec![CoreEffect::State(StateChange::Precision(true))]
        );
        assert!(state.is_precision_enabled());
        assert!(!state.is_enabled());
    }
}
