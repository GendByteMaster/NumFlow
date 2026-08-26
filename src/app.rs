use std::{cell::RefCell, rc::Rc};

use numflow_core::{ControllerState, CoreEffect, InputAction, MotionConfig, MouseButton};
use slint::ComponentHandle;

use crate::{AppWindow, MouseButtonMode, error::AppError};

const DEFAULT_POINTER_SPEED: f32 = 180.0;
const DEFAULT_POINTER_ACCELERATION: f32 = 900.0;

type SharedUiSettings = Rc<RefCell<UiSettings>>;

#[derive(Debug)]
struct UiSettings {
    controller: ControllerState,
    motion: MotionConfig,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            controller: ControllerState::default(),
            motion: MotionConfig::default(),
        }
    }
}

impl UiSettings {
    fn set_enabled(&mut self, enabled: bool) {
        self.apply_state_action(InputAction::SetEnabled(enabled));
    }

    fn set_mouse_button(&mut self, button: MouseButton) {
        self.apply_state_action(InputAction::SelectButton(button));
    }

    fn set_precision(&mut self, enabled: bool) {
        self.apply_state_action(InputAction::SetPrecision(enabled));
    }

    fn set_pointer_speed(&mut self, speed: f32) {
        let mut config = self.motion;
        config.base_speed = f64::from(speed);
        self.motion = config.sanitized();
    }

    fn set_pointer_acceleration(&mut self, acceleration: f32) {
        let mut config = self.motion;
        config.acceleration = f64::from(acceleration);
        self.motion = config.sanitized();
    }

    fn reset_defaults(&mut self) {
        let enabled = self.controller.is_enabled();

        self.controller = ControllerState::default();
        if enabled {
            self.apply_state_action(InputAction::SetEnabled(true));
        }

        self.motion = MotionConfig::default();
    }

    fn apply_state_action(&mut self, action: InputAction) {
        let effects = self.controller.apply(action);

        debug_assert!(
            effects
                .iter()
                .all(|effect| matches!(effect, CoreEffect::State(_))),
            "UI settings action unexpectedly emitted a pointer effect"
        );
    }
}

fn map_mouse_button(button: MouseButtonMode) -> MouseButton {
    match button {
        MouseButtonMode::Left => MouseButton::Left,
        MouseButtonMode::Right => MouseButton::Right,
        MouseButtonMode::Middle => MouseButton::Middle,
    }
}

fn connect_ui(window: &AppWindow, settings: &SharedUiSettings) {
    {
        let settings = Rc::clone(settings);
        window.on_enabled_toggled(move |enabled| {
            settings.borrow_mut().set_enabled(enabled);
        });
    }

    {
        let settings = Rc::clone(settings);
        window.on_mouse_button_changed(move |button| {
            settings
                .borrow_mut()
                .set_mouse_button(map_mouse_button(button));
        });
    }

    {
        let settings = Rc::clone(settings);
        window.on_precision_toggled(move |enabled| {
            settings.borrow_mut().set_precision(enabled);
        });
    }

    {
        let settings = Rc::clone(settings);
        window.on_speed_changed(move |speed| {
            settings.borrow_mut().set_pointer_speed(speed);
        });
    }

    {
        let settings = Rc::clone(settings);
        window.on_acceleration_changed(move |acceleration| {
            settings.borrow_mut().set_pointer_acceleration(acceleration);
        });
    }

    {
        let settings = Rc::clone(settings);
        let weak_window = window.as_weak();

        window.on_reset_defaults(move || {
            settings.borrow_mut().reset_defaults();

            if let Some(window) = weak_window.upgrade() {
                window.set_active_button(MouseButtonMode::Left);
                window.set_precision_enabled(false);
                window.set_pointer_speed(DEFAULT_POINTER_SPEED);
                window.set_pointer_acceleration(DEFAULT_POINTER_ACCELERATION);
            }
        });
    }
}

pub fn run() -> Result<(), AppError> {
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting NumFlow");

    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;
    let settings = Rc::new(RefCell::new(UiSettings::default()));

    connect_ui(&window, &settings);

    window
        .run()
        .map_err(|error| AppError::Ui(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_POINTER_ACCELERATION, DEFAULT_POINTER_SPEED, UiSettings};
    use numflow_core::{MotionConfig, MouseButton};

    #[test]
    fn ui_defaults_match_core_motion_defaults() {
        let defaults = MotionConfig::default();

        assert!((defaults.base_speed - f64::from(DEFAULT_POINTER_SPEED)).abs() <= f64::EPSILON);
        assert!(
            (defaults.acceleration - f64::from(DEFAULT_POINTER_ACCELERATION)).abs() <= f64::EPSILON
        );
    }

    #[test]
    fn pointer_controls_update_core_motion_config() {
        let mut settings = UiSettings::default();

        settings.set_pointer_speed(420.0);
        settings.set_pointer_acceleration(1_600.0);

        assert!((settings.motion.base_speed - 420.0).abs() <= f64::EPSILON);
        assert!((settings.motion.acceleration - 1_600.0).abs() <= f64::EPSILON);
    }

    #[test]
    fn reset_restores_settings_without_disabling_runtime_state() {
        let mut settings = UiSettings::default();
        settings.set_enabled(true);
        settings.set_mouse_button(MouseButton::Right);
        settings.set_precision(true);
        settings.set_pointer_speed(520.0);
        settings.set_pointer_acceleration(2_000.0);

        settings.reset_defaults();

        assert!(settings.controller.is_enabled());
        assert_eq!(settings.controller.selected_button(), MouseButton::Left);
        assert!(!settings.controller.is_precision_enabled());
        assert!((settings.motion.base_speed - 180.0).abs() <= f64::EPSILON);
        assert!((settings.motion.acceleration - 900.0).abs() <= f64::EPSILON);
    }
}
