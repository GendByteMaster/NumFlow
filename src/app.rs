use std::{cell::RefCell, rc::Rc};

use num_traits::ToPrimitive;
use numflow_core::{Bindings, ControllerState, CoreEffect, InputAction, MotionConfig, MouseButton};
use slint::ComponentHandle;

use crate::{
    AppWindow, MouseButtonMode,
    config::{AppConfig, ConfigError, ConfigLoadStatus, ConfigStore},
    error::AppError,
    hud::{HudController, HudEvent},
};

const DEFAULT_POINTER_SPEED: f32 = 180.0;
const DEFAULT_POINTER_ACCELERATION: f32 = 900.0;

type SharedUiSettings = Rc<RefCell<UiSettings>>;
type SharedHud = Rc<RefCell<HudController>>;
type SharedConfigStore = Rc<ConfigStore>;

#[derive(Debug)]
struct UiSettings {
    controller: ControllerState,
    motion: MotionConfig,
    bindings: Bindings,
    config: AppConfig,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self::from_config(AppConfig::default())
    }
}

impl UiSettings {
    fn from_config(config: AppConfig) -> Self {
        let profile = config.active_profile().clone();
        let mut controller = ControllerState::default();
        controller.apply(InputAction::SelectButton(profile.selected_button.into()));
        controller.apply(InputAction::SetPrecision(profile.precision_enabled));

        Self {
            controller,
            motion: profile.motion_config(),
            bindings: profile.bindings(),
            config,
        }
    }

    fn set_enabled(&mut self, enabled: bool) -> Vec<CoreEffect> {
        self.controller.apply(InputAction::SetEnabled(enabled))
    }

    fn set_mouse_button(&mut self, button: MouseButton) -> Vec<CoreEffect> {
        self.config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist")
            .selected_button = button.into();
        self.controller.apply(InputAction::SelectButton(button))
    }

    fn set_precision(&mut self, enabled: bool) -> Vec<CoreEffect> {
        self.config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist")
            .precision_enabled = enabled;
        self.controller.apply(InputAction::SetPrecision(enabled))
    }

    fn set_pointer_speed(&mut self, speed: f32) {
        let mut motion = self.motion;
        motion.base_speed = f64::from(speed);
        self.motion = motion.sanitized();
        self.config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist")
            .speed = self.motion.base_speed;
    }

    fn set_pointer_acceleration(&mut self, acceleration: f32) {
        let mut motion = self.motion;
        motion.acceleration = f64::from(acceleration);
        self.motion = motion.sanitized();
        self.config
            .profiles
            .get_mut(&self.config.active_profile)
            .expect("active profile must exist")
            .acceleration = self.motion.acceleration;
    }

    fn set_hud_enabled(&mut self, enabled: bool) {
        self.config.hud_enabled = enabled;
    }

    fn set_profile(&mut self, profile_name: &str) -> Result<Vec<CoreEffect>, ConfigError> {
        self.config.set_active_profile(profile_name)?;
        let profile = self.config.active_profile().clone();

        let mut effects = self
            .controller
            .apply(InputAction::SelectButton(profile.selected_button.into()));
        effects.extend(
            self.controller
                .apply(InputAction::SetPrecision(profile.precision_enabled)),
        );
        self.motion = profile.motion_config();
        self.bindings = profile.bindings();

        Ok(effects)
    }

    fn held_button(&self) -> Option<MouseButton> {
        self.controller.held_button()
    }

    fn hud_enabled(&self) -> bool {
        self.config.hud_enabled
    }

    fn active_profile_name(&self) -> &str {
        &self.config.active_profile
    }

    fn binding_count(&self) -> usize {
        self.bindings.iter().count()
    }

    fn config_snapshot(&self) -> AppConfig {
        self.config.clone()
    }

    fn reset_defaults(&mut self) -> Vec<CoreEffect> {
        self.config = AppConfig::default();
        let profile = self.config.active_profile().clone();

        let mut effects = self
            .controller
            .apply(InputAction::SelectButton(profile.selected_button.into()));
        effects.extend(
            self.controller
                .apply(InputAction::SetPrecision(profile.precision_enabled)),
        );
        self.motion = profile.motion_config();
        self.bindings = profile.bindings();
        effects
    }
}

fn map_mouse_button(button: MouseButtonMode) -> MouseButton {
    match button {
        MouseButtonMode::Left => MouseButton::Left,
        MouseButtonMode::Right => MouseButton::Right,
        MouseButtonMode::Middle => MouseButton::Middle,
    }
}

fn map_mouse_button_to_ui(button: MouseButton) -> MouseButtonMode {
    match button {
        MouseButton::Left => MouseButtonMode::Left,
        MouseButton::Right => MouseButtonMode::Right,
        MouseButton::Middle => MouseButtonMode::Middle,
    }
}

fn ui_float(value: f64, fallback: f32) -> f32 {
    value.to_f32().unwrap_or(fallback)
}

fn sync_window_from_settings(window: &AppWindow, settings: &UiSettings) {
    window.set_active_button(map_mouse_button_to_ui(
        settings.controller.selected_button(),
    ));
    window.set_precision_enabled(settings.controller.is_precision_enabled());
    window.set_pointer_speed(ui_float(settings.motion.base_speed, DEFAULT_POINTER_SPEED));
    window.set_pointer_acceleration(ui_float(
        settings.motion.acceleration,
        DEFAULT_POINTER_ACCELERATION,
    ));
    window.set_hud_enabled(settings.hud_enabled());
    window.set_active_profile(settings.active_profile_name().into());
}

fn persist_configuration(settings: &SharedUiSettings, store: &ConfigStore) {
    let config = settings.borrow().config_snapshot();
    if let Err(error) = store.save(&config) {
        tracing::error!(%error, path = %store.path().display(), "failed to save NumFlow configuration");
    }
}

fn connect_pointer_controls(
    window: &AppWindow,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
) {
    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        window.on_enabled_toggled(move |enabled| {
            let effects = settings.borrow_mut().set_enabled(enabled);
            hud.borrow_mut().observe_effects(&effects);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        window.on_mouse_button_changed(move |button| {
            let effects = settings
                .borrow_mut()
                .set_mouse_button(map_mouse_button(button));
            hud.borrow_mut().observe_effects(&effects);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        window.on_precision_toggled(move |enabled| {
            let effects = settings.borrow_mut().set_precision(enabled);
            hud.borrow_mut().observe_effects(&effects);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        window.on_speed_changed(move |speed| {
            settings.borrow_mut().set_pointer_speed(speed);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let store = Rc::clone(store);
        window.on_acceleration_changed(move |acceleration| {
            settings.borrow_mut().set_pointer_acceleration(acceleration);
            persist_configuration(&settings, &store);
        });
    }
}

fn connect_preferences(
    window: &AppWindow,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
) {
    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        window.on_hud_toggled(move |enabled| {
            let held_button = {
                let mut settings = settings.borrow_mut();
                settings.set_hud_enabled(enabled);
                settings.held_button()
            };

            let mut hud = hud.borrow_mut();
            hud.set_enabled(enabled);
            if enabled {
                hud.sync_held_button(held_button);
                hud.show_event(HudEvent::HudEnabled);
            }
            drop(hud);
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        let weak_window = window.as_weak();
        window.on_profile_changed(move |profile| {
            let effects = match settings.borrow_mut().set_profile(profile.as_str()) {
                Ok(effects) => effects,
                Err(error) => {
                    tracing::error!(%error, profile = %profile, "failed to activate profile");
                    return;
                }
            };

            hud.borrow_mut().observe_effects(&effects);
            if let Some(window) = weak_window.upgrade() {
                sync_window_from_settings(&window, &settings.borrow());
            }
            persist_configuration(&settings, &store);
        });
    }

    {
        let settings = Rc::clone(settings);
        let hud = Rc::clone(hud);
        let store = Rc::clone(store);
        let weak_window = window.as_weak();
        window.on_reset_defaults(move || {
            let (effects, held_button) = {
                let mut settings = settings.borrow_mut();
                let effects = settings.reset_defaults();
                (effects, settings.held_button())
            };

            {
                let mut hud = hud.borrow_mut();
                hud.set_enabled(true);
                hud.observe_effects(&effects);
                hud.sync_held_button(held_button);
                hud.show_event(HudEvent::DefaultsRestored);
            }

            if let Some(window) = weak_window.upgrade() {
                sync_window_from_settings(&window, &settings.borrow());
            }
            persist_configuration(&settings, &store);
        });
    }
}

fn connect_ui(
    window: &AppWindow,
    settings: &SharedUiSettings,
    hud: &SharedHud,
    store: &SharedConfigStore,
) {
    connect_pointer_controls(window, settings, hud, store);
    connect_preferences(window, settings, hud, store);
}

pub fn run() -> Result<(), AppError> {
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "starting NumFlow");

    let store = Rc::new(ConfigStore::for_current_user()?);
    let loaded = store.load_or_default()?;
    let should_write_defaults = match &loaded.status {
        ConfigLoadStatus::Loaded => false,
        ConfigLoadStatus::Missing => {
            tracing::info!(path = %store.path().display(), "configuration missing; using safe defaults");
            true
        }
        ConfigLoadStatus::Recovered { reason } => {
            tracing::warn!(%reason, path = %store.path().display(), "invalid configuration recovered to safe defaults");
            true
        }
    };

    if should_write_defaults {
        store.save(&loaded.config)?;
    }

    let window = AppWindow::new().map_err(|error| AppError::Ui(error.to_string()))?;
    let settings = Rc::new(RefCell::new(UiSettings::from_config(loaded.config)));
    sync_window_from_settings(&window, &settings.borrow());

    let hud = Rc::new(RefCell::new(
        HudController::new().map_err(|error| AppError::Ui(error.to_string()))?,
    ));
    hud.borrow_mut()
        .set_enabled(settings.borrow().hud_enabled());

    tracing::info!(
        profile = settings.borrow().active_profile_name(),
        bindings = settings.borrow().binding_count(),
        path = %store.path().display(),
        "configuration ready"
    );

    connect_ui(&window, &settings, &hud, &store);

    window
        .run()
        .map_err(|error| AppError::Ui(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_POINTER_ACCELERATION, DEFAULT_POINTER_SPEED, UiSettings};
    use crate::config::AppConfig;
    use numflow_core::{InputAction, MotionConfig, MouseButton};

    #[test]
    fn ui_defaults_match_core_motion_defaults() {
        let defaults = MotionConfig::default();

        assert!((defaults.base_speed - f64::from(DEFAULT_POINTER_SPEED)).abs() <= f64::EPSILON);
        assert!(
            (defaults.acceleration - f64::from(DEFAULT_POINTER_ACCELERATION)).abs() <= f64::EPSILON
        );
    }

    #[test]
    fn pointer_controls_update_profile_and_motion_config() {
        let mut settings = UiSettings::default();

        settings.set_pointer_speed(420.0);
        settings.set_pointer_acceleration(1_600.0);

        assert!((settings.motion.base_speed - 420.0).abs() <= f64::EPSILON);
        assert!((settings.motion.acceleration - 1_600.0).abs() <= f64::EPSILON);
        assert!((settings.config.active_profile().speed - 420.0).abs() <= f64::EPSILON);
        assert!((settings.config.active_profile().acceleration - 1_600.0).abs() <= f64::EPSILON);
    }

    #[test]
    fn hud_feedback_is_enabled_by_default() {
        assert!(UiSettings::default().hud_enabled());
    }

    #[test]
    fn persisted_profile_is_applied_to_runtime_state() {
        let config = AppConfig {
            active_profile: "Precision".to_owned(),
            hud_enabled: false,
            ..AppConfig::default()
        };
        let settings = UiSettings::from_config(config);

        assert_eq!(settings.active_profile_name(), "Precision");
        assert!(settings.controller.is_precision_enabled());
        assert!(!settings.hud_enabled());
        assert!((settings.motion.base_speed - 130.0).abs() <= f64::EPSILON);
        assert_eq!(settings.binding_count(), 15);
    }

    #[test]
    fn profile_switch_updates_runtime_controls() {
        let mut settings = UiSettings::default();

        settings
            .set_profile("Fast")
            .expect("built-in Fast profile should exist");

        assert_eq!(settings.active_profile_name(), "Fast");
        assert!((settings.motion.base_speed - 300.0).abs() <= f64::EPSILON);
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
        settings.set_hud_enabled(false);
        settings
            .set_profile("Fast")
            .expect("built-in Fast profile should exist");

        settings.reset_defaults();

        assert!(settings.controller.is_enabled());
        assert_eq!(settings.controller.selected_button(), MouseButton::Left);
        assert!(!settings.controller.is_precision_enabled());
        assert!((settings.motion.base_speed - 180.0).abs() <= f64::EPSILON);
        assert!((settings.motion.acceleration - 900.0).abs() <= f64::EPSILON);
        assert!(settings.hud_enabled());
        assert_eq!(settings.active_profile_name(), "Normal");
    }

    #[test]
    fn reset_does_not_drop_an_active_drag() {
        let mut settings = UiSettings::default();
        settings.set_enabled(true);
        settings.controller.apply(InputAction::Hold);

        settings.reset_defaults();

        assert_eq!(settings.controller.held_button(), Some(MouseButton::Left));
    }
}
